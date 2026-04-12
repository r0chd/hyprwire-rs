use super::server_client;
use crate::implementation::wire_object::WireObject;
use crate::implementation::{object, types, wire_object};
use crate::{SharedState, message, trace};
use std::cell::{Cell, RefCell};
use std::os::raw;
use std::{rc, sync};

pub(crate) struct ServerObject {
    pub(crate) client: rc::Weak<server_client::ServerClientState>,
    pub(crate) state: rc::Rc<SharedState>,
    pub(crate) spec: Option<sync::Arc<dyn types::ProtocolObjectSpec>>,
    data: Cell<*mut raw::c_void>,
    data_destructor: Cell<Option<unsafe fn(*mut raw::c_void)>>,
    on_drop: RefCell<Option<Box<dyn FnOnce()>>>,
    listeners: RefCell<Vec<*mut raw::c_void>>,
    destroyed: Cell<bool>,
    pub(crate) id: Cell<u32>,
    pub(crate) version: Cell<u32>,
    pub(crate) seq: u32,
    pub(crate) protocol_name: String,
}

impl Drop for ServerObject {
    fn drop(&mut self) {
        trace! {eprintln!("[hw] trace: destroying server object {}", self.id.get())}
        self.destroy();
    }
}

impl ServerObject {
    pub fn new(
        client: rc::Weak<server_client::ServerClientState>,
        state: rc::Rc<SharedState>,
    ) -> Self {
        Self {
            client,
            state,
            spec: None,
            data: Cell::new(std::ptr::null_mut()),
            data_destructor: Cell::new(None),
            on_drop: RefCell::new(None),
            listeners: RefCell::new(Vec::new()),
            destroyed: Cell::new(false),
            id: Cell::new(0),
            version: Cell::new(0),
            seq: 0,
            protocol_name: String::new(),
        }
    }

    pub(crate) fn destroy_for_disconnect<D>(&self, dispatch: &mut D) {
        if self.destroyed.get() {
            return;
        }

        self.dispatch_no_arg_destructor(dispatch);
        self.destroy();
    }

    fn dispatch_no_arg_destructor<D>(&self, dispatch: &mut D) {
        let Some(method) = self.spec.as_ref().and_then(|spec| {
            spec.c2s().iter().find(|method| {
                method.destructor && method.params.is_empty() && method.returns_type.is_empty()
            })
        }) else {
            return;
        };

        if let Err(e) = wire_object::WireObject::called(self, method.idx, &[], &[], dispatch) {
            log::error!(
                "server object {} (protocol {}) destructor dispatch error: {e}",
                self.id.get(),
                self.protocol_name
            );
        }
    }

    fn destroy(&self) {
        if self.destroyed.replace(true) {
            return;
        }

        if let Some(on_drop) = self.on_drop.borrow_mut().take() {
            on_drop();
        }

        if let Some(destructor) = self.data_destructor.replace(None)
            && !self.data.get().is_null()
        {
            unsafe { destructor(self.data.get()) };
            self.data.set(std::ptr::null_mut());
        }

        self.listeners.borrow_mut().clear();
    }
}

impl object::RawObject for ServerObject {
    fn call(&self, id: u32, args: &[types::CallArg]) -> u32 {
        if self.destroyed.get() {
            return 0;
        }

        match wire_object::WireObject::call(self, id, args) {
            Ok(v) => v,
            Err(e) => {
                log::error!(
                    "server object {} (protocol {}) call error: {e}",
                    self.id.get(),
                    self.protocol_name
                );
                0
            }
        }
    }

    fn listen(&self, id: u32, callback: *mut raw::c_void) {
        if self.destroyed.get() {
            return;
        }

        let mut listeners = self.listeners.borrow_mut();
        if listeners.len() <= id as usize {
            listeners.resize(id as usize + 1, std::ptr::null_mut());
        }
        listeners[id as usize] = callback;
    }

    fn create_object(&self, object_name: &str, seq: u32) -> Option<rc::Rc<dyn object::RawObject>> {
        if self.destroyed.get() {
            return None;
        }

        let client = self.client.upgrade()?;
        let obj = client.create_object(&self.protocol_name, object_name, self.version.get(), seq);
        Some(obj as rc::Rc<dyn object::RawObject>)
    }

    fn server_client(&self) -> Option<server_client::ServerClient> {
        self.client.upgrade().map(|client| client.handle())
    }

    fn set_data(&self, data: *mut raw::c_void, destructor: Option<unsafe fn(*mut raw::c_void)>) {
        self.data.set(data);
        self.data_destructor.set(destructor);
    }

    fn get_data(&self) -> *mut raw::c_void {
        self.data.get()
    }

    fn error(&self, error_id: u32, error_msg: &str) {
        if self.destroyed.get() {
            return;
        }

        let msg = message::FatalProtocolError::new(self.id.get(), error_id, error_msg);
        self.state.send_message(&msg);
        self.errd();
    }

    fn set_on_drop(&self, func: Box<dyn FnOnce()>) {
        if self.destroyed.get() {
            func();
            return;
        }

        *self.on_drop.borrow_mut() = Some(func);
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

    fn listener(&self, idx: usize) -> *mut raw::c_void {
        self.listeners.borrow()[idx]
    }

    fn listener_count(&self) -> usize {
        self.listeners.borrow().len()
    }
}
