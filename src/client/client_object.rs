use crate::client::client_socket;
use crate::implementation::{object, types, wire_object};
use crate::{SharedState, client, message, trace};
use std::os::raw;
use std::{cell, rc, sync};

pub struct ClientObject {
    client: rc::Weak<cell::RefCell<client_socket::ClientSocket>>,
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

impl Drop for ClientObject {
    fn drop(&mut self) {
        trace! {log::debug!("destroying object {}", self.id)}
        if let Some(destructor) = self.data_destructor
            && let Some(data) = self.data
        {
            unsafe { destructor(data) };
        }
    }
}

impl ClientObject {
    pub fn new(
        client_socket: rc::Weak<cell::RefCell<client_socket::ClientSocket>>,
        state: rc::Rc<SharedState>,
    ) -> Self {
        Self {
            client: client_socket,
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

impl object::Object for ClientObject {
    fn call(&mut self, id: u32, args: &[types::CallArg]) -> u32 {
        match wire_object::WireObject::call(self, id, args) {
            Ok(v) => v,
            Err(e) => {
                log::error!(
                    "object {} (protocol {}) call error: {e}",
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

    fn client_sock(&self) -> Option<client::Client> {
        self.client.upgrade().map(client::Client)
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
        _ = error_id;
        _ = error_msg;
    }
}

impl wire_object::WireObject for ClientObject {
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
        false
    }

    fn methods_out(&self) -> &[types::Method] {
        self.spec
            .as_ref()
            .map(|spec| spec.c2s())
            .unwrap_or_default()
    }

    fn methods_in(&self) -> &[types::Method] {
        self.spec
            .as_ref()
            .map(|spec| spec.s2c())
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
