use super::server_client;
use crate::{SharedState, message, socket, steady_millis, trace};
use nix::poll;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::os::unix::net;
use std::sync;
use std::{cell, fs, io, path, rc, thread};

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
    impls: sync::Arc<Vec<Box<dyn implementation::server::ProtocolImplementations>>>,
    clients: Vec<rc::Rc<cell::RefCell<server_client::ServerClient>>>,
    pollfds: Vec<poll::PollFd<'static>>,
    poll_thread: Option<thread::JoinHandle<()>>,
    poll_mtx: sync::Arc<sync::Mutex<()>>,
    export_poll_mtx: sync::Arc<sync::Mutex<bool>>,
    export_poll_cv: sync::Arc<sync::Condvar>,
    thread_exit_fd: Option<net::UnixStream>,
    thread_exit_write_fd: Option<net::UnixStream>,
    thread_client_fds: sync::Arc<sync::Mutex<Vec<i32>>>,
}

impl ServerSocket {
    pub fn open(path: Option<&path::Path>) -> io::Result<Self> {
        let wake_pipes = net::UnixStream::pair()?;
        let exit_pipes = net::UnixStream::pair()?;

        let poll_mtx = sync::Arc::new(sync::Mutex::new(()));
        let export_poll_mtx = sync::Arc::new(sync::Mutex::new(false));
        let export_poll_cv = sync::Arc::new(sync::Condvar::new());

        let mut this = match path {
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
                Self {
                    server: Some(socket),
                    export_fd: None,
                    export_write_fd: None,
                    wakeup_fd: wake_pipes.0,
                    wakeup_write_fd: wake_pipes.1,
                    exit_fd: exit_pipes.0,
                    exit_write_fd: exit_pipes.1,
                    is_empty_listener: false,
                    impls: sync::Arc::new(Vec::new()),
                    clients: Vec::new(),
                    pollfds: Vec::new(),
                    poll_thread: None,
                    poll_mtx,
                    export_poll_mtx,
                    export_poll_cv,
                    thread_exit_fd: None,
                    thread_exit_write_fd: None,
                    thread_client_fds: sync::Arc::new(sync::Mutex::new(Vec::new())),
                }
            }
            None => Self {
                server: None,
                export_fd: None,
                export_write_fd: None,
                wakeup_fd: wake_pipes.0,
                wakeup_write_fd: wake_pipes.1,
                exit_fd: exit_pipes.0,
                exit_write_fd: exit_pipes.1,
                is_empty_listener: true,
                impls: sync::Arc::new(Vec::new()),
                clients: Vec::new(),
                pollfds: Vec::new(),
                poll_thread: None,
                poll_mtx,
                export_poll_mtx,
                export_poll_cv,
                thread_exit_fd: None,
                thread_exit_write_fd: None,
                thread_client_fds: sync::Arc::new(sync::Mutex::new(Vec::new())),
            },
        };

