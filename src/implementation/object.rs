use crate::implementation::types;
use crate::{client, server};
use std::os::raw;
use std::{cell, rc};

pub trait Object {
    fn call(&mut self, id: u32, args: &[types::CallArg]) -> u32;

    fn listen(&mut self, id: u32, func: *mut raw::c_void);

    fn client_sock(&self) -> Option<client::Client> {
        None
    }

    fn server_sock(&self) -> Option<server::Server> {
        None
    }

    fn create_object(
        &self,
        _object_name: &str,
        _seq: u32,
    ) -> Option<rc::Rc<cell::RefCell<dyn Object>>> {
        None
    }

    fn set_data(&mut self, data: *mut raw::c_void, destructor: Option<unsafe fn(*mut raw::c_void)>);

    fn get_data(&self) -> *mut raw::c_void;

    fn error(&self, error_id: u32, error_msg: &str);

    fn set_on_destroy(&mut self, _func: Box<dyn FnOnce()>) {
        todo!("set_on_destroy")
    }
}
