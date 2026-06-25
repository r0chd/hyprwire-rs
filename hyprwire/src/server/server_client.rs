use super::server_object;
use crate::ConnectionState;
use crate::implementation;
use crate::implementation::object::Object;
use crate::{steady_millis, trace};
use hyprwire_core::message::Message;
use hyprwire_core::message::wire::{fatal_protocol_error, generic_protocol_message, new_object};
use rustix::net;
use rustix::net::sockopt;
use std::os::fd::AsRawFd;
use std::sync::atomic;
use std::{hash, ops, sync};

/// A handle to a connected client managed by a [`super::Server`].
#[derive(Clone, Debug)]
pub struct ServerClient {
    pub(crate) id: u32,
    pub(crate) creds: sync::Arc<sync::OnceLock<net::UCred>>,
}

impl PartialEq for ServerClient {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for ServerClient {}

impl hash::Hash for ServerClient {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl ServerClient {
    /// Returns the server-local client id for this handle.
    #[must_use]
    pub const fn id(&self) -> u32 {
        self.id
    }

    /// Returns the peer process id reported by the Unix socket credentials.
    #[must_use]
    #[allow(clippy::missing_panics_doc)]
    #[allow(clippy::unwrap_used)]
    pub fn creds(&self) -> &net::UCred {
        // creds are set on first dispatch
        // objects can only be created by client and
        // servers can bind them only from callbacks
        // which are ran after dispatching
        // in short creds are always set at this point
        self.creds.get().unwrap()
    }
}

/// Server-side state for a connected client.
///
/// This type is attached to server-created objects and can be used to inspect
/// metadata about the peer connection.
pub(crate) struct ServerClientState {
    pub(crate) id: u32,
    pub(crate) creds: sync::Arc<sync::OnceLock<net::UCred>>,
    pub(crate) first_poll_done: atomic::AtomicBool,
    pub(crate) version: atomic::AtomicU32,
    pub(crate) max_id: atomic::AtomicU32,
    pub(crate) state: sync::Arc<ConnectionState>,
    pub(crate) impls:
        sync::Arc<sync::RwLock<Vec<Box<dyn implementation::server::ProtocolImplementations>>>>,
    pub(crate) scheduled_roundtrip_seq: atomic::AtomicU32,
    pub(crate) objects: sync::Mutex<Vec<sync::Arc<server_object::ServerObject>>>,
    self_ref: sync::Weak<Self>,
}

impl ServerClientState {
    pub(crate) fn new(
        id: u32,
        state: sync::Arc<ConnectionState>,
        impls: sync::Arc<
            sync::RwLock<Vec<Box<dyn implementation::server::ProtocolImplementations>>>,
        >,
    ) -> sync::Arc<Self> {
        sync::Arc::new_cyclic(|weak_self| Self {
            id,
            creds: sync::Arc::new(sync::OnceLock::new()),
            first_poll_done: atomic::AtomicBool::new(false),
            version: atomic::AtomicU32::new(0),
            max_id: atomic::AtomicU32::new(1),
            state,
            impls,
            scheduled_roundtrip_seq: atomic::AtomicU32::new(0),
            objects: sync::Mutex::new(Vec::new()),
            self_ref: weak_self.clone(),
        })
    }

    pub fn handle(&self) -> ServerClient {
        ServerClient {
            id: self.id,
            creds: sync::Arc::clone(&self.creds),
        }
    }

