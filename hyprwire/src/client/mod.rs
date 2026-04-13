mod client_object;
pub(crate) mod client_socket;
mod server_spec;

use crate::implementation;
use implementation::client::ProtocolImplementations;
use std::os::fd;
use std::{io, path, rc};

/// Client-side entry point for connecting to a Hyprwire server and dispatching
/// protocol events.
///
/// A `Client` can connect directly to a Unix socket path or take ownership of
/// an already-connected Unix socket file descriptor.
pub struct Client(pub(crate) rc::Rc<client_socket::ClientSocket>);

impl Client {
    /// Connects to a Hyprwire server over a Unix socket path.
    ///
    /// # Errors
    /// Returns any I/O error produced while opening the Unix socket.
    pub fn open<T>(path: T) -> io::Result<Self>
    where
        T: AsRef<path::Path>,
    {
        Ok(Self(client_socket::ClientSocket::open(path)?))
    }

    /// Creates a client from an already-connected Unix socket file descriptor.
    ///
    /// The returned client takes ownership of `fd`.
    pub fn from_fd<T>(fd: T) -> io::Result<Self>
    where
        T: Into<fd::OwnedFd>,
    {
        Ok(Self(client_socket::ClientSocket::from_fd(fd)?))
    }

    /// Registers a protocol implementation on the client.
    pub fn add_implementation<T>(&mut self, p_impl: T)
    where
        T: ProtocolImplementations + 'static,
    {
        self.0.add_implementation(Box::new(p_impl));
    }

    /// Blocks until the initial Hyprwire handshake completes.
    ///
    /// Returns an error if the connection closes or the handshake fails.
    ///
    /// # Errors
    /// Returns an error if the connection closes, the handshake times out, or
    /// the server sends invalid handshake traffic.
    pub fn wait_for_handshake<D>(&mut self, state: &mut D) -> Result<(), io::Error> {
        self.0.wait_for_handshake(state)
    }

    /// Dispatches pending events from the server.
    ///
    /// `state` receives generated event callbacks. If `block` is `true`, this
    /// call waits until new protocol traffic is available.
    ///
    /// # Errors
    /// Returns an error if the connection closes, polling fails, or incoming
    /// protocol traffic is malformed.
    pub fn dispatch_events<D>(&self, state: &mut D, block: bool) -> Result<(), io::Error> {
        self.0.dispatch_events(state, block)
    }

    /// Performs a roundtrip against the server.
    ///
    /// This sends a roundtrip request and blocks until the matching
    /// acknowledgment is received, dispatching events into `state` while
    /// waiting.
    ///
    /// # Errors
    /// Returns an error if the connection closes or dispatching protocol
    /// traffic fails while waiting for the roundtrip acknowledgment.
    pub fn roundtrip<D>(&self, state: &mut D) -> Result<(), io::Error> {
        self.0.roundtrip(state)
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
        self.0.handshake_done.get()
    }

    /// Binds a server-advertised protocol and returns its typed root object.
    ///
    /// The provided `spec` must come from [`Client::get_spec`]. `version`
    /// selects the protocol version to bind and must not exceed the version
    /// advertised by the server for that spec.
    ///
    /// # Errors
    /// Returns an error if the requested version is invalid, the connection
    /// closes during binding, or the server does not complete object creation
    /// successfully.
    pub fn bind<T: crate::Object, D: crate::Dispatch<T>>(
        &self,
        spec: &dyn implementation::types::ProtocolSpec,
        version: u32,
        state: &mut D,
    ) -> Result<T, io::Error> {
        let obj = self.0.bind_protocol(spec, version)?;
        let raw_obj: rc::Rc<dyn implementation::object::RawObject> = obj.clone();
        let typed = T::from_object::<D>(raw_obj);
        self.0.wait_for_object(&obj, state)?;
        Ok(typed)
    }

    #[must_use]
    /// Returns the server-advertised protocol specification with the given
    /// name, if present.
    pub fn get_spec(&self, name: &str) -> Option<server_spec::ServerSpec> {
        self.0.get_spec(name)
    }

    #[must_use]
    /// Returns the raw object associated with a pending or resolved sequence
    /// number.
    ///
    /// This is a low-level helper primarily used by generated code and manual
    /// protocol integrations.
    pub fn object_for_seq(
        &self,
        seq: u32,
    ) -> Option<rc::Rc<dyn implementation::object::RawObject>> {
        self.0
            .object_for_seq(seq)
            .map(|obj| obj as rc::Rc<dyn implementation::object::RawObject>)
    }

    #[must_use]
    /// Returns the raw object with the given wire object id, if known.
    ///
    /// This is a low-level helper primarily used by generated code and manual
    /// protocol integrations.
    pub fn object_for_id(&self, id: u32) -> Option<rc::Rc<dyn implementation::object::RawObject>> {
        self.0
            .object_for_id(id)
            .map(|obj| obj as rc::Rc<dyn implementation::object::RawObject>)
    }
}
