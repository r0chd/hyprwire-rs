use super::server_client;
use crate::implementation::server;
use crate::{message, socket, steady_millis, trace};
use hyprwire_core::message::wire::{fatal_protocol_error, roundtrip_done};
use polling::AsSource;
use std::os::fd;
use std::os::fd::AsRawFd;
use std::os::unix::net;
use std::{cell, fs, io, path, rc, time};

const LISTENER_KEY: usize = 0;

/// Server-side entry point for accepting clients and dispatching Hyprwire
/// protocol traffic.
///
/// A `ServerSocket` can either listen on a Unix socket path or operate without
/// a listener and accept already-connected client file descriptors via
/// [`ServerSocket::add_client`].
pub struct ServerSocket {
    // `poller` must be declared before `server` and `clients`: struct fields
    // drop in declaration order, and the poller's kernel registrations must be
    // released before the streams they reference.
    poller: polling::Poller,
    server: Option<net::UnixListener>,
    impls: rc::Rc<cell::RefCell<Vec<Box<dyn server::ProtocolImplementations>>>>,
    clients: Vec<rc::Rc<server_client::ServerClientState>>,
    next_client_id: u32,
}

impl ServerSocket {
    /// Opens a Hyprwire server socket listening on the given Unix socket path.
    ///
    /// To create a server without a listener (accepting only pre-connected
    /// client file descriptors via [`ServerSocket::add_client`]), use
    /// [`ServerSocket::detached`] instead.
    ///
    /// # Errors
    /// Returns an error if socket creation fails, the socket path cannot be
    /// bound, or an existing live server is already listening on the
    /// requested path.
    pub fn bind<P>(path: &P) -> io::Result<Self>
    where
        P: AsRef<path::Path>,
    {
        let poller = polling::Poller::new()?;

        if fs::exists(path)? {
            match net::UnixStream::connect(path) {
                Ok(_) => {
                    return Err(io::Error::new(io::ErrorKind::AddrInUse, "socket is alive"));
                }
                Err(e) if e.kind() != io::ErrorKind::ConnectionRefused => return Err(e),
                _ => fs::remove_file(path)?,
            }
        }

        let listener = net::UnixListener::bind(path)?;
        listener.set_nonblocking(true)?;

        unsafe { poller.add(&listener, polling::Event::readable(LISTENER_KEY))? };

        Ok(Self {
            poller,
            server: Some(listener),
            impls: rc::Rc::new(cell::RefCell::new(Vec::new())),
            clients: Vec::new(),
            next_client_id: 1,
        })
    }

    /// Opens a Hyprwire server socket without a listener.
    ///
    /// Such a server accepts only pre-connected client file descriptors added
    /// through [`ServerSocket::add_client`].
    ///
    /// # Errors
    /// Returns an error if poller creation fails.
    pub fn detached() -> io::Result<Self> {
        Ok(Self {
            poller: polling::Poller::new()?,
            server: None,
            impls: rc::Rc::new(cell::RefCell::new(Vec::new())),
            clients: Vec::new(),
            next_client_id: 1,
        })
    }

    /// Registers a protocol implementation on the server.
    pub fn add_implementation<I, H>(&mut self, version: u32, handler: &mut H)
    where
        I: server::Construct<H> + 'static,
    {
        let implementation = I::new(version, handler);
        self.impls.borrow_mut().push(Box::new(implementation));
    }

