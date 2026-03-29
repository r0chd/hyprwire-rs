use super::server_object;
use crate::implementation::wire_object::WireObject;
use crate::message::Message;
use crate::{SharedState, message, steady_millis, trace};
use nix::sys;
use std::ffi;
use std::ops;
use std::os::fd::AsRawFd;
use std::{cell, rc};

/// A handle to a connected client managed by a [`super::Server`].
#[derive(Clone)]
pub struct ServerClientHandle(pub(crate) u32);

pub(crate) struct ServerClient {
    pub(crate) id: u32,
    pub(crate) pid: cell::Cell<i32>,
    pub(crate) first_poll_done: cell::Cell<bool>,
    pub(crate) version: cell::Cell<u32>,
    pub(crate) max_id: cell::Cell<u32>,
    pub(crate) state: rc::Rc<SharedState>,
    pub(crate) scheduled_roundtrip_seq: cell::Cell<u32>,
    pub(crate) objects: cell::RefCell<Vec<rc::Rc<server_object::ServerObject>>>,
    self_ref: rc::Weak<cell::RefCell<Self>>,
}

impl ServerClient {
    pub(crate) fn new(id: u32, state: rc::Rc<SharedState>) -> rc::Rc<cell::RefCell<Self>> {
        rc::Rc::new_cyclic(|weak_self| {
            cell::RefCell::new(Self {
                id,
                pid: cell::Cell::new(0),
                first_poll_done: cell::Cell::new(false),
                version: cell::Cell::new(0),
                max_id: cell::Cell::new(1),
                state,
                scheduled_roundtrip_seq: cell::Cell::new(0),
                objects: cell::RefCell::new(Vec::new()),
                self_ref: weak_self.clone(),
            })
        })
    }

    #[allow(dead_code)]
    pub(crate) fn get_pid(&self) -> i32 {
        self.pid.get()
    }

    pub(crate) fn dispatch_first_poll(&self) {
        if self.first_poll_done.get() {
            return;
        }
        self.first_poll_done.set(true);

        match sys::socket::getsockopt(&self.state.stream, sys::socket::sockopt::PeerCredentials) {
            Ok(cred) => {
                self.pid.set(cred.pid());
                trace! {
                    eprintln!(
                        "[hw] trace: [{} @ {:.3}] peer pid: {}",
                        self.state.stream.as_raw_fd(),
                        steady_millis(),
                        self.pid.get()
                    )
                }
            }
            Err(_) => {
                trace! {
                    eprintln!("[hw] trace: dispatchFirstPoll: failed to get pid")
                }
            }
        }
    }

    pub(crate) fn protocol_names(&self) -> Vec<String> {
        let impls = self.state.impls.as_ref().unwrap();
        impls
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

    pub(crate) fn create_object(
        &self,
        protocol: &str,
        object_name: &str,
        version: u32,
        seq: u32,
    ) -> rc::Rc<server_object::ServerObject> {
        let mut server_obj =
            server_object::ServerObject::new(self.self_ref.clone(), rc::Rc::clone(&self.state));
        server_obj.id.set(self.max_id.get());
        self.max_id.set(self.max_id.get() + 1);
        server_obj.version.set(version);
        server_obj.seq = seq;
        server_obj.protocol_name = protocol.to_string();

        let impls = self.state.impls.as_ref().unwrap();
        for imp in impls.iter() {
            if imp.protocol().spec_name() == protocol {
                for spec in imp.protocol().objects() {
                    if object_name.is_empty() || spec.object_name() == object_name {
                        server_obj.spec = Some(std::sync::Arc::clone(spec));
                        break;
                    }
                }
                break;
            }
        }

        let obj = rc::Rc::new(server_obj);
        self.objects.borrow_mut().push(rc::Rc::clone(&obj));

        let new_obj_msg = message::NewObject::new(seq, obj.id.get());
        self.state.send_message(&new_obj_msg);

        self.on_bind(rc::Rc::clone(&obj));

        obj
    }

    pub(crate) fn on_bind(&self, obj: rc::Rc<server_object::ServerObject>) {
        let protocol_name = obj.protocol_name.clone();
        let object_name = obj
            .spec
            .as_ref()
            .map(|spec| spec.object_name().to_string())
            .unwrap_or_default();

        let impls = self.state.impls.as_ref().unwrap();
        for imp in impls.iter() {
            if imp.protocol().spec_name() == protocol_name {
                if let Some(obj_impl) = imp
                    .implementation()
                    .iter()
                    .find(|impl_obj| impl_obj.object_name == object_name)
                {
                    (obj_impl.on_bind)(obj as rc::Rc<dyn crate::implementation::object::RawObject>);
                }
                return;
            }
        }
    }

    pub(crate) fn on_generic(
        &self,
        msg: &message::GenericProtocolMessage<ops::Range<usize>>,
        dispatch: *mut ffi::c_void,
    ) {
        let obj = self
            .objects
            .borrow()
            .iter()
            .find(|obj| obj.id.get() == msg.object())
            .map(rc::Rc::clone);

        match obj {
            Some(obj) => {
                if let Err(e) = obj.called(msg.method(), msg.data_span(), msg.fds(), dispatch) {
                    log::error!(
                        "[{} @ {:.3}] object {} called method error: {e}",
                        self.state.stream.as_raw_fd(),
                        steady_millis(),
                        msg.object(),
                    );
                }
            }
            None => {
                trace! {
                    eprintln!(
                        "[hw] trace: [{} @ {:.3}] -> Generic message not handled. No object with id {}!",
                        self.state.stream.as_raw_fd(),
                        steady_millis(),
                        msg.object(),
                    )
                }
            }
        }
    }
}

impl Drop for ServerClient {
    fn drop(&mut self) {
        trace! {
            eprintln!("[hw] trace: [{}] destroying client", self.state.stream.as_raw_fd())
        }
    }
}
