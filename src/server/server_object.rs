use super::server_client;
use crate::implementation::wire_object::WireObject;
use crate::implementation::{object, types, wire_object};
use crate::{SharedState, message, trace};
use std::os::raw;
use std::{cell, rc, sync};

pub(crate) struct ServerObject {
    pub(crate) client: rc::Weak<cell::RefCell<server_client::ServerClient>>,
    pub(crate) state: rc::Rc<SharedState>,
    pub(crate) spec: Option<sync::Arc<dyn types::ProtocolObjectSpec>>,
    data: Option<*mut raw::c_void>,
    data_destructor: Option<unsafe fn(*mut raw::c_void)>,
    listeners: Vec<*mut raw::c_void>,
    pub(crate) id: u32,
    pub(crate) version: u32,
    pub(crate) seq: u32,
    pub(crate) protocol_name: String,
}

impl Drop for ServerObject {
    fn drop(&mut self) {
        trace! {eprintln!("[hw] trace: destroying server object {}", self.id)}
        if let Some(destructor) = self.data_destructor
            && let Some(data) = self.data
        {
            unsafe { destructor(data) };
        }
    }
}

impl ServerObject {
    pub fn new(
        client: rc::Weak<cell::RefCell<server_client::ServerClient>>,
        state: rc::Rc<SharedState>,
    ) -> Self {
        Self {
            client,
            state,
            spec: None,
            data: None,
            data_destructor: None,
            listeners: Vec::new(),
            id: 0,
            version: 0,
            seq: 0,
            protocol_name: String::new(),
        }
    }
}

impl object::Object for ServerObject {
    fn call(&mut self, id: u32, args: &[types::CallArg]) -> u32 {
        match wire_object::WireObject::call(self, id, args) {
            Ok(v) => v,
            Err(e) => {
                log::error!(
                    "server object {} (protocol {}) call error: {e}",
                    self.id,
                    self.protocol_name
                );
                0
            }
        }
    }

    fn listen(&mut self, id: u32, callback: *mut raw::c_void) {
        if self.listeners.len() <= id as usize {
            self.listeners.reserve_exact(id as usize + 1);
        }

        self.listeners.push(callback);
    }

    fn create_object(
        &self,
        object_name: &str,
        seq: u32,
    ) -> Option<rc::Rc<cell::RefCell<dyn object::Object>>> {
        let client = self.client.upgrade()?;
        let obj =
            client
                .borrow()
                .create_object(&self.protocol_name, object_name, self.version, seq);
        Some(obj as rc::Rc<cell::RefCell<dyn object::Object>>)
    }

    fn set_data(
        &mut self,
        data: *mut raw::c_void,
        destructor: Option<unsafe fn(*mut raw::c_void)>,
    ) {
        self.data = Some(data);
        self.data_destructor = destructor;
    }

    fn get_data(&self) -> *mut raw::c_void {
        self.data.unwrap_or(std::ptr::null_mut())
    }

    fn error(&self, error_id: u32, error_msg: &str) {
        let msg = message::FatalProtocolError::new(self.id, error_id, error_msg);
        self.state.send_message(&msg);
        self.errd();
    }
}

impl wire_object::WireObject for ServerObject {
    fn set_version(&mut self, version: u32) {
        self.version = version;
    }

    fn version(&self) -> u32 {
        self.version
    }

    fn id(&self) -> u32 {
        self.id
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

    fn methods_in(&self) -> &[types::Method] {
        self.spec
            .as_ref()
            .map(|spec| spec.c2s())
            .unwrap_or_default()
    }

    fn errd(&self) {
        self.state.error.set(true);
    }

    fn send_message(&self, msg: &dyn message::Message) {
        self.state.send_message(msg);
    }

    fn listeners(&self) -> &[*mut std::os::raw::c_void] {
        &self.listeners
    }
}
