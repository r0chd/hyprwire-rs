use crate::implementation::types;
use crate::server::server_client;
use crate::{client, server};
use std::os::raw;
use std::rc;

pub trait RawObject {
    fn call(&self, id: u32, args: &[types::CallArg]) -> u32;

    fn listen(&self, id: u32, func: *mut raw::c_void);

    fn client_sock(&self) -> Option<client::Client> {
        None
    }

    fn server_sock(&self) -> Option<server::Server> {
        None
    }

    fn server_client(&self) -> Option<server_client::ServerClient> {
        None
    }

    fn create_object(&self, _object_name: &str, _seq: u32) -> Option<rc::Rc<dyn RawObject>> {
        None
    }

    fn set_data(&self, data: *mut raw::c_void, destructor: Option<unsafe fn(*mut raw::c_void)>);

    fn get_data(&self) -> *mut raw::c_void;

    fn error(&self, error_id: u32, error_msg: &str);
}
