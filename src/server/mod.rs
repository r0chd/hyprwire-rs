pub(crate) mod server_client;
mod server_object;
mod server_socket;

use crate::implementation::server;
use std::os::fd::RawFd;
use std::{io, path, ptr};

/// Server-side entry point for accepting clients and dispatching Hyprwire
/// protocol traffic.
///
/// A `Server` can either listen on a Unix socket path or operate without a
/// listener and accept already-connected client file descriptors via
/// [`Server::add_client`].
pub struct Server(server_socket::ServerSocket);

impl Server {
    /// Opens a Hyprwire server socket.
    ///
    /// If `path` is `Some`, the server listens on that Unix socket path. If it
    /// is `None`, the server starts without a listener and can be used only
    /// with clients added through [`Server::add_client`].
    pub fn open(path: Option<&path::Path>) -> io::Result<Self> {
        Ok(Self(server_socket::ServerSocket::open(path)?))
    }

    /// Registers a protocol implementation on the server.
    ///
    /// Implementations must be added before any clients connect. Once a client
    /// has been added, the server freezes the implementation list and further
    /// calls will panic.
    pub fn add_implementation<T>(&mut self, p_impl: T)
    where
        T: server::ProtocolImplementations + 'static,
    {
        self.0.add_implementation(Box::new(p_impl));
    }

    /// Processes pending protocol traffic for connected clients.
    ///
    /// Pass the dispatch state that receives generated event callbacks. If
    /// `block` is `true`, this call waits until at least one event source
    /// becomes ready before dispatching work.
    pub fn dispatch_events<D>(&mut self, state: &mut D, block: bool) -> bool {
        crate::set_dispatch_state(ptr::from_mut::<D>(state).cast());
        let result = self.0.dispatch_events(block);
        crate::set_dispatch_state(std::ptr::null_mut());
        result
    }

    /// Returns a file descriptor that becomes readable when the server has
    /// work to process.
    ///
    /// This can be integrated into an external event loop. When the returned
    /// descriptor is readable, call [`Server::dispatch_events`] to accept new
    /// clients and process pending protocol traffic.
    ///
    /// The returned descriptor remains owned by the server and is valid for as
    /// long as the server stays alive.
    pub fn extract_loop_fd(&mut self) -> io::Result<RawFd> {
        self.0.extract_loop_fd()
    }

    /// Adds an already-connected Unix socket as a server client.
    ///
    /// This is primarily useful when the server is running without a listener.
    pub fn add_client(&mut self, fd: RawFd) {
        self.0.add_client(fd);
    }

    /// Removes a client previously added to the server.
    ///
    /// Returns `true` if a matching client file descriptor was present.
    pub fn remove_client(&mut self, fd: RawFd) -> bool {
        self.0.remove_client(fd)
    }
}
