use crate::server::server_client;
use crate::{client, server};
use hyprwire_core::types;
use std::{any, rc};

pub trait ObjectData {
    fn dispatch(&self, method: u32, data: &[u8], fds: &[i32], state: &mut dyn any::Any);

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

    fn dispatch(&self, method: u32, data: &[u8], fds: &[i32], state: &mut dyn any::Any);
}
