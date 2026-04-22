use super::client_object;
use crate::client::server_spec;
use crate::implementation::wire_object::WireObject;
use crate::{implementation, socket, steady_millis, trace};
use hyprwire_core::message;
use hyprwire_core::message::Message;
use hyprwire_core::message::wire::{
    bind_protocol, generic_protocol_message, hello, roundtrip_request,
};
use hyprwire_core::types::ProtocolSpec;
use nix::{errno, poll, sys};
use std::os::fd;
use std::os::fd::{AsFd, AsRawFd};
use std::os::unix::net;
use std::{cell, io, ops, path, rc, time};

pub struct ClientSocket {
    impls: cell::RefCell<Vec<Box<dyn implementation::client::ProtocolImplementations>>>,
    server_specs: cell::RefCell<Vec<server_spec::AdvertisedSpec>>,
    objects: cell::RefCell<Vec<rc::Rc<client_object::ClientObject>>>,
    handshake_begin: time::Instant,
    pub(crate) state: rc::Rc<crate::SharedState>,
    pub(crate) handshake_done: cell::Cell<bool>,
    pub(crate) last_ackd_roundtrip_seq: cell::Cell<u32>,
    last_sent_roundtrip_seq: cell::Cell<u32>,
    pub(crate) seq: cell::Cell<u32>,
    pub(crate) pending_outgoing:
        cell::RefCell<Vec<generic_protocol_message::GenericProtocolMessage<ops::Range<usize>>>>,
    self_ref: rc::Weak<Self>,
}

const HANDSHAKE_MAX_MS: u64 = 5000;

impl ClientSocket {
    fn new(stream: net::UnixStream) -> rc::Rc<Self> {
        let state = rc::Rc::new(crate::SharedState::new(
            stream,
            rc::Rc::new(cell::RefCell::new(Vec::new())),
        ));
        let client_socket = rc::Rc::new_cyclic(|weak_self| Self {
            last_ackd_roundtrip_seq: cell::Cell::new(0),
            last_sent_roundtrip_seq: cell::Cell::new(0),
            seq: cell::Cell::new(0),
            impls: cell::RefCell::new(Vec::new()),
            server_specs: cell::RefCell::new(Vec::new()),
            state: rc::Rc::clone(&state),
            objects: cell::RefCell::new(Vec::new()),
            handshake_done: cell::Cell::new(false),
            handshake_begin: time::Instant::now(),
            pending_outgoing: cell::RefCell::new(Vec::new()),
            self_ref: weak_self.clone(),
        });
        state.send_message(&hello::Hello::new());

        client_socket
    }

    pub fn open<P>(path: P) -> io::Result<rc::Rc<Self>>
    where
        P: AsRef<path::Path>,
    {
        let stream = net::UnixStream::connect(path)?;
        stream.set_nonblocking(true)?;
        Ok(Self::new(stream))
    }

    pub fn from_fd<F>(fd: F) -> io::Result<rc::Rc<Self>>
    where
        F: Into<fd::OwnedFd>,
    {
        let stream = net::UnixStream::from(fd.into());
        stream.set_nonblocking(true)?;
        Ok(Self::new(stream))
    }

    pub fn add_implementation(
        &self,
        p_impl: Box<dyn implementation::client::ProtocolImplementations>,
    ) {
        self.impls.borrow_mut().push(p_impl);
    }

