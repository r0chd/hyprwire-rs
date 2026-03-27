use crate::client::client_socket;
use crate::implementation::{object, types, wire_object};
use crate::{SharedState, client, message, trace};
use std::cell::{Cell, RefCell};
use std::os::raw;
use std::rc;
use std::sync;

pub struct ClientObject {
    client: rc::Weak<client_socket::ClientSocket>,
    pub(crate) state: rc::Rc<SharedState>,
    pub(crate) spec: Option<sync::Arc<dyn types::ProtocolObjectSpec>>,
    data: Cell<*mut raw::c_void>,
    data_destructor: Cell<Option<unsafe fn(*mut raw::c_void)>>,
    on_drop: RefCell<Option<Box<dyn FnOnce()>>>,
    listeners: RefCell<Vec<*mut raw::c_void>>,
    pub(crate) id: Cell<u32>,
    pub(crate) version: Cell<u32>,
    pub(crate) seq: u32,
    pub(crate) protocol_name: String,
}

impl Drop for ClientObject {
    fn drop(&mut self) {
        trace! {eprintln!("[hw] trace: destroying object {}", self.id.get())}
        if let Some(on_drop) = self.on_drop.borrow_mut().take() {
            on_drop();
        }
        if let Some(destructor) = self.data_destructor.get()
            && !self.data.get().is_null()
        {
            unsafe { destructor(self.data.get()) };
        }
    }
}

impl ClientObject {
    pub fn new(
        client_socket: rc::Weak<client_socket::ClientSocket>,
        state: rc::Rc<SharedState>,
    ) -> Self {
        Self {
            client: client_socket,
            state,
            spec: None,
            data: Cell::new(std::ptr::null_mut()),
            data_destructor: Cell::new(None),
            on_drop: RefCell::new(None),
            listeners: RefCell::new(Vec::new()),
            id: Cell::new(0),
            version: Cell::new(0),
            seq: 0,
            protocol_name: String::new(),
        }
    }
}

impl object::RawObject for ClientObject {
    fn call(&self, id: u32, args: &[types::CallArg]) -> u32 {
        match wire_object::WireObject::call(self, id, args) {
            Ok(v) => v,
            Err(e) => {
                log::error!(
                    "object {} (protocol {}) call error: {e}",
                    self.id.get(),
                    self.protocol_name
                );
                0
            }
        }
    }

    fn listen(&self, id: u32, callback: *mut raw::c_void) {
        let mut listeners = self.listeners.borrow_mut();
        if listeners.len() <= id as usize {
            listeners.reserve_exact(id as usize + 1);
        }
        listeners.push(callback);
    }

    fn client_sock(&self) -> Option<client::Client> {
        self.client.upgrade().map(client::Client)
    }

    fn set_data(&self, data: *mut raw::c_void, destructor: Option<unsafe fn(*mut raw::c_void)>) {
        self.data.set(data);
        self.data_destructor.set(destructor);
    }

    fn get_data(&self) -> *mut raw::c_void {
        self.data.get()
    }

    fn error(&self, error_id: u32, error_msg: &str) {
        _ = error_id;
        _ = error_msg;
    }

    fn set_on_drop(&self, func: Box<dyn FnOnce()>) {
        *self.on_drop.borrow_mut() = Some(func);
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

    fn listener(&self, idx: usize) -> *mut raw::c_void {
        self.listeners.borrow()[idx]
    }

    fn listener_count(&self) -> usize {
        self.listeners.borrow().len()
    }
}