        this.recheck_pollfds();
        Ok(this)
    }

    pub fn add_implementation(
        &mut self,
        implementation: Box<dyn implementation::server::ProtocolImplementations>,
    ) {
        sync::Arc::get_mut(&mut self.impls)
            .expect("cannot add implementations after clients connect")
            .push(implementation);
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
        let state = rc::Rc::clone(&client.borrow().state);

        let mut data = {
            let stream = state.stream.borrow();
            match socket::SocketRawParsedMessage::read_from_socket(&stream) {
                Ok(d) => d,
                Err(_) => {
                    drop(stream);
                    state.send_message(&message::FatalProtocolError::new(
                        0,
                        u32::MAX,
                        "fatal: invalid message on wire",
                    ));
                    state.error.set(true);
                    return;
                }
            }
        };

        if data.data.is_empty() {
            return;
        }

        if message::handle_message(&mut data, message::Role::Server(&client.borrow())).is_err() {
            state.send_message(&message::FatalProtocolError::new(
                0,
                u32::MAX,
                "fatal: failed to handle message on wire",
            ));
            state.error.set(true);
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
                self.clients[client_idx].borrow().state.error.set(true);
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

            if self.clients[client_idx].borrow().state.error.get() {
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
            self.clients.retain(|c| !c.borrow().state.error.get());
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

        let state = rc::Rc::new(SharedState::with_impls(
            stream,
            sync::Arc::clone(&self.impls),
        ));
        let client = server_client::ServerClient::new(rc::Rc::clone(&state));

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

        // sync client fds for the poll thread
        if self.poll_thread.is_some() {
            let mut cfds = self.thread_client_fds.lock().unwrap();
            *cfds = self.clients.iter().map(|c| c.borrow().state.fd).collect();
        }
    }

    pub fn dispatch_events(&mut self, block: bool) -> bool {
        let mtx = sync::Arc::clone(&self.poll_mtx);
        let _poll_guard = mtx.lock().unwrap();

        while self.dispatch_pending() {}

        self.clear_exit_fd();
        self.clear_wakeup_fd();

        if block {
            let _ = poll::poll(&mut self.pollfds, poll::PollTimeout::NONE);
            while self.dispatch_pending() {}
        }

        drop(_poll_guard);

        let export_mtx = sync::Arc::clone(&self.export_poll_mtx);
        let export_cv = sync::Arc::clone(&self.export_poll_cv);
        let mut poll_event = export_mtx.lock().unwrap();
        *poll_event = false;
        export_cv.notify_all();

        true
    }

    pub fn extract_loop_fd(&mut self) -> io::Result<i32> {
        if let Some(export_fd) = self.export_fd.as_ref() {
            return Ok(export_fd.as_raw_fd());
        }

        let export_pipes = net::UnixStream::pair()?;
        let exit_pipes = net::UnixStream::pair()?;

        self.export_fd = Some(export_pipes.0);
        self.export_write_fd = Some(export_pipes.1);
        self.thread_exit_fd = Some(exit_pipes.0);
        self.thread_exit_write_fd = Some(exit_pipes.1);

        self.recheck_pollfds();

        let poll_mtx = sync::Arc::clone(&self.poll_mtx);
        let export_poll_mtx = sync::Arc::clone(&self.export_poll_mtx);
        let export_poll_cv = sync::Arc::clone(&self.export_poll_cv);

        let export_write_fd = self.export_write_fd.as_ref().unwrap().as_raw_fd();
        let thread_exit_fd = self.thread_exit_fd.as_ref().unwrap().as_raw_fd();

        let server_fd = self
            .server
            .as_ref()
            .map(|s| s.as_fd().as_raw_fd());
        let is_empty_listener = self.is_empty_listener;
        let exit_fd = self.exit_fd.as_raw_fd();
        let wakeup_fd = self.wakeup_fd.as_raw_fd();

        let client_fds = sync::Arc::clone(&self.thread_client_fds);

        self.poll_thread = Some(thread::spawn(move || {
            loop {
                let mut pollfds = Vec::new();

                {
                    let _guard = poll_mtx.lock().unwrap();

                    if !is_empty_listener && let Some(fd) = server_fd {
                        pollfds.push(poll::PollFd::new(
                            unsafe { BorrowedFd::borrow_raw(fd) },
                            poll::PollFlags::POLLIN,
                        ));
                    }

                    pollfds.push(poll::PollFd::new(
                        unsafe { BorrowedFd::borrow_raw(exit_fd) },
                        poll::PollFlags::POLLIN,
                    ));
                    pollfds.push(poll::PollFd::new(
                        unsafe { BorrowedFd::borrow_raw(wakeup_fd) },
                        poll::PollFlags::POLLIN,
                    ));
                    pollfds.push(poll::PollFd::new(
                        unsafe { BorrowedFd::borrow_raw(thread_exit_fd) },
                        poll::PollFlags::POLLIN,
                    ));

                    let cfds = client_fds.lock().unwrap();
                    for &fd in cfds.iter() {
                        pollfds.push(poll::PollFd::new(
                            unsafe { BorrowedFd::borrow_raw(fd) },
                            poll::PollFlags::POLLIN,
                        ));
                    }
                }

                let _ = poll::poll(&mut pollfds, poll::PollTimeout::NONE);

                // check thread exit fd
                for pfd in &pollfds {
                    if let Some(revents) = pfd.revents() && revents.contains(poll::PollFlags::POLLIN)
                            && pfd.as_fd().as_raw_fd() == thread_exit_fd
                        {
                            return;
                    }
                }

                {
                    let mut poll_event = export_poll_mtx.lock().unwrap();
                    *poll_event = true;
                    let _ = nix::unistd::write(
                        unsafe { BorrowedFd::borrow_raw(export_write_fd) },
                        b"x",
                    );

                    let result = export_poll_cv.wait_timeout_while(
                        poll_event,
                        std::time::Duration::from_secs(5),
                        |event| *event,
                    );
                    if let Ok((guard, timeout)) = result && timeout.timed_out() && *guard {
                        continue;
                    }
                }
            }
        }));

        Ok(self.export_fd.as_ref().unwrap().as_raw_fd())
    }
}

impl Drop for ServerSocket {
    fn drop(&mut self) {
        if let Some(exit_write) = &self.thread_exit_write_fd {
            let _ = io::Write::write(&mut &*exit_write, b"x");
        }
        if let Some(thread) = self.poll_thread.take() {
            let _ = thread.join();
        }
    }
}
