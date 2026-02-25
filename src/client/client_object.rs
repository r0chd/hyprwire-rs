use crate::client::client_socket;
use crate::implementation::{object, types, wire_object};
use crate::{client, message};
use std::os::raw;
use std::{cell, rc};

pub struct ClientObject {
    client: Option<rc::Weak<cell::RefCell<client::ClientSocket>>>,
    // SAFETY: spec points into ClientSocket.impls which outlives ClientSocket.objects.
    // Only accessed while the parent ClientSocket is alive.
    pub(crate) spec: Option<*const dyn types::ProtocolObjectSpec>,
    data: Option<*mut raw::c_void>,
    listeners: Vec<*mut raw::c_void>,
    pub(crate) id: u32,
    version: u32,
    pub(crate) seq: u32,
    pub(crate) protocol_name: String,
}

impl ClientObject {
    pub fn new(client_socket: rc::Weak<cell::RefCell<client_socket::ClientSocket>>) -> Self {
        Self {
            client: Some(client_socket),
            spec: None,
            data: None,
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
        wire_object::WireObject::call(self, id, args)
    }

    fn listen(&mut self, id: u32, callback: *mut raw::c_void) {
        if self.listeners.len() <= id as usize {
            self.listeners.reserve_exact(id as usize + 1);
        }

        self.listeners.push(callback);
    }

    fn client_sock(&self) -> Option<rc::Rc<cell::RefCell<client::ClientSocket>>> {
        self.client.as_ref().and_then(|weak| weak.upgrade())
    }

    fn set_data(&mut self, data: *mut raw::c_void) {
        self.data = Some(data);
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

    fn server(&self) -> bool {
        false
    }

    fn methods_out(&self) -> &[types::Method] {
        self.spec
            .map(|spec| unsafe { &*spec }.c2s())
            .unwrap_or_default()
    }

    fn methods_in(&self) -> &[types::Method] {
        self.spec
            .map(|spec| unsafe { &*spec }.s2c())
            .unwrap_or_default()
    }

    fn errd(&mut self) {
        if let Some(client) = self.client.as_ref().and_then(|weak| weak.upgrade()) {
            client.borrow_mut().error = true;
        }
    }

    fn send_message(&mut self, msg: &dyn message::Message) {
        if let Some(client) = self.client.as_ref().and_then(|weak| weak.upgrade()) {
            client.borrow_mut().send_message(msg);
        }
    }

    fn listeners(&self) -> &[*mut std::os::raw::c_void] {
        &self.listeners
    }
}