    pub fn wait_for_handshake<D>(&self, dispatch: &mut D) -> Result<(), io::Error> {
        while !self.state.error.get() && !self.handshake_done.get() {
            self.dispatch_events(dispatch, true)?;
        }

        if self.state.error.get() {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "handshake failed",
            ));
        }

        Ok(())
    }

    pub fn get_spec(&self, name: &str) -> Option<server_spec::AdvertisedSpec> {
        self.server_specs
            .borrow()
            .iter()
            .find(|spec| spec.name() == name)
            .cloned()
    }

    pub fn bind_protocol(
        &self,
        spec: &dyn ProtocolSpec,
        version: u32,
    ) -> Result<rc::Rc<client_object::ClientObject>, io::Error> {
        if version > spec.spec_ver() {
            crate::log_error!(
                "version {} is larger than current spec ver of {}",
                version,
                spec.spec_ver()
            );
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "version {} exceeds spec version {}",
                    version,
                    spec.spec_ver()
                ),
            ));
        }

        let mut object =
            client_object::ClientObject::new(self.self_ref.clone(), rc::Rc::clone(&self.state));
        let objects = spec.objects();
        if objects.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "protocol {} does not expose object metadata; bind requires a generated protocol spec",
                    spec.spec_name()
                ),
            ));
        }
        object.spec = Some(std::sync::Arc::clone(&objects[0]));
        let seq = self.seq.get() + 1;
        self.seq.set(seq);
        object.seq = seq;
        object.version.set(version);
        object.protocol_name = spec.spec_name().to_string();

        let object = rc::Rc::new(object);
        self.objects.borrow_mut().push(rc::Rc::clone(&object));

        let bind_message = bind_protocol::BindProtocol::new(spec.spec_name(), seq, version);
        self.state.send_message(&bind_message);

        Ok(object)
    }

    pub(crate) fn wait_for_object<D>(
        &self,
        object: &rc::Rc<client_object::ClientObject>,
        dispatch: &mut D,
    ) -> Result<(), io::Error> {
        while object.id.get() == 0 && !self.state.error.get() {
            self.dispatch_events(dispatch, true)?;
        }

        if self.state.error.get() {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "connection error while waiting for object",
            ));
        }

        Ok(())
    }

    pub fn make_object(
        &self,
        protocol_name: &str,
        object_name: &str,
        seq: u32,
    ) -> Result<rc::Rc<client_object::ClientObject>, message::Error> {
        let mut object =
            client_object::ClientObject::new(self.self_ref.clone(), rc::Rc::clone(&self.state));
        object.protocol_name = protocol_name.to_string();

        if let Some(obj) = self
            .impls
            .borrow()
            .iter()
            .find(|imp| imp.protocol().spec_name() == protocol_name)
            .and_then(|imp| {
                imp.protocol()
                    .objects()
                    .iter()
                    .find(|obj| obj.object_name() == object_name)
            })
        {
            object.spec = Some(std::sync::Arc::clone(obj));
        }

        if object.spec.is_none() {
            return Err(message::Error::NoSpec);
        }

        object.seq = seq;
        object.set_version(0); // TODO: client version doesn't matter that much, but for verification's sake we could fix this

        let object = rc::Rc::new(object);
        self.objects.borrow_mut().push(rc::Rc::clone(&object));
        Ok(object)
    }

    pub fn extract_loop_fd(&self) -> fd::BorrowedFd<'_> {
        self.state.stream.as_fd()
    }

    pub fn server_specs(&self, specs: &[Box<str>]) {
        let mut server_specs = self.server_specs.borrow_mut();
        for spec in specs {
            let at_pos = spec.rfind('@').unwrap();

            let s = server_spec::AdvertisedSpec::new(
                spec[0..at_pos].to_string(),
                spec[at_pos + 1..].parse().unwrap(),
            );
            server_specs.push(s);
        }
    }

    pub fn disconnect_on_error(&self) {
        self.state.error.set(true);
        let _ = self.state.stream.shutdown(std::net::Shutdown::Both);
    }

    pub fn roundtrip<D>(&self, dispatch: &mut D) -> Result<(), io::Error> {
        if self.state.error.get() {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "connection closed",
            ));
        }

        let next_seq = self.last_sent_roundtrip_seq.get() + 1;
        self.last_sent_roundtrip_seq.set(next_seq);
        self.state
            .send_message(&roundtrip_request::RoundtripRequest::new(next_seq));

        while self.last_ackd_roundtrip_seq.get() < next_seq {
            self.dispatch_events(dispatch, true)?;
        }

        Ok(())
    }

    pub fn dispatch_events<D>(&self, dispatch: &mut D, block: bool) -> Result<(), io::Error> {
        if self.state.error.get() {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "connection closed",
            ));
        }

        self.collect_orphaned_objects();

        if !self.handshake_done.get() {
            #[allow(clippy::cast_possible_truncation)]
            let elapsed_ms = self.handshake_begin.elapsed().as_millis() as u64;
            let max_ms = HANDSHAKE_MAX_MS.saturating_sub(elapsed_ms);

            let timeout = if block {
                #[allow(clippy::cast_possible_truncation)]
                let max_ms_i32 = max_ms as i32;
                poll::PollTimeout::try_from(max_ms_i32).unwrap_or(poll::PollTimeout::ZERO)
            } else {
                poll::PollTimeout::ZERO
            };

            let mut pfd = [poll::PollFd::new(
                self.state.stream.as_fd(),
                poll::PollFlags::POLLIN,
            )];

            let ready = poll::poll(&mut pfd, timeout)
                .map_err(|e| io::Error::from_raw_os_error(e as i32))?;

            if ready == 0 {
                if block {
                    self.disconnect_on_error();
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "handshake timed out",
                    ));
                }
                return Ok(());
            }

            // peek to check for HUP (0 bytes = connection closed)
            let mut peek_buf = [0u8; 1];
            match sys::socket::recv(
                self.state.stream.as_raw_fd(),
                &mut peek_buf,
                sys::socket::MsgFlags::MSG_PEEK,
            ) {
                Ok(0) => {
                    if block {
                        return Err(io::Error::new(
                            io::ErrorKind::ConnectionAborted,
                            "connection closed",
                        ));
                    }
                    return Ok(());
                }
                Err(e) => return Err(io::Error::from_raw_os_error(e as i32)),
                Ok(_) => {}
            }
        }

        if self.handshake_done.get() {
            let timeout = if block {
                poll::PollTimeout::NONE
            } else {
                poll::PollTimeout::ZERO
            };

            let mut pfd = [poll::PollFd::new(
                self.state.stream.as_fd(),
                poll::PollFlags::POLLIN,
            )];

            let ready = poll::poll(&mut pfd, timeout)
                .map_err(|e| io::Error::from_raw_os_error(e as i32))?;

            if ready == 0 {
                if block {
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        "connection closed",
                    ));
                }
                self.collect_orphaned_objects();
                return Ok(());
            }

            // peek to check for HUP (0 bytes = connection closed)
            let mut peek_buf = [0u8; 1];
            match sys::socket::recv(
                self.state.stream.as_raw_fd(),
                &mut peek_buf,
                sys::socket::MsgFlags::MSG_PEEK,
            ) {
                Ok(0) => {
                    if block {
                        return Err(io::Error::new(
                            io::ErrorKind::ConnectionAborted,
                            "connection closed",
                        ));
                    }
                    return Ok(());
                }
                Err(errno::Errno::ECONNRESET) => {
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        "connection closed",
                    ));
                }
                Err(e) => return Err(io::Error::from_raw_os_error(e as i32)),
                Ok(_) => {}
            }
        }

        // dispatch

        let mut data = {
            match socket::SocketRawParsedMessage::read_from_socket(&self.state.stream) {
                Err(_) => {
                    crate::log_error!("fatal: received malformed message from server");
                    self.disconnect_on_error();
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "received malformed message from server",
                    ));
                }
                Ok(data) => data,
            }
        };

        if data.data.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "connection closed",
            ));
        }

        if crate::message::handle_message(&mut data, &crate::message::Role::Client(self), dispatch)
            .is_err()
        {
            crate::log_error!("fatal: failed to handle message on wire");
            self.disconnect_on_error();
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "failed to handle message",
            ));
        }

        let pending = std::mem::take(&mut *self.pending_outgoing.borrow_mut());
        for mut msg in pending {
            let seq = msg.depends_on_seq();
            let obj_id = self.object_for_seq(seq).map(|obj| obj.id.get());

            match obj_id {
                None => continue,
                Some(0) => {
                    self.pending_outgoing.borrow_mut().push(msg);
                    continue;
                }
                Some(id) => {
                    msg.resolve_seq(id);
                    trace! {
                        crate::log_debug!("[hw] trace: [{} @ {:.3}] -> Handle deferred {}", self.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                    }
                }
            }

            self.state.send_message(&msg);
        }

        self.collect_orphaned_objects();

        if self.state.error.get() {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "connection closed",
            ));
        }

        Ok(())
    }

    pub fn on_seq(&self, seq: u32, id: u32) {
        let objects = self.objects.borrow();
        if let Some(object) = objects.iter().find(|object| object.seq == seq) {
            object.id.set(id);
        }
    }

    pub fn destroy_object(&self, id: u32) {
        self.objects.borrow_mut().retain(|obj| obj.id.get() != id);
    }

    pub fn collect_orphaned_objects(&self) {
        self.objects.borrow_mut().retain(|obj| {
            if obj.id.get() == 0 {
                return true;
            }

            let should_remove = rc::Rc::strong_count(obj) <= 1;

            if should_remove {
                trace! {
                    crate::log_debug!("[{} @ {:.3}] -> Cleaning up orphaned object {}", self.state.stream.as_raw_fd(), steady_millis(), obj.id.get())
                }
            }

            !should_remove
        });
    }

    pub fn on_generic<D>(
        &self,
        msg: &generic_protocol_message::GenericProtocolMessage<ops::Range<usize>>,
        dispatch: &mut D,
    ) {
        let obj = self
            .objects
            .borrow()
            .iter()
            .find(|obj| obj.id.get() == msg.object())
            .map(rc::Rc::clone);

        if let Some(obj) = obj {
            if let Err(e) = obj.called(msg.method(), msg.data_span(), msg.fds(), dispatch) {
                crate::log_error!(
                    "[{} @ {:.3}] object {} called method error: {e}",
                    self.state.stream.as_raw_fd(),
                    steady_millis(),
                    msg.object(),
                );
            }
        } else {
            crate::log_error!(
                "[{} @ {:.3}] generic message references unknown object {}",
                self.state.stream.as_raw_fd(),
                steady_millis(),
                msg.object(),
            );
            self.disconnect_on_error();
        }
    }

    pub fn object_for_id(&self, id: u32) -> Option<rc::Rc<client_object::ClientObject>> {
        self.objects
            .borrow()
            .iter()
            .find(|object| object.id.get() == id)
            .map(rc::Rc::clone)
    }

    pub fn object_for_seq(&self, seq: u32) -> Option<rc::Rc<client_object::ClientObject>> {
        self.objects
            .borrow()
            .iter()
            .find(|object| object.seq == seq)
            .map(rc::Rc::clone)
    }
}
