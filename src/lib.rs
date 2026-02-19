use std::{sync, time};

pub mod client;
pub(crate) mod helpers;
pub mod implementation;
pub(crate) mod message;
pub(crate) mod socket;

static START: sync::OnceLock<time::Instant> = sync::OnceLock::new();

pub fn steady_millis() -> f64 {
    let start = START.get_or_init(time::Instant::now);
    start.elapsed().as_nanos() as f64 / 1_000_000.0
}
