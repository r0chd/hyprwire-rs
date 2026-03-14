use std::os::unix::net;
use std::{cell, fs, io, path, rc};

use crate::implementation;

pub struct ServerSocket {
    server: Option<net::UnixListener>,
    export_fd: Option<net::UnixStream>,
    export_write_fd: Option<net::UnixStream>,
    wakeup_fd: net::UnixStream,
    wakeup_write_fd: net::UnixStream,
    exit_fd: net::UnixStream,
    exit_write_fd: net::UnixStream,
    is_empty_listener: bool,
    impls: Vec<Box<dyn implementation::server::ProtocolImplementations>>,
    _self: rc::Weak<cell::RefCell<Self>>,
}

impl ServerSocket {
    pub fn open(path: Option<&path::Path>) -> io::Result<rc::Rc<cell::RefCell<Self>>> {
        let wake_pipes = net::UnixStream::pair()?;
        let exit_pipes = net::UnixStream::pair()?;

        match path {
            Some(path) => {
                if fs::exists(path)? {
                    match net::UnixStream::connect(path) {
                        Ok(_) => {
                            return Err(io::Error::new(
                                io::ErrorKind::AddrInUse,
                                "socket is alive",
                            ));
                        }
                        Err(e) if e.kind() != io::ErrorKind::ConnectionRefused => return Err(e),
                        _ => fs::remove_file(path)?,
                    }
                }

                let socket = net::UnixListener::bind(path)?;
                Ok(rc::Rc::new_cyclic(|weak_self| {
                    cell::RefCell::new(Self {
                        server: Some(socket),
                        export_fd: None,
                        export_write_fd: None,
                        wakeup_fd: wake_pipes.0,
                        wakeup_write_fd: wake_pipes.1,
                        exit_fd: exit_pipes.0,
                        exit_write_fd: exit_pipes.1,
                        is_empty_listener: false,
                        impls: Vec::new(),
                        _self: weak_self.clone(),
                    })
                }))
            }
            None => Ok(rc::Rc::new_cyclic(|weak_self| {
                cell::RefCell::new(Self {
                    server: None,
                    export_fd: None,
                    export_write_fd: None,
                    wakeup_fd: wake_pipes.0,
                    wakeup_write_fd: wake_pipes.1,
                    exit_fd: exit_pipes.0,
                    exit_write_fd: exit_pipes.1,
                    is_empty_listener: true,
                    impls: Vec::new(),
                    _self: weak_self.clone(),
                })
            })),
        }
    }

    pub fn add_implementation(
        &mut self,
        implementation: Box<dyn implementation::server::ProtocolImplementations>,
    ) {
        self.impls.push(implementation);
    }
}
