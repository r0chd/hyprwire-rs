use crate::client;
use crate::implementation::types;
use std::os::raw;
use std::{cell, rc};

pub trait Object {
    fn call(&mut self, id: u32, args: &[types::CallArg]) -> u32;

    fn listen(&mut self, id: u32, func: *mut raw::c_void);

    fn client_sock(&self) -> Option<rc::Rc<cell::RefCell<client::ClientSocket>>> {
        None
    }

    // fn server_sock(&self) -> Option<>,

    fn set_data(&mut self, data: *mut raw::c_void);

    fn get_data(&self) -> *mut raw::c_void;

    fn error(&self, error_id: u32, error_msg: &str);

    // fn get_client(&self) -> ServerC
}
