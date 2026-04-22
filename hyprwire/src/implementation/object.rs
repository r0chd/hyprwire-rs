use crate::server::server_client;
use crate::{client, server};
use hyprwire_core::types;
use std::rc;

pub trait ObjectData {
    /// Dispatch an incoming method call.
    ///
    /// `state` is a type-erased pointer to the caller's dispatch state (`&mut D`
    /// passed to `dispatch_events`). The concrete `ObjectData` implementation is
    /// monomorphized for the correct `D` and casts it back.
    fn dispatch(&self, method: u32, data: &[u8], fds: &[i32], state: *mut ());

    fn destroyed(&self) {}
}

pub trait Object {
    fn call(&self, id: u32, args: &[types::CallArg]) -> u32;

    fn client_sock(&self) -> Option<client::Client> {
        None
    }

    fn server_sock(&self) -> Option<server::Server> {
        None
    }

    fn server_client(&self) -> Option<server_client::ServerClient> {
        None
    }

    fn create_object(&self, _object_name: &str, _seq: u32) -> Option<rc::Rc<dyn Object>> {
        None
    }

    fn error(&self, error_id: u32, error_msg: &str);

    fn set_object_data(&self, data: Box<dyn ObjectData>);

    fn dispatch(&self, method: u32, data: &[u8], fds: &[i32], state: *mut ());
}