    pub(crate) fn dispatch_first_poll(&self) {
        if self.first_poll_done.load(atomic::Ordering::Relaxed) {
            return;
        }
        self.first_poll_done.store(true, atomic::Ordering::Relaxed);

        match sockopt::socket_peercred(&self.state.stream) {
            Ok(cred) => {
                // SAFETY: dispatch_first_poll can only run once
                let _ = self.creds.set(cred);
                trace! {
                    crate::log_debug!(
                        "[hw] trace: [{} @ {:.3}] peer pid: {}",
                        self.state.stream.as_raw_fd(),
                        steady_millis(),
                        cred.pid
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
    ) -> sync::Arc<server_object::ServerObject> {
        let mut server_obj =
            server_object::ServerObject::new(self.self_ref.clone(), sync::Arc::clone(&self.state));
        let id = self.max_id.fetch_add(1, atomic::Ordering::Relaxed);
        server_obj.id.store(id, atomic::Ordering::Relaxed);
        server_obj.version.store(version, atomic::Ordering::Relaxed);
        server_obj.seq = seq;
        server_obj.protocol_name = protocol.to_string();

        let impls = sync::Arc::clone(&self.impls);
        for imp in &(*impls.read().unwrap_or_else(sync::PoisonError::into_inner)) {
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

        let obj = sync::Arc::new(server_obj);
        let new_obj_msg = new_object::NewObject::new(seq, obj.id.load(atomic::Ordering::Relaxed));
        self.state.send_message(&new_obj_msg);

        self.objects
            .lock()
            .unwrap_or_else(sync::PoisonError::into_inner)
            .push(sync::Arc::clone(&obj));
        self.on_bind(sync::Arc::clone(&obj));

        obj
    }

    pub(crate) fn on_bind(&self, obj: sync::Arc<server_object::ServerObject>) {
        let protocol_name = obj.protocol_name.clone();
        let object_name = obj
            .spec
            .as_ref()
            .map(|spec| spec.object_name().to_string())
            .unwrap_or_default();

        let impls = sync::Arc::clone(&self.impls);
        for imp in &(*impls.read().unwrap_or_else(sync::PoisonError::into_inner)) {
            if imp.protocol().spec_name() == protocol_name {
                if let Some(obj_impl) = imp
                    .implementation()
                    .iter()
                    .find(|impl_obj| impl_obj.object_name == object_name)
                {
                    (obj_impl.on_bind)(obj as sync::Arc<dyn crate::implementation::object::Object>);
                }
                return;
            }
        }
    }

    pub(crate) fn destroy_object(&self, id: u32) {
        self.objects
            .lock()
            .unwrap_or_else(sync::PoisonError::into_inner)
            .retain(|obj| obj.id.load(atomic::Ordering::Relaxed) != id);
    }

    pub(crate) fn on_generic<D: 'static>(
        &self,
        msg: &generic_protocol_message::GenericProtocolMessage<ops::Range<usize>>,
        dispatch: &mut D,
    ) {
        let obj = {
            self.objects
                .lock()
                .unwrap_or_else(sync::PoisonError::into_inner)
                .iter()
                .find(|obj| obj.id.load(atomic::Ordering::Relaxed) == msg.object())
                .map(sync::Arc::clone)
        };

        if let Some(obj) = obj {
            obj.dispatch(msg.method(), msg.data_span(), msg.fds(), dispatch);

            // Handle destructor methods
            if let Some(spec) = &obj.spec
                && let Some(method) = spec.c2s().get(msg.method() as usize)
                && method.destructor
            {
                obj.destroyed.store(true, atomic::Ordering::Relaxed);
                let id = obj.id.load(atomic::Ordering::Relaxed);
                if id != 0
                    && let Some(client) = obj.client.upgrade()
                {
                    client.destroy_object(id);
                }
            }
        } else {
            let error = format!("generic message references unknown object {}", msg.object());
            crate::log_error!(
                "[{} @ {:.3}] {}",
                self.state.stream.as_raw_fd(),
                steady_millis(),
                error,
            );
            let fatal =
                fatal_protocol_error::FatalProtocolError::new(msg.object(), u32::MAX, &error);
            self.state.send_message(&fatal);
            self.state.error.store(true, atomic::Ordering::Relaxed);
        }
    }

    pub(crate) fn destroy_objects_for_disconnect<D: 'static>(&self, dispatch: &mut D) {
        let objects = self
            .objects
            .lock()
            .unwrap_or_else(sync::PoisonError::into_inner)
            .iter()
            .map(sync::Arc::clone)
            .collect::<Vec<_>>();

        for obj in objects.iter().rev() {
            obj.destroy_for_disconnect(dispatch);
        }

        self.objects
            .lock()
            .unwrap_or_else(sync::PoisonError::into_inner)
            .clear();
    }
}

impl Drop for ServerClientState {
    fn drop(&mut self) {
        trace! {
            crate::log_debug!("[hw] trace: [{}] destroying client", self.state.stream.as_raw_fd())
        }
    }
}
