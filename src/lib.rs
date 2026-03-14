pub mod client;
pub(crate) mod helpers;
pub mod implementation;
pub(crate) mod message;
pub mod scanner;
pub mod server;
pub(crate) mod socket;

use implementation::object;
use std::{cell, sync, time};

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

pub struct DispatchData<D> {
    pub state: *mut D,
    pub object: *const cell::RefCell<dyn object::Object>,
}

static START: sync::OnceLock<time::Instant> = sync::OnceLock::new();

pub fn steady_millis() -> f64 {
    let start = START.get_or_init(time::Instant::now);
    start.elapsed().as_nanos() as f64 / 1_000_000.0
}
