use super::server_client;
use crate::implementation::object::Object;
use crate::implementation::wire_object::WireObject;
use crate::implementation::{object, wire_object};
use crate::trace;
use hyprwire_core::message;
use hyprwire_core::message::wire::fatal_protocol_error;
use hyprwire_core::types;
use std::{any, cell, rc, sync};

pub(crate) struct ServerObject {
    pub(crate) client: rc::Weak<server_client::ServerClientState>,
    pub(crate) state: rc::Rc<crate::ConnectionState>,
    pub(crate) spec: Option<sync::Arc<dyn types::ProtocolObjectSpec>>,
    pub(crate) destroyed: cell::Cell<bool>,
    object_data: cell::RefCell<Option<Box<dyn object::ObjectData>>>,
    pub(crate) id: cell::Cell<u32>,
    pub(crate) version: cell::Cell<u32>,
    pub(crate) seq: u32,
    pub(crate) protocol_name: String,
}

impl Drop for ServerObject {
    fn drop(&mut self) {
        trace! {crate::log_debug!("[hw] trace: destroying server object {}", self.id.get())}
        self.destroy();
    }
}

impl ServerObject {
    pub fn new(
        client: rc::Weak<server_client::ServerClientState>,
        state: rc::Rc<crate::ConnectionState>,
    ) -> Self {
        Self {
            object_data: cell::RefCell::new(None),
            client,
            state,
            spec: None,
            destroyed: cell::Cell::new(false),
            id: cell::Cell::new(0),
            version: cell::Cell::new(0),
            seq: 0,
            protocol_name: String::new(),
        }
    }

    pub(crate) fn destroy_for_disconnect(&self, dispatch: &mut dyn any::Any) {
        if self.destroyed.get() {
            return;
        }

        let Some(method) = self.spec.as_ref().and_then(|spec| {
            spec.c2s().iter().find(|method| {
                method.destructor && method.params.is_empty() && method.returns_type.is_empty()
            })
        }) else {
            return;
        };

        self.dispatch(method.idx, &[], &[], dispatch);

        self.destroy();
    }

    fn destroy(&self) {
        self.destroyed.set(true);
    }
}

impl object::Object for ServerObject {
    fn dispatch(&self, method: u32, data: &[u8], fds: &[i32], state: &mut dyn any::Any) {
        if let Some(object_data) = self.object_data.borrow().as_ref() {
            object_data.dispatch(method, data, fds, state);
        }
    }

    fn set_object_data(&self, data: Box<dyn object::ObjectData>) {
        *self.object_data.borrow_mut() = Some(data);
    }

    fn call(&self, id: u32, args: &[types::CallArg]) -> u32 {
        if self.destroyed.get() {
            return 0;
        }

        match wire_object::WireObject::call(self, id, args) {
            Ok(v) => v,
            Err(e) => {
                crate::log_error!(
                    "server object {} (protocol {}) call error: {e}",
                    self.id.get(),
                    self.protocol_name
                );
                0
            }
        }
    }

    fn create_object(&self, object_name: &str, seq: u32) -> Option<rc::Rc<dyn object::Object>> {
        if self.destroyed.get() {
            return None;
        }

        let client = self.client.upgrade()?;
        let obj = client.create_object(&self.protocol_name, object_name, self.version.get(), seq);
        Some(obj as rc::Rc<dyn object::Object>)
    }

    fn server_client(&self) -> Option<server_client::ServerClient> {
        self.client.upgrade().map(|client| client.handle())
    }

    fn error(&self, error_id: u32, error_msg: &str) {
        if self.destroyed.get() {
            return;
        }

        let msg = fatal_protocol_error::FatalProtocolError::new(self.id.get(), error_id, error_msg);
        self.state.send_message(&msg);
        self.errd();
    }
}

impl wire_object::WireObject for ServerObject {
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
        true
    }

    fn methods_out(&self) -> &[types::Method] {
        self.spec
            .as_ref()
            .map(|spec| spec.s2c())
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