    fn dispatch_client<D: 'static>(
        client: &rc::Rc<server_client::ServerClientState>,
        dispatch: &mut D,
    ) {
        let state = rc::Rc::clone(&client.state);

        let mut data = {
            if let Ok(d) = socket::SocketRawParsedMessage::read_from_socket(&state.stream) {
                d
            } else {
                state.send_message(&fatal_protocol_error::FatalProtocolError::new(
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
            state.send_message(&fatal_protocol_error::FatalProtocolError::new(
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
            state.send_message(&roundtrip_done::RoundtripDone::new(scheduled_seq));
            client.scheduled_roundtrip_seq.set(0);
        }
    }

    fn accept_one(&mut self) -> io::Result<bool> {
        let Some(server) = self.server.as_ref() else {
            return Ok(false);
        };

        let (stream, _addr) = match server.accept() {
            Ok(conn) => conn,
            Err(e) => {
                crate::log_error!("failed to accept connection: {e}");
                return Ok(false);
            }
        };

        if stream.set_nonblocking(true).is_err() {
            return Ok(false);
        }

        let state = rc::Rc::new(crate::ConnectionState::new(
            stream,
            rc::Rc::clone(&self.impls),
        ));
        let client_id = self.next_client_id;
        let client = server_client::ServerClientState::new(client_id, rc::Rc::clone(&state));

        unsafe {
            self.poller.add(
                &client.state.stream,
                polling::Event::readable(client_id as usize),
            )?;
        }

        self.next_client_id += 1;
        self.clients.push(client);
        Ok(true)
    }

    fn dispatch_pending<D: 'static>(&mut self, dispatch: &mut D, block: bool) -> io::Result<bool> {
        let mut events = polling::Events::new();
        let timeout = if block {
            None
        } else {
            Some(time::Duration::ZERO)
        };
        self.poller.wait(&mut events, timeout)?;

        if events.is_empty() {
            return Ok(false);
        }

        let mut dead: Vec<u32> = Vec::new();

        for ev in events.iter() {
            if ev.key == LISTENER_KEY {
                let _ = self.accept_one()?;

                if let Some(server) = self.server.as_ref() {
                    self.poller
                        .modify(server, polling::Event::readable(LISTENER_KEY))?;
                }

                continue;
            }

            let id = ev.key as u32;
            let Some(client) = self.clients.iter().find(|c| c.id == id).map(rc::Rc::clone) else {
                continue;
            };

            Self::dispatch_client(&client, dispatch);

            if client.state.error.get() {
                dead.push(id);
            } else {
                self.poller
                    .modify(&client.state.stream, polling::Event::readable(id as usize))?;
            }
        }

        for id in dead {
            let Some(idx) = self.clients.iter().position(|c| c.id == id) else {
                continue;
            };
            let client = self.clients.remove(idx);
            client.destroy_objects_for_disconnect(dispatch);
            let _ = self.poller.delete(&client.state.stream);
            trace! {
                crate::log_debug!(
                    "[hw] trace: [{} @ {:.3}] Dropping client",
                    client.state.stream.as_raw_fd(),
                    steady_millis(),
                )
            }
        }

        Ok(true)
    }

    /// Processes pending protocol traffic for connected clients.
    ///
    /// Pass the dispatch state that receives generated event callbacks. If
    /// `block` is `true`, this call waits until at least one event source
    /// becomes ready before dispatching work.
    pub fn dispatch_events<D: 'static>(&mut self, state: &mut D, block: bool) -> crate::Result<()> {
        let mut first = true;
        loop {
            let do_block = block && first;
            let any = self
                .dispatch_pending(state, do_block)
                .map_err(crate::Error::Io)?;
            first = false;
            if !any {
                break;
            }
        }
        Ok(())
    }

    /// Adds an already-connected Unix socket as a server client.
    ///
    /// This is primarily useful when the server is running without a listener.
    pub fn add_client<F>(&mut self, fd: F) -> crate::Result<server_client::ServerClient>
    where
        F: Into<fd::OwnedFd>,
    {
        let stream = net::UnixStream::from(fd.into());
        _ = stream.set_nonblocking(true);
        let state = rc::Rc::new(crate::ConnectionState::new(
            stream,
            rc::Rc::clone(&self.impls),
        ));
        let client_id = self.next_client_id;
        let client = server_client::ServerClientState::new(client_id, rc::Rc::clone(&state));

        // SAFETY: see `accept_one` — same drop-order argument.
        if let Err(e) = unsafe {
            self.poller.add(
                &client.state.stream,
                polling::Event::readable(client_id as usize),
            )
        } {
            return Err(crate::Error::Io(e));
        }

        self.next_client_id += 1;
        self.clients.push(rc::Rc::clone(&client));

        Ok(server_client::ServerClient {
            id: client_id,
            creds: client.creds.clone(),
        })
    }

    /// Removes a client previously added to the server.
    ///
    /// Returns `true` if a matching client handle was present.
    pub fn remove_client<D: 'static>(
        &mut self,
        client: &server_client::ServerClient,
        dispatch: &mut D,
    ) -> crate::Result<bool> {
        for state in self.clients.iter().filter(|c| c.id == client.id()) {
            state.state.error.set(true);
            let _ = state.state.stream.shutdown(std::net::Shutdown::Both);
            state.destroy_objects_for_disconnect(dispatch);
            let _ = self.poller.delete(&state.state.stream);
        }

        let before = self.clients.len();
        self.clients.retain(|c| c.id != client.id());
        Ok(self.clients.len() < before)
    }

    /// Returns a file descriptor that becomes readable when the server has
    /// work to process.
    ///
    /// This can be integrated into an external event loop. When the returned
    /// descriptor is readable, call [`ServerSocket::dispatch_events`] to accept
    /// new clients and process pending protocol traffic. The fd is the
    /// underlying [`polling::Poller`] source (epollfd on Linux, kqueuefd on
    /// BSD/macOS), which is itself pollable.
    pub fn extract_loop_fd(&self) -> fd::BorrowedFd<'_> {
        self.poller.source()
    }
}
