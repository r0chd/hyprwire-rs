mod client_object;
pub(crate) mod client_socket;
mod server_spec;

use crate::{implementation, message};
use implementation::client::ProtocolImplementations;
use std::os::fd;
use std::{cell, ffi, io, path, ptr, rc};

pub struct Client(pub(crate) rc::Rc<cell::RefCell<client_socket::ClientSocket>>);

impl Client {
    #[must_use]
    pub fn open(path: &path::Path) -> Self {
        Self(client_socket::ClientSocket::open(path))
    }

    #[must_use]
    pub fn from_fd(fd: fd::RawFd) -> Self {
        Self(client_socket::ClientSocket::from_fd(fd))
    }

    pub fn add_implementation<T>(&mut self, p_impl: T)
    where
        T: ProtocolImplementations + 'static,
    {
        self.0.borrow_mut().add_implementation(Box::new(p_impl));
    }

    pub fn wait_for_handshake(&mut self) -> Result<(), io::Error> {
        self.0.borrow_mut().wait_for_handshake()
    }

    pub fn dispatch_events<D>(&self, state: &mut D, block: bool) -> Result<(), io::Error> {
        crate::set_dispatch_state(ptr::from_mut::<D>(state).cast::<ffi::c_void>());
        let result = self.0.borrow_mut().dispatch_events(block);
        crate::set_dispatch_state(std::ptr::null_mut());
        result
    }

    pub fn roundtrip<D>(&self, state: &mut D) -> Result<(), io::Error> {
        crate::set_dispatch_state(ptr::from_mut::<D>(state).cast::<ffi::c_void>());
        let result = self.0.borrow_mut().roundtrip();
        crate::set_dispatch_state(std::ptr::null_mut());
        result
    }

    #[must_use]
    pub fn extract_loop_fd(&self) -> i32 {
        self.0.borrow().extract_loop_fd()
    }

    #[must_use]
    pub fn is_handshake_done(&self) -> bool {
        self.0.borrow().handshake_done.get()
    }

    pub(crate) fn make_object(
        &self,
        protocol_name: &str,
        object_name: &str,
        seq: u32,
    ) -> Result<rc::Rc<cell::RefCell<dyn implementation::object::Object>>, message::MessageError>
    {
        let obj = self
            .0
            .borrow_mut()
            .make_object(protocol_name, object_name, seq)?;
        Ok(obj)
    }

    pub fn make<T: crate::Proxy, D: crate::Dispatch<T>>(
        &self,
        protocol_name: &str,
        seq: u32,
    ) -> Result<T, message::MessageError> {
        let obj = self
            .0
            .borrow_mut()
            .make_object(protocol_name, T::NAME, seq)?;
        let obj = implementation::types::Object::from_raw(obj);
        Ok(T::from_object::<D>(obj))
    }

    pub fn bind<T: crate::Proxy, D: crate::Dispatch<T>>(
        &self,
        spec: &dyn implementation::types::ProtocolSpec,
        version: u32,
    ) -> Result<T, io::Error> {
        let obj = self.0.borrow_mut().bind_protocol(spec, version)?;
        let obj = implementation::types::Object::from_raw(obj);
        Ok(T::from_object::<D>(obj))
    }

    #[must_use]
    pub fn get_spec(&self, name: &str) -> Option<server_spec::ServerSpec> {
        self.0.borrow().get_spec(name)
    }

    pub fn disconnect_on_error(&self) {
        self.0.borrow_mut().disconnect_on_error();
    }

    #[must_use]
    pub fn object_for_seq(
        &self,
        seq: u32,
    ) -> Option<rc::Rc<cell::RefCell<dyn implementation::object::Object>>> {
        self.0
            .borrow()
            .object_for_seq(seq)
            .map(|obj| obj as rc::Rc<cell::RefCell<dyn implementation::object::Object>>)
    }

    #[must_use]
    pub fn object_for_id(
        &self,
        id: u32,
    ) -> Option<rc::Rc<cell::RefCell<dyn implementation::object::Object>>> {
        self.0
            .borrow()
            .object_for_id(id)
            .map(|obj| obj as rc::Rc<cell::RefCell<dyn implementation::object::Object>>)
    }
}
