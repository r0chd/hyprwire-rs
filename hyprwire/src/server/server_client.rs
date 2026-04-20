use super::server_object;
use crate::SharedState;
use crate::implementation::wire_object::WireObject;
use crate::message::Message;
use crate::{message, steady_millis, trace};
use nix::sys::socket;
use nix::sys::socket::sockopt;
use std::hash::{Hash, Hasher};
use std::os::fd::AsRawFd;
use std::{cell, ops, rc};

/// A handle to a connected client managed by a [`super::Server`].
#[derive(Clone, Debug)]
pub struct ServerClient {
    pub(crate) id: u32,
    pub(crate) creds: rc::Rc<cell::OnceCell<socket::UnixCredentials>>,
}

impl PartialEq for ServerClient {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for ServerClient {}

impl Hash for ServerClient {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl ServerClient {
    /// Returns the server-local client id for this handle.
    #[must_use]
    pub fn id(&self) -> u32 {
        self.id
    }

    /// Returns the peer process id reported by the Unix socket credentials.
    #[must_use]
    pub fn creds(&self) -> &socket::UnixCredentials {
        // SAFETY: creds are set on first dispatch
        // objects can only be created by client and
        // servers can bind them only from callbacks
        // which are ran after dispatching
        #[allow(clippy::missing_panics_doc)]
        self.creds.get().unwrap()
    }
}

/// Server-side state for a connected client.
///
/// This type is attached to server-created objects and can be used to inspect
/// metadata about the peer connection.
pub(crate) struct ServerClientState {
    pub(crate) id: u32,
    pub(crate) creds: rc::Rc<cell::OnceCell<socket::UnixCredentials>>,
    pub(crate) first_poll_done: cell::Cell<bool>,
    pub(crate) version: cell::Cell<u32>,
    pub(crate) max_id: cell::Cell<u32>,
    pub(crate) state: rc::Rc<SharedState>,
    pub(crate) scheduled_roundtrip_seq: cell::Cell<u32>,
    pub(crate) objects: cell::RefCell<Vec<rc::Rc<server_object::ServerObject>>>,
    self_ref: rc::Weak<Self>,
}

impl ServerClientState {
    pub(crate) fn new(id: u32, state: rc::Rc<SharedState>) -> rc::Rc<Self> {
        rc::Rc::new_cyclic(|weak_self| Self {
            id,
            creds: rc::Rc::new(cell::OnceCell::new()),
            first_poll_done: cell::Cell::new(false),
            version: cell::Cell::new(0),
            max_id: cell::Cell::new(1),
            state,
            scheduled_roundtrip_seq: cell::Cell::new(0),
            objects: cell::RefCell::new(Vec::new()),
            self_ref: weak_self.clone(),
        })
    }

    pub fn handle(&self) -> ServerClient {
        ServerClient {
            id: self.id,
            creds: rc::Rc::clone(&self.creds),
        }
    }

    pub(crate) fn dispatch_first_poll(&self) {
        if self.first_poll_done.get() {
            return;
        }
        self.first_poll_done.set(true);

        match socket::getsockopt(&self.state.stream, sockopt::PeerCredentials) {
            Ok(cred) => {
                // SAFETY: dispatch_first_poll can only run once
                self.creds.set(cred).unwrap();
                trace! {
                    crate::log_debug!(
                        "[hw] trace: [{} @ {:.3}] peer pid: {}",
                        self.state.stream.as_raw_fd(),
                        steady_millis(),
                        cred.pid()
                    )
                }
            }
            Err(_) => {
                trace! {
                    crate::log_debug!("[hw] trace: dispatchFirstPoll: failed to get pid")
                }
            }
        }
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

        for imp in self.state.impls.borrow().iter() {
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

        for imp in self.state.impls.borrow().iter() {
            if imp.protocol().spec_name() == protocol_name {
                if let Some(obj_impl) = imp
                    .implementation()
                    .iter()
                    .find(|impl_obj| impl_obj.object_name == object_name)
                {
                    (obj_impl.on_bind)(obj as rc::Rc<dyn crate::implementation::object::Object>);
                }
                return;
            }
        }
    }

    pub(crate) fn destroy_object(&self, id: u32) {
        self.objects.borrow_mut().retain(|obj| obj.id.get() != id);
    }

    pub(crate) fn on_generic<D>(
        &self,
        msg: &message::GenericProtocolMessage<ops::Range<usize>>,
        dispatch: &mut D,
    ) {
        let obj = self
            .objects
            .borrow()
            .iter()
            .find(|obj| obj.id.get() == msg.object())
            .map(rc::Rc::clone);

        if let Some(obj) = obj {
            if let Err(e) = obj.called(msg.method(), msg.data_span(), msg.fds(), dispatch) {
                crate::log_error!(
                    "[{} @ {:.3}] object {} called method error: {e}",
                    self.state.stream.as_raw_fd(),
                    steady_millis(),
                    msg.object(),
                );
            }
        } else {
            let error = format!("generic message references unknown object {}", msg.object());
            crate::log_error!(
                "[{} @ {:.3}] {}",
                self.state.stream.as_raw_fd(),
                steady_millis(),
                error,
            );
            let fatal = message::FatalProtocolError::new(msg.object(), u32::MAX, &error);
            self.state.send_message(&fatal);
            self.state.error.set(true);
        }
    }

    pub(crate) fn destroy_objects_for_disconnect<D>(&self, dispatch: &mut D) {
        let objects = self
            .objects
            .borrow()
            .iter()
            .map(rc::Rc::clone)
            .collect::<Vec<_>>();

        for obj in objects.iter().rev() {
            obj.destroy_for_disconnect(dispatch);
        }

        self.objects.borrow_mut().clear();
    }
}

impl Drop for ServerClientState {
    fn drop(&mut self) {
        trace! {
            crate::log_debug!("[hw] trace: [{}] destroying client", self.state.stream.as_raw_fd())
        }
    }
}
