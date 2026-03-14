use super::{server_object, server_socket};
use crate::implementation::wire_object::WireObject;
use crate::message::Message;
use crate::{message, steady_millis, trace};
use nix::{errno, poll, sys};
use std::ops;
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::net;
use std::{cell, io, rc};

pub(crate) struct ServerClient {
    pub(crate) stream: net::UnixStream,
    pub(crate) pid: i32,
    pub(crate) first_poll_done: bool,
    pub(crate) version: u32,
    pub(crate) max_id: u32,
    pub(crate) error: bool,
    pub(crate) scheduled_roundtrip_seq: u32,
    pub(crate) objects: Vec<rc::Rc<cell::RefCell<server_object::ServerObject>>>,
    pub(crate) pending_binds: Vec<rc::Rc<cell::RefCell<server_object::ServerObject>>>,
    // SAFETY: ServerClient is always accessed from within ServerSocket methods,
    // and ServerSocket outlives its clients. We use a raw pointer to avoid
    // deadlocking on the RwLock that wraps ServerSocket.
    server: *const server_socket::ServerSocket,
    _self: rc::Weak<cell::RefCell<Self>>,
}

// SAFETY: ServerClient is only accessed from within ServerSocket which is behind Arc<RwLock>
unsafe impl Send for ServerClient {}
unsafe impl Sync for ServerClient {}

impl ServerClient {
    pub fn new(
        stream: net::UnixStream,
        server: *const server_socket::ServerSocket,
        weak_self: rc::Weak<cell::RefCell<Self>>,
    ) -> Self {
        Self {
            stream,
            pid: 0,
            first_poll_done: false,
            version: 0,
            max_id: 1,
            error: false,
            scheduled_roundtrip_seq: 0,
            objects: Vec::new(),
            pending_binds: Vec::new(),
            server,
            _self: weak_self,
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

        match sys::socket::getsockopt(&self.stream, sys::socket::sockopt::PeerCredentials) {
            Ok(cred) => {
                self.pid = cred.pid();
                trace! {
                    log::debug!(
                        "[{} @ {:.3}] peer pid: {}",
                        self.stream.as_raw_fd(),
                        steady_millis(),
                        self.pid
                    )
                }
            }
            Err(e) => {
                log::error!(
                    "[{} @ {:.3}] failed to get peer credentials: {e}",
                    self.stream.as_raw_fd(),
                    steady_millis(),
                );
            }
        }
    }

    pub fn protocol_names(&self) -> Vec<String> {
        // SAFETY: server pointer is valid as long as ServerSocket is alive,
        // and we are always called from within ServerSocket methods.
        let server = unsafe { &*self.server };
        server
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

    pub fn create_object(&mut self, protocol: &str, object_name: &str, version: u32, seq: u32) {
        let obj = rc::Rc::new_cyclic(|weak_obj| {
            let mut server_obj =
                server_object::ServerObject::new(self._self.clone(), weak_obj.clone());
            server_obj.id = self.max_id;
            self.max_id += 1;
            server_obj.version = version;
            server_obj.seq = seq;
            server_obj.protocol_name = protocol.to_string();

            // Find spec from server implementations
            // SAFETY: server pointer valid, called from within ServerSocket methods
            let server = unsafe { &*self.server };
            for imp in &server.impls {
                if imp.protocol().spec_name() == protocol {
                    for spec in imp.protocol().objects() {
                        if object_name.is_empty() || spec.object_name() == object_name {
                            // SAFETY: spec comes from server impls which outlive the objects
                            server_obj.spec =
                                Some(unsafe { std::mem::transmute(*spec as *const _) });
                            break;
                        }
                    }
                    break;
                }
            }

            cell::RefCell::new(server_obj)
        });

        let new_obj_msg = message::NewObject::new(seq, obj.borrow().id);
        self.send_message(&new_obj_msg);

        self.objects.push(rc::Rc::clone(&obj));
        self.pending_binds.push(obj);
    }

    pub fn take_pending_binds(
        &mut self,
    ) -> Vec<rc::Rc<cell::RefCell<server_object::ServerObject>>> {
        std::mem::take(&mut self.pending_binds)
    }

    pub fn on_bind(&self, obj: rc::Rc<cell::RefCell<server_object::ServerObject>>) {
        let (protocol_name, object_name) = {
            let obj_ref = obj.borrow();
            let object_name = obj_ref
                .spec
                .map(|spec| unsafe { &*spec }.object_name().to_string())
                .unwrap_or_default();
            (obj_ref.protocol_name.clone(), object_name)
        };

        // SAFETY: server pointer valid, called from within ServerSocket methods
        let server = unsafe { &*self.server };
        for imp in &server.impls {
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
                    self.stream.as_raw_fd(),
                    steady_millis(),
                    msg.object(),
                );
            }
            return;
        }

        log::debug!(
            "[{} @ {:.3}] -> Generic message not handled. No object with id {}!",
            self.stream.as_raw_fd(),
            steady_millis(),
            msg.object(),
        );
    }

    pub fn send_message<T>(&self, message: &T)
    where
        T: message::Message + ?Sized,
    {
        trace! { log::trace!("[{} @ {:.3}] -> {}", self.stream.as_raw_fd(), steady_millis(), message.parse_data()) };

        let buf = message.data();
        let iov = [io::IoSlice::new(buf)];
        let cmsg = [sys::socket::ControlMessage::ScmRights(message.fds())];
        loop {
            match sys::socket::sendmsg::<()>(
                self.stream.as_raw_fd(),
                &iov,
                &cmsg,
                sys::socket::MsgFlags::empty(),
                None,
            ) {
                Ok(_) => break,
                Err(errno::Errno::EAGAIN) => {
                    let mut pfd = [poll::PollFd::new(
                        self.stream.as_fd(),
                        poll::PollFlags::POLLOUT | poll::PollFlags::POLLWRBAND,
                    )];
                    if let Err(e) = poll::poll(&mut pfd, poll::PollTimeout::NONE) {
                        log::error!(
                            "[{} @ {:.3}] poll error during send_message: {e}",
                            self.stream.as_raw_fd(),
                            steady_millis(),
                        );
                        break;
                    }
                    continue;
                }
                Err(_) => {
                    break;
                }
            }
        }
    }
}
