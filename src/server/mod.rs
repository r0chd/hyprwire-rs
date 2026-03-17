pub(crate) mod server_client;
mod server_object;
mod server_socket;

use crate::implementation::server;
use std::os::fd::RawFd;
use std::{io, path};

pub struct Server(server_socket::ServerSocket);

impl Server {
    pub fn open(path: Option<&path::Path>) -> io::Result<Self> {
        Ok(Self(server_socket::ServerSocket::open(path)?))
    }

    pub fn add_implementation<T>(&mut self, p_impl: T)
    where
        T: server::ProtocolImplementations + 'static,
    {
        self.0.add_implementation(Box::new(p_impl));
    }

    pub fn dispatch_events<D>(&mut self, state: &mut D, block: bool) -> bool {
        crate::set_dispatch_state(state as *mut D as *mut std::ffi::c_void);
        let result = self.0.dispatch_events(block);
        crate::set_dispatch_state(std::ptr::null_mut());
        result
    }

    pub fn extract_loop_fd(&mut self) -> io::Result<RawFd> {
        self.0.extract_loop_fd()
    }

    pub fn add_client(&self, _fd: RawFd) {
        todo!("add_client")
    }

    pub fn remove_client(&self, _fd: RawFd) -> bool {
        todo!("remove_client")
    }

    pub fn create_object(
        &self,
        _client_fd: RawFd,
        _reference: &dyn crate::implementation::object::Object,
        _object: &str,
        _seq: u32,
    ) -> Option<std::rc::Rc<std::cell::RefCell<dyn crate::implementation::object::Object>>> {
        todo!("create_object")
    }
}
