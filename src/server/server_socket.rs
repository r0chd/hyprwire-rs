use super::server_client;
use crate::{message, socket, steady_millis, trace, SharedState};
use nix::poll;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::os::unix::net;
use std::sync;
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
    pub(crate) impls: Vec<Box<dyn implementation::server::ProtocolImplementations>>,
    clients: Vec<rc::Rc<cell::RefCell<server_client::ServerClient>>>,
    pollfds: Vec<poll::PollFd<'static>>,
    _self: sync::Weak<sync::RwLock<Self>>,
}

impl ServerSocket {
    pub fn open(path: Option<&path::Path>) -> io::Result<sync::Arc<sync::RwLock<Self>>> {
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
                socket.set_nonblocking(true)?;
                let arc = sync::Arc::new_cyclic(|weak_self| {
                    sync::RwLock::new(Self {
                        server: Some(socket),
                        export_fd: None,
                        export_write_fd: None,
                        wakeup_fd: wake_pipes.0,
                        wakeup_write_fd: wake_pipes.1,
                        exit_fd: exit_pipes.0,
                        exit_write_fd: exit_pipes.1,
                        is_empty_listener: false,
                        impls: Vec::new(),
                        clients: Vec::new(),
                        pollfds: Vec::new(),
                        _self: weak_self.clone(),
                    })
                });
                arc.write().unwrap().recheck_pollfds();
                Ok(arc)
            }
            None => {
                let arc = sync::Arc::new_cyclic(|weak_self| {
                    sync::RwLock::new(Self {
                        server: None,
                        export_fd: None,
                        export_write_fd: None,
                        wakeup_fd: wake_pipes.0,
                        wakeup_write_fd: wake_pipes.1,
                        exit_fd: exit_pipes.0,
                        exit_write_fd: exit_pipes.1,
                        is_empty_listener: true,
                        impls: Vec::new(),
                        clients: Vec::new(),
                        pollfds: Vec::new(),
                        _self: weak_self.clone(),
                    })
                });
                arc.write().unwrap().recheck_pollfds();
                Ok(arc)
            }
        }
    }

    pub fn add_implementation(
        &mut self,
        implementation: Box<dyn implementation::server::ProtocolImplementations>,
    ) {
        self.impls.push(implementation);
    }

    pub fn dispatch_pending(&mut self) -> bool {
        let _ = poll::poll(&mut self.pollfds, poll::PollTimeout::ZERO);

        if self.dispatch_new_connections() {
            return self.dispatch_pending();
        }

        self.dispatch_existing_connections()
    }

    fn clear_fd(fd: &net::UnixStream) {
        let mut buf = [0u8; 128];
        let mut pfd = [poll::PollFd::new(fd.as_fd(), poll::PollFlags::POLLIN)];

        loop {
            let _ = poll::poll(&mut pfd, poll::PollTimeout::ZERO);

            if let Some(revents) = pfd[0].revents()
                && revents.contains(poll::PollFlags::POLLIN)
            {
                let _ = io::Read::read(&mut &*fd, &mut buf);
                continue;
            }

            break;
        }
    }

    fn clear_exit_fd(&self) {
        Self::clear_fd(&self.exit_fd);
    }

    fn clear_wakeup_fd(&self) {
        Self::clear_fd(&self.wakeup_fd);
    }

    fn dispatch_client(&self, client: &rc::Rc<cell::RefCell<server_client::ServerClient>>) {
        let state = sync::Arc::clone(&client.borrow().state);

        let mut data = {
            let stream = state.stream.lock().unwrap();
            match socket::SocketRawParsedMessage::read_from_socket(&stream) {
                Ok(d) => d,
                Err(_) => {
                    state.send_message(&message::FatalProtocolError::new(
                        0,
                        u32::MAX,
                        "fatal: invalid message on wire",
                    ));
                    state.error.store(true, sync::atomic::Ordering::Relaxed);
                    return;
                }
            }
        };

        if data.data.is_empty() {
            return;
        }

        if message::handle_message(&mut data, message::Role::Server(&client.borrow()))
            .is_err()
        {
            state.send_message(&message::FatalProtocolError::new(
                0,
                u32::MAX,
                "fatal: failed to handle message on wire",
            ));
            state.error.store(true, sync::atomic::Ordering::Relaxed);
            return;
        }

        let scheduled_seq = client.borrow().scheduled_roundtrip_seq.get();
        if scheduled_seq > 0 {
            state.send_message(&message::RoundtripDone::new(scheduled_seq));
            client.borrow().scheduled_roundtrip_seq.set(0);
        }
    }

    pub fn dispatch_existing_connections(&mut self) -> bool {
        let mut had_any = false;
        let mut needs_poll_recheck = false;

        let internal_fds = self.internal_fds();

        for i in internal_fds..self.pollfds.len() {
            let revents = match self.pollfds[i].revents() {
                Some(r) => r,
                None => continue,
            };

            if !revents.contains(poll::PollFlags::POLLIN) {
                continue;
            }

            let client_idx = i - internal_fds;
            self.dispatch_client(&self.clients[client_idx].clone());

            had_any = true;

            if revents.contains(poll::PollFlags::POLLHUP) {
                self.clients[client_idx].borrow_mut().state.error.store(true, sync::atomic::Ordering::Relaxed);
                needs_poll_recheck = true;
                trace! {
                    log::debug!(
                        "[{} @ {:.3}] Dropping client (hangup)",
                        self.clients[client_idx].borrow().state.fd,
                        steady_millis(),
                    )
                }
                continue;
            }

            if self.clients[client_idx].borrow().state.error.load(sync::atomic::Ordering::Relaxed) {
                trace! {
                    log::debug!(
                        "[{} @ {:.3}] Dropping client (protocol error)",
                        self.clients[client_idx].borrow().state.fd,
                        steady_millis(),
                    )
                }
            }
        }

        if needs_poll_recheck {
            self.clients.retain(|c| !c.borrow().state.error.load(sync::atomic::Ordering::Relaxed));
            self.recheck_pollfds();
        }

        had_any
    }

    fn internal_fds(&self) -> usize {
        if self.is_empty_listener { 2 } else { 3 }
    }

    pub fn dispatch_new_connections(&mut self) -> bool {
        if self.is_empty_listener {
            return false;
        }

        let revents = match self.pollfds[0].revents() {
            Some(r) => r,
            None => return false,
        };

        if !revents.contains(poll::PollFlags::POLLIN) {
            return false;
        }

        let server = match &self.server {
            Some(s) => s,
            None => return false,
        };

        let (stream, _addr) = match server.accept() {
            Ok(conn) => conn,
            Err(e) => {
                log::error!("failed to accept connection: {e}");
                return false;
            }
        };

        let state = sync::Arc::new(SharedState::new(stream));
        let client = rc::Rc::new(cell::RefCell::new(server_client::ServerClient::new(
            sync::Arc::clone(&state),
            self._self.clone(),
        )));

        self.clients.push(client);
        self.recheck_pollfds();

        true
    }

    fn recheck_pollfds(&mut self) {
        self.pollfds.clear();

        if !self.is_empty_listener
            && let Some(server) = &self.server
        {
            let fd = unsafe { BorrowedFd::borrow_raw(server.as_fd().as_raw_fd()) };
            self.pollfds
                .push(poll::PollFd::new(fd, poll::PollFlags::POLLIN));
        }

        let fd = unsafe { BorrowedFd::borrow_raw(self.exit_fd.as_fd().as_raw_fd()) };
        self.pollfds
            .push(poll::PollFd::new(fd, poll::PollFlags::POLLIN));

        let fd = unsafe { BorrowedFd::borrow_raw(self.wakeup_fd.as_fd().as_raw_fd()) };
        self.pollfds
            .push(poll::PollFd::new(fd, poll::PollFlags::POLLIN));

        for client in &self.clients {
            let fd = unsafe { BorrowedFd::borrow_raw(client.borrow().state.fd) };
            self.pollfds
                .push(poll::PollFd::new(fd, poll::PollFlags::POLLIN));
        }
    }

    pub fn dispatch_events(&mut self, block: bool) -> bool {
        while self.dispatch_pending() {}

        self.clear_exit_fd();
        self.clear_wakeup_fd();

        if block {
            let _ = poll::poll(&mut self.pollfds, poll::PollTimeout::NONE);
            while self.dispatch_pending() {}
        }

        true
    }
}
