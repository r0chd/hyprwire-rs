pub mod client;
pub(crate) mod helpers;
pub mod implementation;
pub(crate) mod message;
pub mod scanner;
pub mod server;
pub(crate) mod socket;

use implementation::object;
use std::sync::atomic;
use std::{cell, ffi, sync, time};

pub struct SharedState {
    pub error: atomic::AtomicBool,
}

impl SharedState {
    pub fn new() -> Self {
        Self {
            error: atomic::AtomicBool::new(false),
        }
    }
}

#[macro_export]
macro_rules! include_protocol {
    ($name:expr) => {
        include!(concat!(env!("OUT_DIR"), "/", $name, ".rs"));
    };
}

pub trait Proxy {
    type Event<'a>;
}

pub trait Dispatch<I: Proxy> {
    fn event(&mut self, proxy: &I, event: I::Event<'_>);
}

pub struct DispatchData {
    pub object: *const cell::RefCell<dyn object::Object>,
}

thread_local! {
    static DISPATCH_STATE: cell::Cell<*mut ffi::c_void> = const { cell::Cell::new(std::ptr::null_mut()) };
}

pub fn set_dispatch_state(state: *mut ffi::c_void) {
    DISPATCH_STATE.set(state);
}

pub fn get_dispatch_state() -> *mut ffi::c_void {
    DISPATCH_STATE.get()
}

static START: sync::OnceLock<time::Instant> = sync::OnceLock::new();

pub fn steady_millis() -> f64 {
    let start = START.get_or_init(time::Instant::now);
    start.elapsed().as_nanos() as f64 / 1_000_000.0
}
