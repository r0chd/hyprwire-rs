mod server_socket;

use std::{cell, io, path, rc};

pub struct Server(rc::Rc<cell::RefCell<server_socket::ServerSocket>>);

impl Server {
    pub fn open(path: Option<&path::Path>) -> io::Result<Self> {
        Ok(Self(server_socket::ServerSocket::open(path)?))
    }
}
