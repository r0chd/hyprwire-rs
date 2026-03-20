use super::client_socket::ClientSocket;
use super::server_spec;
use crate::{implementation, message};
use implementation::client::ProtocolImplementations;
use std::os::fd;
use std::{cell, io, path, rc};

pub struct Client(rc::Rc<cell::RefCell<ClientSocket>>);

impl Client {
    pub fn open(path: &path::Path) -> Self {
        Self(ClientSocket::open(path))
    }

    pub fn from_fd(fd: fd::RawFd) -> Self {
        Self(ClientSocket::from_fd(fd))
    }

    pub fn add_implementation(&mut self, p_impl: impl ProtocolImplementations + 'static) {
        self.0.borrow_mut().add_implementation(Box::new(p_impl));
    }

    pub fn wait_for_handshake(&mut self) -> Result<(), io::Error> {
        self.0.borrow_mut().wait_for_handshake()
    }

    pub fn dispatch_events(&mut self, block: bool) -> Result<(), io::Error> {
        self.0.borrow_mut().dispatch_events(block)
    }

    pub fn roundtrip(&mut self) -> Result<(), io::Error> {
        self.0.borrow_mut().roundtrip()
    }

    pub fn extract_loop_fd(&self) -> i32 {
        self.0.borrow().extract_loop_fd()
    }

    pub(crate) fn make_object(
        &mut self,
        protocol_name: &str,
        object_name: &str,
        seq: u32,
    ) -> Result<rc::Rc<dyn implementation::object::RawObject>, message::MessageError>
    {
        let obj = self
            .0
            .borrow_mut()
            .make_object(protocol_name, object_name, seq)?;
        Ok(obj)
    }

    pub(crate) fn bind_protocol(
        &mut self,
        spec: &dyn implementation::types::ProtocolSpec,
        version: u32,
    ) -> Result<rc::Rc<dyn implementation::object::RawObject>, io::Error> {
        self.0.borrow_mut().bind_protocol(spec, version)
    }

    pub fn get_spec(&self, name: &str) -> Option<server_spec::ServerSpec> {
        self.0.borrow().get_spec(name)
    }

    pub fn disconnect_on_error(&mut self) {
        self.0.borrow_mut().disconnect_on_error()
    }
}
