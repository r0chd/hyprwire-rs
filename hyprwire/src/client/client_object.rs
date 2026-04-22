use crate::client::client_socket;
use crate::implementation::wire_object::WireObject;
use crate::implementation::{object, wire_object};
use crate::{client, trace};
use hyprwire_core::{message, types};
use std::{any, cell, rc, sync};

pub struct ClientObject {
    client: rc::Weak<client_socket::ClientSocket>,
    pub(crate) state: rc::Rc<crate::SharedState>,
    pub(crate) spec: Option<sync::Arc<dyn types::ProtocolObjectSpec>>,
    pub(crate) id: cell::Cell<u32>,
    pub(crate) version: cell::Cell<u32>,
    pub(crate) seq: u32,
    pub(crate) protocol_name: String,
    object_data: cell::RefCell<Option<Box<dyn object::ObjectData>>>,
    pub(crate) destroyed: cell::Cell<bool>,
}

impl Drop for ClientObject {
    fn drop(&mut self) {
        if !self.destroyed.get()
            && self.id.get() != 0
            && self.spec.is_some()
            && self.client.upgrade().is_some()
        {
            let methods = self.methods_out();
            if let Some(destructor) = methods
                .iter()
                .find(|method| method.destructor && method.since <= self.version.get())
            {
                if !destructor.returns_type.is_empty() {
                    crate::log_debug!(
                        "can't auto-call destructor for object {}: method {} has returns type",
                        self.id.get(),
                        destructor.idx
                    );
                    return;
                }

                if !destructor.params.is_empty() {
                    crate::log_debug!(
                        "can't auto-call destructor for object {}: method {} has params",
                        self.id.get(),
                        destructor.idx
                    );
                    return;
                }

                trace! {crate::log_debug!("auto-calling protocol destructor {} for object {}", destructor.idx, self.id.get())}
                _ = self.call(destructor.idx, &[]);
            }
        }
    }
}

impl ClientObject {
    pub fn new(
        client_socket: rc::Weak<client_socket::ClientSocket>,
        state: rc::Rc<crate::SharedState>,
    ) -> Self {
        Self {
            destroyed: cell::Cell::new(false),
            client: client_socket,
            state,
            spec: None,
            id: cell::Cell::new(0),
            version: cell::Cell::new(0),
            seq: 0,
            protocol_name: String::new(),
            object_data: cell::RefCell::new(None),
        }
    }
}

impl object::Object for ClientObject {
    fn set_object_data(&self, data: Box<dyn object::ObjectData>) {
        *self.object_data.borrow_mut() = Some(data);
    }

    fn dispatch(&self, method: u32, data: &[u8], fds: &[i32], state: &mut dyn any::Any) {
        if let Some(object_data) = self.object_data.borrow().as_ref() {
            object_data.dispatch(method, data, fds, state);
        }
    }

    fn call(&self, id: u32, args: &[types::CallArg]) -> u32 {
        match wire_object::WireObject::call(self, id, args) {
            Ok(v) => v,
            Err(e) => {
                crate::log_error!(
                    "object {} (protocol {}) call error: {e}",
                    self.id.get(),
                    self.protocol_name
                );
                0
            }
        }
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
        self.version.set(version);
    }

    fn version(&self) -> u32 {
        self.version.get()
    }

    fn id(&self) -> u32 {
        self.id.get()
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
        self.state.error.set(true);
    }

    fn mark_destroyed(&self) {
        self.destroyed.set(true);
    }

    fn send_message(&self, msg: &dyn message::Message) {
        self.state.send_message(msg);
    }
}
