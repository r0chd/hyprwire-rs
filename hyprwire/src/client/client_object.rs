use super::event_queue;
use crate::client::client_socket;
use crate::implementation::wire_object::WireObject;
use crate::implementation::{object, wire_object};
use crate::{client, trace};
use hyprwire_core::{message, types};
use std::sync::atomic;
use std::{any, sync};

pub struct ClientObject {
    client: sync::Weak<client_socket::ClientSocket>,
    pub(crate) state: sync::Arc<crate::ConnectionState>,
    pub(crate) spec: Option<sync::Arc<dyn types::ProtocolObjectSpec>>,
    pub(crate) id: atomic::AtomicU32,
    pub(crate) version: atomic::AtomicU32,
    pub(crate) seq: u32,
    pub(crate) protocol_name: String,
    object_data: sync::RwLock<Option<Box<dyn object::ObjectData>>>,
    pub(crate) destroyed: atomic::AtomicBool,
    event_queue: event_queue::WeakEventQueue,
}

impl Drop for ClientObject {
    fn drop(&mut self) {
        if !self.destroyed.load(atomic::Ordering::Relaxed)
            && self.id.load(atomic::Ordering::Relaxed) != 0
            && self.spec.is_some()
            && self.client.upgrade().is_some()
        {
            let methods = self.methods_out();
            if let Some(destructor) = methods.iter().find(|method| {
                method.destructor && method.since <= self.version.load(atomic::Ordering::Relaxed)
            }) {
                if !destructor.returns_type.is_empty() {
                    crate::log_debug!(
                        "can't auto-call destructor for object {}: method {} has returns type",
                        self.id.load(atomic::Ordering::Relaxed),
                        destructor.idx
                    );
                    return;
                }

                if !destructor.params.is_empty() {
                    crate::log_debug!(
                        "can't auto-call destructor for object {}: method {} has params",
                        self.id.load(atomic::Ordering::Relaxed),
                        destructor.idx
                    );
                    return;
                }

                trace! {crate::log_debug!("auto-calling protocol destructor {} for object {}", destructor.idx, self.id.load(atomic::Ordering::Relaxed))}
                _ = self.call(destructor.idx, &[]);
            }
        }
    }
}

impl ClientObject {
    pub fn new(
        client_socket: sync::Weak<client_socket::ClientSocket>,
        state: sync::Arc<crate::ConnectionState>,
        event_queue: event_queue::WeakEventQueue,
    ) -> Self {
        Self {
            destroyed: atomic::AtomicBool::default(),
            client: client_socket,
            state,
            spec: None,
            id: atomic::AtomicU32::default(),
            version: atomic::AtomicU32::default(),
            seq: 0,
            protocol_name: String::new(),
            object_data: sync::RwLock::default(),
            event_queue,
        }
    }
}

impl object::Object for ClientObject {
    fn set_object_data(&self, data: Box<dyn object::ObjectData>) {
        *self.object_data.write().unwrap() = Some(data);
    }

    fn dispatch(&self, method: u32, data: &[u8], fds: &[i32], state: &mut dyn any::Any) {
        if let Some(object_data) = self.object_data.read().unwrap().as_ref() {
            object_data.dispatch(method, data, fds, state);
        }
    }

    fn call(&self, id: u32, args: &[types::CallArg]) -> u32 {
        match wire_object::WireObject::call(self, id, args) {
            Ok(v) => v,
            Err(e) => {
                crate::log_error!(
                    "object {} (protocol {}) call error: {e}",
                    self.id.load(atomic::Ordering::Relaxed),
                    self.protocol_name
                );
                0
            }
        }
    }

    fn event_queue(&self) -> Option<event_queue::EventQueue> {
        self.event_queue.upgrade()
    }

    fn client_sock(&self) -> Option<client::Client> {
        self.client.upgrade().map(client::Client)
    }

    fn error(&self, error_id: u32, error_msg: &str) {
        _ = error_id;
        _ = error_msg;
    }
}

impl wire_object::WireObject for ClientObject {
    fn set_version(&self, version: u32) {
        self.version.store(version, atomic::Ordering::Relaxed);
    }

    fn version(&self) -> u32 {
        self.version.load(atomic::Ordering::Relaxed)
    }

    fn id(&self) -> u32 {
        self.id.load(atomic::Ordering::Relaxed)
    }

    fn seq(&self) -> u32 {
        self.seq
    }

    fn protocol_name(&self) -> &str {
        &self.protocol_name
    }

    fn server(&self) -> bool {
        false
    }

    fn methods_out(&self) -> &[types::Method] {
        self.spec
            .as_ref()
            .map(|spec| spec.c2s())
            .unwrap_or_default()
    }

    fn errd(&self) {
        self.state.error.store(true, atomic::Ordering::Relaxed);
    }

    fn mark_destroyed(&self) {
        self.destroyed.store(true, atomic::Ordering::Relaxed);
    }

    fn send_message(&self, msg: &dyn message::Message) {
        self.state.send_message(msg);
    }
}
