mod client_object;
pub(crate) mod client_socket;
pub(crate) mod event_queue;
mod server_spec;

use crate::implementation::client::ProtocolImplementations;
use crate::implementation::object;
use std::os::fd;
use std::sync::atomic;
use std::{path, sync};

/// Client-side entry point for connecting to a Hyprwire server and dispatching
/// protocol events.
///
/// A `Client` can connect directly to a Unix socket path or take ownership of
/// an already-connected Unix socket file descriptor.
pub struct Client(pub(crate) sync::Arc<client_socket::ClientSocket>);

impl Client {
    /// Connects to a Hyprwire server over a Unix socket path.
    ///
    /// # Errors
    /// Returns any I/O error produced while opening the Unix socket.
    pub fn connect<P>(path: P) -> crate::Result<Self>
    where
        P: AsRef<path::Path>,
    {
        Ok(Self(client_socket::ClientSocket::connect(path)?))
    }

    /// Creates a client from an already-connected Unix socket file descriptor.
    ///
    /// The returned client takes ownership of `fd`.
    ///
    /// # Errors
    /// Failed to move the socket into or out of nonblocking mode
    pub fn from_fd<F>(fd: F) -> crate::Result<Self>
    where
        F: Into<fd::OwnedFd>,
    {
        Ok(Self(client_socket::ClientSocket::from_fd(fd)?))
    }

    /// Creates an [`EventQueue`][event_queue::EventQueue] backed by this client connection.
    pub fn new_event_queue(&self) -> event_queue::EventQueue {
        event_queue::EventQueue::new(sync::Arc::clone(&self.0))
    }

    /// Registers a protocol implementation on the client.
    pub fn add_implementation<I>(&mut self)
    where
        I: ProtocolImplementations + 'static,
    {
        self.0.add_implementation(Box::new(I::new()));
    }

    #[must_use]
    /// Returns a file descriptor that becomes readable when the client has
    /// work to process.
    ///
    /// The descriptor remains owned by the client and must not be closed by
    /// the caller.
    pub fn extract_loop_fd(&self) -> fd::BorrowedFd<'_> {
        self.0.extract_loop_fd()
    }

    #[must_use]
    /// Returns `true` once the initial handshake has completed successfully.
    pub fn is_handshake_done(&self) -> bool {
        self.0.handshake_done.load(atomic::Ordering::Relaxed)
    }

    /// Binds a server-advertised protocol and returns its typed root object.
    ///
    /// The protocol is identified automatically from `O`'s associated
    /// `ProtocolImpl` type. `version` must not exceed the version the server
    /// advertises for that protocol.
    ///
    /// # Errors
    /// Returns [`Error::ProtocolNotFound`][crate::Error::ProtocolNotFound] if
    /// the server does not advertise the protocol, or an error if the requested
    /// version is invalid, the connection closes, or object creation fails.
    pub fn bind<O, D>(
        &self,
        event_queue: &event_queue::EventQueue,
        state: &mut D,
        version: u32,
    ) -> crate::Result<O>
    where
        O: crate::Object,
        O::ProtocolImpl: ProtocolImplementations,
        D: crate::Dispatch<O> + 'static,
    {
        let advertised = self
            .0
            .get_spec(O::ProtocolImpl::spec_name())
            .ok_or(crate::Error::ProtocolNotFound)?;
        let spec = server_spec::ServerSpec::<O::ProtocolImpl>::new(advertised.version());
        let obj = self.0.bind_protocol(event_queue, &spec, version)?;
        // Install event handlers before waiting so events that arrive in the same
        // socket read with NewObject are dispatched rather than dropped.
        let typed = O::from_object::<D>(sync::Arc::clone(&obj) as sync::Arc<dyn object::Object>);
        self.0.wait_for_object(event_queue, &obj, state)?;
        Ok(typed)
    }

    #[must_use]
    /// Returns the server-advertised protocol specification with the given
    /// name, if present.
    pub fn get_spec<I>(&self) -> Option<server_spec::ServerSpec<I>>
    where
        I: ProtocolImplementations,
    {
        self.0
            .get_spec(I::spec_name())
            .map(|spec| server_spec::ServerSpec::new(spec.version()))
    }

    #[must_use]
    /// Returns the raw object associated with a pending or resolved sequence
    /// number.
    ///
    /// This is a low-level helper primarily used by generated code and manual
    /// protocol integrations.
    pub fn object_for_seq(&self, seq: u32) -> Option<sync::Arc<dyn object::Object>> {
        self.0
            .object_for_seq(seq)
            .map(|obj| obj as sync::Arc<dyn object::Object>)
    }

    #[must_use]
    /// Returns the raw object with the given wire object id, if known.
    ///
    /// This is a low-level helper primarily used by generated code and manual
    /// protocol integrations.
    pub fn object_for_id(&self, id: u32) -> Option<sync::Arc<dyn object::Object>> {
        self.0
            .object_for_id(id)
            .map(|obj| obj as sync::Arc<dyn object::Object>)
    }
}
