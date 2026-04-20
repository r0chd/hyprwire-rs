use crate::client::client_socket;
use crate::implementation::wire_object::WireObject;
use crate::implementation::{object, types, wire_object};
use crate::{SharedState, client, message, trace};
use std::cell;
use std::os::raw;
use std::rc;
use std::sync;

pub struct ClientObject {
    client: rc::Weak<client_socket::ClientSocket>,
    pub(crate) state: rc::Rc<SharedState>,
    pub(crate) spec: Option<sync::Arc<dyn types::ProtocolObjectSpec>>,
    data: cell::Cell<*mut raw::c_void>,
    data_destructor: cell::Cell<Option<unsafe fn(*mut raw::c_void)>>,
    listeners: cell::RefCell<Vec<*mut raw::c_void>>,
    pub(crate) id: cell::Cell<u32>,
    pub(crate) version: cell::Cell<u32>,
    pub(crate) seq: u32,
    pub(crate) protocol_name: String,
    destroyed: cell::Cell<bool>,
}

impl Drop for ClientObject {
    fn drop(&mut self) {
        if !self.destroyed.get()
            && self.id.get() != 0
            && self.spec.is_some()
            && self.client.upgrade().is_some()
        {
            if let Some(destructor) = self.data_destructor.get()
                && !self.data.get().is_null()
            {
                unsafe { destructor(self.data.get()) }
            }

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
        state: rc::Rc<SharedState>,
    ) -> Self {
        Self {
            destroyed: cell::Cell::new(false),
            client: client_socket,
            state,
            spec: None,
            data: cell::Cell::new(std::ptr::null_mut()),
            data_destructor: cell::Cell::new(None),
            listeners: cell::RefCell::new(Vec::new()),
            id: cell::Cell::new(0),
            version: cell::Cell::new(0),
            seq: 0,
            protocol_name: String::new(),
        }
    }
}

impl object::Object for ClientObject {
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

    fn listen(&self, id: u32, callback: *mut raw::c_void) {
        let mut listeners = self.listeners.borrow_mut();
        if listeners.len() <= id as usize {
            listeners.resize(id as usize + 1, std::ptr::null_mut());
        }
        listeners[id as usize] = callback;
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

    fn mark_destroyed(&self) {
        self.destroyed.set(true);
    }

    fn on_destructor(&self) {
        let id = self.id.get();
        self.destroyed.set(true);
        if id != 0
            && let Some(client) = self.client.upgrade()
        {
            client.destroy_object(id);
        }
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

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
