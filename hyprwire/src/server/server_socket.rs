use super::server_client;
use crate::{SharedState, message, socket, steady_millis, trace};
use nix::poll;
use std::os::fd;
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::net;
use std::sync;
use std::{fs, io, path, rc, thread};

use crate::implementation;

/// Server-side entry point for accepting clients and dispatching Hyprwire
/// protocol traffic.
///
/// A `ServerSocket` can either listen on a Unix socket path or operate without
/// a listener and accept already-connected client file descriptors via
/// [`ServerSocket::add_client`].
pub struct ServerSocket {
    server: Option<net::UnixListener>,
    export_fd: Option<net::UnixStream>,
    export_write_fd: Option<net::UnixStream>,
    wakeup_fd: net::UnixStream,
    wakeup_write_fd: net::UnixStream,
    exit_fd: net::UnixStream,
    exit_write_fd: net::UnixStream,
    is_empty_listener: bool,
    impls: rc::Rc<Vec<Box<dyn implementation::server::ProtocolImplementations>>>,
    clients: Vec<rc::Rc<server_client::ServerClientState>>,
    pollfds: Vec<poll::PollFd<'static>>,
    poll_thread: Option<thread::JoinHandle<()>>,
    poll_mtx: sync::Arc<sync::Mutex<()>>,
    export_poll_mtx: sync::Arc<sync::Mutex<bool>>,
    export_poll_cv: sync::Arc<sync::Condvar>,
    thread_client_fds: sync::Arc<sync::Mutex<Vec<i32>>>,
    next_client_id: u32,
}

impl ServerSocket {
    /// Opens a Hyprwire server socket.
    ///
    /// If `path` is `Some`, the server listens on that Unix socket path. If it
    /// is `None`, the server starts without a listener and can be used only
    /// with clients added through [`ServerSocket::add_client`].
    ///
    /// # Errors
    /// Returns an error if socket creation fails, the socket path cannot be
    /// prepared or bound, or an existing live server is already listening on
    /// the requested path.
    pub fn open<T>(path: Option<&T>) -> io::Result<Self>
    where
        T: AsRef<path::Path>,
    {
        let wake_pipes = net::UnixStream::pair()?;
        let exit_pipes = net::UnixStream::pair()?;

        let poll_mtx = sync::Arc::new(sync::Mutex::new(()));
        let export_poll_mtx = sync::Arc::new(sync::Mutex::new(false));
        let export_poll_cv = sync::Arc::new(sync::Condvar::new());

        let mut this = match path.as_ref() {
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
                    impls: rc::Rc::new(Vec::new()),
                    clients: Vec::new(),
                    pollfds: Vec::new(),
                    poll_thread: None,
                    poll_mtx,
                    export_poll_mtx,
                    export_poll_cv,
                    thread_client_fds: sync::Arc::new(sync::Mutex::new(Vec::new())),
                    next_client_id: 1,
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
                impls: rc::Rc::new(Vec::new()),
                clients: Vec::new(),
                pollfds: Vec::new(),
                poll_thread: None,
                poll_mtx,
                export_poll_mtx,
                export_poll_cv,
                thread_client_fds: sync::Arc::new(sync::Mutex::new(Vec::new())),
                next_client_id: 1,
            },
        };

