use super::{server_object, server_socket};
use crate::implementation::wire_object::WireObject;
use crate::message::Message;
use crate::{message, steady_millis, trace, SharedState};
use nix::sys;
use std::ops;
use std::{cell, rc, sync};

pub(crate) struct ServerClient {
    pub(crate) pid: i32,
    pub(crate) first_poll_done: bool,
    pub(crate) version: u32,
    pub(crate) max_id: u32,
    pub(crate) state: sync::Arc<SharedState>,
    pub(crate) scheduled_roundtrip_seq: u32,
    pub(crate) objects: Vec<rc::Rc<cell::RefCell<server_object::ServerObject>>>,
    server: sync::Weak<sync::RwLock<server_socket::ServerSocket>>,
}

impl ServerClient {
    pub fn new(
        state: sync::Arc<SharedState>,
        server: sync::Weak<sync::RwLock<server_socket::ServerSocket>>,
    ) -> Self {
        Self {
            pid: 0,
            first_poll_done: false,
            version: 0,
            max_id: 1,
            state,
            scheduled_roundtrip_seq: 0,
            objects: Vec::new(),
            server,
        }
    }

    pub fn get_pid(&self) -> i32 {
        self.pid
    }

    pub fn dispatch_first_poll(&mut self) {
        if self.first_poll_done {
            return;
        }
        self.first_poll_done = true;

        let stream = self.state.stream.lock().unwrap();
        match sys::socket::getsockopt(&*stream, sys::socket::sockopt::PeerCredentials) {
            Ok(cred) => {
                self.pid = cred.pid();
                trace! {
                    log::debug!(
                        "[{} @ {:.3}] peer pid: {}",
                        self.state.fd,
                        steady_millis(),
                        self.pid
                    )
                }
            }
            Err(e) => {
                log::error!(
                    "[{} @ {:.3}] failed to get peer credentials: {e}",
                    self.state.fd,
                    steady_millis(),
                );
            }
        }
    }

    pub fn protocol_names(&self) -> Vec<String> {
        self.server
            .upgrade()
            .unwrap()
            .read()
            .unwrap()
            .impls
            .iter()
            .map(|imp| {
                format!(
                    "{}@{}",
                    imp.protocol().spec_name(),
                    imp.protocol().spec_ver()
                )
            })
            .collect()
    }

    pub fn create_object(
        &mut self,
        protocol: &str,
        object_name: &str,
        version: u32,
        seq: u32,
    ) -> rc::Rc<cell::RefCell<server_object::ServerObject>> {
        let mut server_obj = server_object::ServerObject::new(sync::Arc::clone(&self.state));
        server_obj.id = self.max_id;
        self.max_id += 1;
        server_obj.version = version;
        server_obj.seq = seq;
        server_obj.protocol_name = protocol.to_string();

        for imp in &self.server.upgrade().unwrap().read().unwrap().impls {
            if imp.protocol().spec_name() == protocol {
                for spec in imp.protocol().objects() {
                    if object_name.is_empty() || spec.object_name() == object_name {
                        server_obj.spec = Some(sync::Arc::clone(spec));
                        break;
                    }
                }
                break;
            }
        }

        let obj = rc::Rc::new(cell::RefCell::new(server_obj));
        self.objects.push(rc::Rc::clone(&obj));

        let new_obj_msg = message::NewObject::new(seq, obj.borrow().id);
        self.state.send_message(&new_obj_msg);

        self.on_bind(rc::Rc::clone(&obj));

        obj
    }

    pub fn on_bind(&self, obj: rc::Rc<cell::RefCell<server_object::ServerObject>>) {
        let (protocol_name, object_name) = {
            let obj_ref = obj.borrow();
            let object_name = obj_ref
                .spec
                .as_ref()
                .map(|spec| spec.object_name().to_string())
                .unwrap_or_default();
            (obj_ref.protocol_name.clone(), object_name)
        };

        for imp in &self.server.upgrade().unwrap().read().unwrap().impls {
            if imp.protocol().spec_name() == protocol_name {
                if let Some(obj_impl) = imp
                    .implementation()
                    .iter()
                    .find(|impl_obj| impl_obj.object_name == object_name)
                {
                    (obj_impl.on_bind)(
                        obj as rc::Rc<cell::RefCell<dyn crate::implementation::object::Object>>,
                    );
                }
                return;
            }
        }
    }

    pub fn on_generic(&mut self, msg: &message::GenericProtocolMessage<ops::Range<usize>>) {
        if let Some(obj) = self
            .objects
            .iter()
            .find(|obj| obj.borrow().id == msg.object())
        {
            if let Err(e) = obj
                .borrow_mut()
                .called(msg.method(), msg.data_span(), msg.fds())
            {
                log::error!(
                    "[{} @ {:.3}] object {} called method error: {e}",
                    self.state.fd,
                    steady_millis(),
                    msg.object(),
                );
            }
            return;
        }

        log::debug!(
            "[{} @ {:.3}] -> Generic message not handled. No object with id {}!",
            self.state.fd,
            steady_millis(),
            msg.object(),
        );
    }
}
