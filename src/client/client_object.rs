use crate::client::client_socket;
use crate::implementation::{object, types, wire_object};
use crate::{SharedState, client, message, trace};
use std::os::raw;
use std::sync::atomic;
use std::{rc, sync};

pub struct ClientObject {
    client: rc::Weak<client_socket::ClientSocket>,
    pub(crate) state: rc::Rc<SharedState>,
    pub(crate) spec: Option<sync::Arc<dyn types::ProtocolObjectSpec>>,
    data: sync::Mutex<Option<*mut raw::c_void>>,
    data_destructor: sync::Mutex<Option<unsafe fn(*mut raw::c_void)>>,
    on_drop: sync::Mutex<Option<Box<dyn FnOnce() + Send>>>,
    listeners: sync::Mutex<Vec<*mut raw::c_void>>,
    pub(crate) id: atomic::AtomicU32,
    pub(crate) version: atomic::AtomicU32,
    pub(crate) seq: u32,
    pub(crate) protocol_name: String,
}

// SAFETY: ClientObject is only accessed from the dispatch thread.
// The Rc/RefCell fields prevent auto-impl but access is single-threaded.
unsafe impl Send for ClientObject {}
unsafe impl Sync for ClientObject {}

impl Drop for ClientObject {
    fn drop(&mut self) {
        trace! {eprintln!("[hw] trace: destroying object {}", self.id.load(atomic::Ordering::Relaxed))}
        if let Some(on_drop) = self.on_drop.lock().unwrap().take() {
            on_drop();
        }
        if let Some(destructor) = *self.data_destructor.lock().unwrap()
            && let Some(data) = self.data.lock().unwrap().as_ref()
        {
            unsafe { destructor(*data) };
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
            data: sync::Mutex::new(None),
            data_destructor: sync::Mutex::new(None),
            on_drop: sync::Mutex::new(None),
            listeners: sync::Mutex::new(Vec::new()),
            id: atomic::AtomicU32::new(0),
            version: atomic::AtomicU32::new(0),
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
                    self.id.load(atomic::Ordering::Relaxed),
                    self.protocol_name
                );
                0
            }
        }
    }

    fn listen(&self, id: u32, callback: *mut raw::c_void) {
        let mut listeners = self.listeners.lock().unwrap();
        if listeners.len() <= id as usize {
            listeners.reserve_exact(id as usize + 1);
        }
        listeners.push(callback);
    }

    fn client_sock(&self) -> Option<client::Client> {
        self.client.upgrade().map(client::Client)
    }

    fn set_data(&self, data: *mut raw::c_void, destructor: Option<unsafe fn(*mut raw::c_void)>) {
        *self.data.lock().unwrap() = Some(data);
        *self.data_destructor.lock().unwrap() = destructor;
    }

    fn get_data(&self) -> *mut raw::c_void {
        self.data
            .lock()
            .unwrap()
            .as_ref()
            .map_or(std::ptr::null_mut(), |p| *p)
    }

    fn error(&self, error_id: u32, error_msg: &str) {
        _ = error_id;
        _ = error_msg;
    }

    fn set_on_drop(&self, func: Box<dyn FnOnce() + Send>) {
        *self.on_drop.lock().unwrap() = Some(func);
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
        self.listeners.lock().unwrap()[idx]
    }

    fn listener_count(&self) -> usize {
        self.listeners.lock().unwrap().len()
    }
}
