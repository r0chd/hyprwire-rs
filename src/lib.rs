pub mod client;
pub(crate) mod helpers;
pub mod implementation;
pub(crate) mod message;
pub mod server;
pub(crate) mod socket;

use implementation::object;
use nix::{errno, poll, sys};
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::net;
use std::{cell, ffi, io, sync, time};

pub(crate) struct SharedState {
    pub(crate) error: cell::Cell<bool>,
    pub(crate) stream: cell::RefCell<net::UnixStream>,
    pub(crate) fd: i32,
    pub(crate) impls: Option<sync::Arc<Vec<Box<dyn implementation::server::ProtocolImplementations>>>>,
}

impl SharedState {
    pub(crate) fn new(stream: net::UnixStream) -> Self {
        let fd = stream.as_raw_fd();
        Self {
            error: cell::Cell::new(false),
            stream: cell::RefCell::new(stream),
            fd,
            impls: None,
        }
    }

    pub(crate) fn with_impls(
        stream: net::UnixStream,
        impls: sync::Arc<Vec<Box<dyn implementation::server::ProtocolImplementations>>>,
    ) -> Self {
        let fd = stream.as_raw_fd();
        Self {
            error: cell::Cell::new(false),
            stream: cell::RefCell::new(stream),
            fd,
            impls: Some(impls),
        }
    }

    pub(crate) fn send_message(&self, message: &dyn message::Message) {
        trace! { log::trace!("[{} @ {:.3}] -> {}", self.fd, steady_millis(), message.parse_data()) };

        let stream = self.stream.borrow();
        let buf = message.data();
        let iov = [io::IoSlice::new(buf)];
        let cmsg = [sys::socket::ControlMessage::ScmRights(message.fds())];
        loop {
            match sys::socket::sendmsg::<()>(
                stream.as_raw_fd(),
                &iov,
                &cmsg,
                sys::socket::MsgFlags::empty(),
                None,
            ) {
                Ok(_) => break,
                Err(errno::Errno::EAGAIN) => {
                    let mut pfd = [poll::PollFd::new(
                        stream.as_fd(),
                        poll::PollFlags::POLLOUT | poll::PollFlags::POLLWRBAND,
                    )];
                    if let Err(e) = poll::poll(&mut pfd, poll::PollTimeout::NONE) {
                        log::error!(
                            "[{} @ {:.3}] poll error during send_message: {e}",
                            self.fd,
                            steady_millis(),
                        );
                        break;
                    }
                    continue;
                }
                Err(_) => {
                    break;
                }
            }
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

pub(crate) fn steady_millis() -> f64 {
    let start = START.get_or_init(time::Instant::now);
    start.elapsed().as_nanos() as f64 / 1_000_000.0
}