        this.recheck_pollfds();
        Ok(this)
    }

    /// Registers a protocol implementation on the server.
    ///
    /// # Panics
    /// New implementation is added while client is connected to socket
    pub fn add_implementation<T>(&mut self, implementation: T)
    where
        T: implementation::server::ProtocolImplementations + 'static,
    {
        rc::Rc::get_mut(&mut self.impls)
            .expect("cannot add implementations after clients connect")
            .push(Box::new(implementation));
    }

    pub(crate) fn dispatch_pending<D>(&mut self, dispatch: &mut D) -> bool {
        let _ = poll::poll(&mut self.pollfds, poll::PollTimeout::ZERO);

        if self.dispatch_new_connections() {
            return self.dispatch_pending(dispatch);
        }

        self.dispatch_existing_connections(dispatch)
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

    fn clear_event_fd(&self) {
        if let Some(fd) = &self.export_fd {
            Self::clear_fd(fd);
        }
    }

    fn dispatch_client<D>(client: &rc::Rc<server_client::ServerClientState>, dispatch: &mut D) {
        let state = rc::Rc::clone(&client.state);

        let mut data = {
            if let Ok(d) = socket::SocketRawParsedMessage::read_from_socket(&state.stream) {
                d
            } else {
                state.send_message(&message::FatalProtocolError::new(
                    0,
                    u32::MAX,
                    "fatal: invalid message on wire",
                ));
                state.error.set(true);
                let _ = state.stream.shutdown(std::net::Shutdown::Both);
                return;
            }
        };

        if data.data.is_empty() {
            state.error.set(true);
            let _ = state.stream.shutdown(std::net::Shutdown::Both);
            return;
        }

        if message::handle_message(&mut data, &message::Role::Server(client), dispatch).is_err() {
            state.send_message(&message::FatalProtocolError::new(
                0,
                u32::MAX,
                "fatal: failed to handle message on wire",
            ));
            state.error.set(true);
            let _ = state.stream.shutdown(std::net::Shutdown::Both);
            return;
        }

        let scheduled_seq = client.scheduled_roundtrip_seq.get();
        if scheduled_seq > 0 {
            state.send_message(&message::RoundtripDone::new(scheduled_seq));
            client.scheduled_roundtrip_seq.set(0);
        }
    }

    pub(crate) fn dispatch_existing_connections<D>(&mut self, dispatch: &mut D) -> bool {
        let mut had_any = false;
        let mut needs_poll_recheck = false;

        let internal_fds = self.internal_fds();

        for i in internal_fds..self.pollfds.len() {
            let Some(revents) = self.pollfds.get(i).and_then(poll::PollFd::revents) else {
                continue;
            };

            let has_input = revents.contains(poll::PollFlags::POLLIN);
            let has_hangup = revents.contains(poll::PollFlags::POLLHUP);

            if !has_input && !has_hangup {
                continue;
            }

            let client_idx = i - internal_fds;

            if has_input {
                Self::dispatch_client(&self.clients[client_idx], dispatch);
                had_any = true;
            }

            if has_hangup {
                had_any = true;
                self.clients[client_idx].state.error.set(true);
                let _ = self.clients[client_idx]
                    .state
                    .stream
                    .shutdown(std::net::Shutdown::Both);
                self.clients[client_idx].destroy_objects_for_disconnect(dispatch);
                needs_poll_recheck = true;
                trace! {
                    eprintln!(
                        "[hw] trace: [{} @ {:.3}] Dropping client (hangup)",
                        self.clients[client_idx].state.stream.as_raw_fd(),
                        steady_millis(),
                    )
                }
                continue;
            }

            if self.clients[client_idx].state.error.get() {
                self.clients[client_idx].destroy_objects_for_disconnect(dispatch);
                needs_poll_recheck = true;
                trace! {
                    eprintln!(
                        "[hw] trace: [{} @ {:.3}] Dropping client (protocol error)",
                        self.clients[client_idx].state.stream.as_raw_fd(),
                        steady_millis(),
                    )
                }
            }
        }

        if needs_poll_recheck {
            self.clients.retain(|c| !c.state.error.get());
            self.recheck_pollfds();
        }

        had_any
    }

    fn internal_fds(&self) -> usize {
        if self.is_empty_listener { 2 } else { 3 }
    }

    pub(crate) fn dispatch_new_connections(&mut self) -> bool {
        if self.is_empty_listener {
            return false;
        }

        let Some(revents) = self.pollfds.first().and_then(poll::PollFd::revents) else {
            return false;
        };

        if !revents.contains(poll::PollFlags::POLLIN) {
            return false;
        }

        let Some(server) = self.server.as_ref() else {
            return false;
        };

        let (stream, _addr) = match server.accept() {
            Ok(conn) => conn,
            Err(e) => {
                log::error!("failed to accept connection: {e}");
                return false;
            }
        };

        let state = rc::Rc::new(SharedState::new(stream, rc::Rc::clone(&self.impls)));
        let client =
            server_client::ServerClientState::new(self.next_client_id, rc::Rc::clone(&state));
        self.next_client_id += 1;

        self.clients.push(client);
        self.recheck_pollfds();

        true
    }

    fn recheck_pollfds(&mut self) {
        self.pollfds.clear();

        if !self.is_empty_listener
            && let Some(server) = &self.server
        {
            let fd = unsafe { fd::BorrowedFd::borrow_raw(server.as_fd().as_raw_fd()) };
            self.pollfds
                .push(poll::PollFd::new(fd, poll::PollFlags::POLLIN));
        }

        let fd = unsafe { fd::BorrowedFd::borrow_raw(self.exit_fd.as_fd().as_raw_fd()) };
        self.pollfds
            .push(poll::PollFd::new(fd, poll::PollFlags::POLLIN));

        let fd = unsafe { fd::BorrowedFd::borrow_raw(self.wakeup_fd.as_fd().as_raw_fd()) };
        self.pollfds
            .push(poll::PollFd::new(fd, poll::PollFlags::POLLIN));

        for client in &self.clients {
            let fd = unsafe { fd::BorrowedFd::borrow_raw(client.state.stream.as_raw_fd()) };
            self.pollfds
                .push(poll::PollFd::new(fd, poll::PollFlags::POLLIN));
        }

        let mut cfds = self.thread_client_fds.lock().unwrap();
        *cfds = self
            .clients
            .iter()
            .map(|c| c.state.stream.as_raw_fd())
            .collect();
    }

    /// Processes pending protocol traffic for connected clients.
    ///
    /// Pass the dispatch state that receives generated event callbacks. If
    /// `block` is `true`, this call waits until at least one event source
    /// becomes ready before dispatching work.
    ///
    /// # Panics
    /// Panics if an internal synchronization mutex has been poisoned by a
    /// panic in another thread while coordinating poll/export state.
    pub fn dispatch_events<D>(&mut self, state: &mut D, block: bool) -> bool {
        let mtx = sync::Arc::clone(&self.poll_mtx);
        let poll_guard = mtx.lock().unwrap();

        while self.dispatch_pending(state) {}

        self.clear_event_fd();
        self.clear_exit_fd();
        self.clear_wakeup_fd();

        if block {
            let _ = poll::poll(&mut self.pollfds, poll::PollTimeout::NONE);
            while self.dispatch_pending(state) {}
        }

        drop(poll_guard);

        let export_mtx = sync::Arc::clone(&self.export_poll_mtx);
        let export_cv = sync::Arc::clone(&self.export_poll_cv);
        let mut poll_event = export_mtx.lock().unwrap();
        *poll_event = false;
        export_cv.notify_all();

        true
    }

    /// Adds an already-connected Unix socket as a server client.
    ///
    /// This is primarily useful when the server is running without a listener.
    pub fn add_client<T>(&mut self, fd: T) -> server_client::ServerClient
    where
        T: Into<fd::OwnedFd>,
    {
        let stream = net::UnixStream::from(fd.into());
        let state = rc::Rc::new(SharedState::new(stream, rc::Rc::clone(&self.impls)));
        let client_id = self.next_client_id;
        let client = server_client::ServerClientState::new(client_id, rc::Rc::clone(&state));
        self.next_client_id += 1;

        self.clients.push(rc::Rc::clone(&client));
        self.recheck_pollfds();

        // wake up any poller
        let _ = io::Write::write(&mut &self.wakeup_write_fd, b"x");

        server_client::ServerClient {
            id: client_id,
            pid: client.pid.clone(),
        }
    }

    /// Removes a client previously added to the server.
    ///
    /// Returns `true` if a matching client handle was present.
    pub fn remove_client<D>(
        &mut self,
        client: &server_client::ServerClient,
        dispatch: &mut D,
    ) -> bool {
        for state in self.clients.iter().filter(|c| c.id == client.id()) {
            state.state.error.set(true);
            let _ = state.state.stream.shutdown(std::net::Shutdown::Both);
            state.destroy_objects_for_disconnect(dispatch);
        }

        let before = self.clients.len();
        self.clients.retain(|c| c.id != client.id());
        let removed = self.clients.len() < before;

        if removed {
            self.recheck_pollfds();
        }

        removed
    }

    /// Returns a file descriptor that becomes readable when the server has
    /// work to process.
    ///
    /// This can be integrated into an external event loop. When the returned
    /// descriptor is readable, call [`ServerSocket::dispatch_events`] to accept
    /// new clients and process pending protocol traffic.
    ///
    /// # Errors
    /// Returns an error if creating the internal wakeup pipe for exported loop
    /// integration fails.
    ///
    /// # Panics
    /// Panics if an internal synchronization mutex has been poisoned by a
    /// panic in another thread while coordinating poll/export state.
    pub fn extract_loop_fd(&mut self) -> io::Result<fd::BorrowedFd<'_>> {
        if self.export_fd.is_none() {
            let export_pipes = net::UnixStream::pair()?;

            let export_write_fd = export_pipes.1.as_raw_fd();

            self.export_fd = Some(export_pipes.0);
            self.export_write_fd = Some(export_pipes.1);

            self.recheck_pollfds();

            let poll_mtx = sync::Arc::clone(&self.poll_mtx);
            let export_poll_mtx = sync::Arc::clone(&self.export_poll_mtx);
            let export_poll_cv = sync::Arc::clone(&self.export_poll_cv);

            let server_fd = self.server.as_ref().map(|s| s.as_fd().as_raw_fd());
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
                                unsafe { fd::BorrowedFd::borrow_raw(fd) },
                                poll::PollFlags::POLLIN,
                            ));
                        }

                        pollfds.push(poll::PollFd::new(
                            unsafe { fd::BorrowedFd::borrow_raw(exit_fd) },
                            poll::PollFlags::POLLIN,
                        ));
                        pollfds.push(poll::PollFd::new(
                            unsafe { fd::BorrowedFd::borrow_raw(wakeup_fd) },
                            poll::PollFlags::POLLIN,
                        ));

                        let cfds = client_fds.lock().unwrap();
                        for &fd in cfds.iter() {
                            pollfds.push(poll::PollFd::new(
                                unsafe { fd::BorrowedFd::borrow_raw(fd) },
                                poll::PollFlags::POLLIN,
                            ));
                        }
                    }

                    let _ = poll::poll(&mut pollfds, poll::PollTimeout::NONE);

                    // check exit fd
                    for pfd in &pollfds {
                        if let Some(revents) = pfd.revents()
                            && revents.contains(poll::PollFlags::POLLIN)
                            && pfd.as_fd().as_raw_fd() == exit_fd
                        {
                            return;
                        }
                    }

                    {
                        let mut poll_event = export_poll_mtx.lock().unwrap();
                        *poll_event = true;
                        let _ = nix::unistd::write(
                            unsafe { fd::BorrowedFd::borrow_raw(export_write_fd) },
                            b"x",
                        );

                        let result = export_poll_cv.wait_timeout_while(
                            poll_event,
                            std::time::Duration::from_secs(5),
                            |event| *event,
                        );
                        if let Ok((guard, timeout)) = result
                            && timeout.timed_out()
                            && *guard
                        {}
                    }
                }
            }));
        }

        Ok(self.export_fd.as_ref().unwrap().as_fd())
    }
}

impl Drop for ServerSocket {
    fn drop(&mut self) {
        if self.poll_thread.is_some() {
            let _ = io::Write::write(&mut &self.exit_write_fd, b"x");
        }
        if let Some(thread) = self.poll_thread.take() {
            let _ = thread.join();
        }
    }
}
