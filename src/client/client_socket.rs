use super::client_object;
use crate::client::server_spec;
use crate::implementation::types::ProtocolSpec;
use crate::implementation::wire_object::WireObject;
use crate::message::Message;
use crate::{SharedState, implementation, message, socket, steady_millis, trace};
use nix::sys;
use nix::{errno, poll};
use std::os::fd;
use std::os::fd::{BorrowedFd, FromRawFd};
use std::os::unix::net;
use std::{cell, io, ops, path, rc, sync, time};

pub struct ClientSocket {
    impls: Vec<Box<dyn implementation::client::ProtocolImplementations>>,
    server_specs: cell::RefCell<Vec<server_spec::ServerSpec>>,
    objects: cell::RefCell<Vec<rc::Rc<cell::RefCell<client_object::ClientObject>>>>,
    handshake_begin: time::Instant,
    pub(crate) state: rc::Rc<SharedState>,
    pub(crate) handshake_done: cell::Cell<bool>,
    pub(crate) last_ackd_roundtrip_seq: cell::Cell<u32>,
    last_sent_roundtrip_seq: u32,
    pub(crate) seq: u32,
    pub(crate) pending_outgoing: Vec<message::GenericProtocolMessage<ops::Range<usize>>>,
    _self: rc::Weak<cell::RefCell<Self>>,
}

const HANDSHAKE_MAX_MS: u64 = 5000;

impl ClientSocket {
    fn new(stream: net::UnixStream) -> rc::Rc<cell::RefCell<Self>> {
        let state = rc::Rc::new(SharedState::new(stream));
        let client_socket = rc::Rc::new_cyclic(|weak_self| {
            cell::RefCell::new(Self {
                last_ackd_roundtrip_seq: cell::Cell::new(0),
                last_sent_roundtrip_seq: 0,
                seq: 0,
                impls: Vec::new(),
                server_specs: cell::RefCell::new(Vec::new()),
                state: rc::Rc::clone(&state),
                objects: cell::RefCell::new(Vec::new()),
                handshake_done: cell::Cell::new(false),
                handshake_begin: time::Instant::now(),
                pending_outgoing: Vec::new(),
                _self: weak_self.clone(),
            })
        });
        state.send_message(&message::Hello::new());

        client_socket
    }

    pub fn open(path: &path::Path) -> rc::Rc<cell::RefCell<Self>> {
        let stream = net::UnixStream::connect(path).expect("Failed to connect to Unix socket");
        Self::new(stream)
    }

    pub fn from_fd(fd: fd::RawFd) -> rc::Rc<cell::RefCell<Self>> {
        let stream = unsafe { net::UnixStream::from_raw_fd(fd) };
        Self::new(stream)
    }

    pub fn add_implementation(
        &mut self,
        p_impl: Box<dyn implementation::client::ProtocolImplementations>,
    ) {
        self.impls.push(p_impl);
    }

    pub fn wait_for_handshake(&mut self) -> Result<(), io::Error> {
        self.handshake_begin = time::Instant::now();

        while !self.state.error.get() && !self.handshake_done.get() {
            self.dispatch_events(true)?;
        }

        if self.state.error.get() {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "handshake failed",
            ));
        }

        Ok(())
    }

    pub fn get_spec(&self, name: &str) -> Option<server_spec::ServerSpec> {
        self.server_specs
            .borrow()
            .iter()
            .find(|spec| spec.spec_name() == name)
            .cloned()
    }

    pub fn bind_protocol(
        &mut self,
        spec: &dyn ProtocolSpec,
        version: u32,
    ) -> Result<rc::Rc<cell::RefCell<dyn implementation::object::Object>>, io::Error> {
        if version > spec.spec_ver() {
            log::error!(
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
            client_object::ClientObject::new(self._self.clone(), rc::Rc::clone(&self.state));
        let objects = spec.objects();
        object.spec = Some(sync::Arc::clone(&objects[0]));
        self.seq += 1;
        object.seq = self.seq;
        object.version = version;
        object.protocol_name = spec.spec_name().to_string();

        let object = rc::Rc::new(cell::RefCell::new(object));
        self.objects.borrow_mut().push(rc::Rc::clone(&object));

        let bind_message = message::BindProtocol::new(spec.spec_name(), self.seq, version);
        self.state.send_message(&bind_message);

        self.wait_for_object(&object)?;

        Ok(object)
    }

    fn wait_for_object(
        &mut self,
        object: &rc::Rc<cell::RefCell<client_object::ClientObject>>,
    ) -> Result<(), io::Error> {
        while object.borrow().id == 0 && !self.state.error.get() {
            self.dispatch_events(true)?;
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
    ) -> Result<rc::Rc<cell::RefCell<client_object::ClientObject>>, message::MessageError> {
        let mut object =
            client_object::ClientObject::new(self._self.clone(), rc::Rc::clone(&self.state));
        object.protocol_name = protocol_name.to_string();

        if let Some(obj) = self
            .impls
            .iter()
            .find(|imp| imp.protocol().spec_name() == protocol_name)
            .and_then(|imp| {
                imp.protocol()
                    .objects()
                    .iter()
                    .find(|obj| obj.object_name() == object_name)
            })
        {
            object.spec = Some(sync::Arc::clone(obj));
        }

        if object.spec.is_none() {
            return Err(message::MessageError::NoSpec);
        }

        object.seq = seq;
        object.set_version(0); // TODO: client version doesn't matter that much, but for verification's sake we could fix this

        let object = rc::Rc::new(cell::RefCell::new(object));
        self.objects.borrow_mut().push(rc::Rc::clone(&object));
        Ok(object)
    }

    pub fn extract_loop_fd(&self) -> i32 {
        self.state.fd
    }

    pub fn server_specs(&self, specs: &[rc::Rc<str>]) {
        let mut server_specs = self.server_specs.borrow_mut();
        for spec in specs.iter() {
            let at_pos = spec.rfind('@').unwrap();

            let s = server_spec::ServerSpec::new(
                spec[0..at_pos].to_string(),
                spec[at_pos + 1..].parse().unwrap(),
            );
            server_specs.push(s);
        }
    }

    pub fn disconnect_on_error(&self) {
        self.state.error.set(true);
        self.state
            .stream
            .borrow()
            .shutdown(std::net::Shutdown::Both)
            .unwrap();
    }

    pub fn roundtrip(&mut self) -> Result<(), io::Error> {
        if self.state.error.get() {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "connection closed",
            ));
        }

        self.last_sent_roundtrip_seq += 1;
        let next_seq = self.last_sent_roundtrip_seq;
        self.state
            .send_message(&message::RoundtripRequest::new(next_seq));

        while self.last_ackd_roundtrip_seq.get() < next_seq {
            self.dispatch_events(true)?;
        }

        Ok(())
    }

    pub fn dispatch_events(&mut self, block: bool) -> Result<(), io::Error> {
        if self.state.error.get() {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "connection closed",
            ));
        }

        let borrowed_fd = unsafe { BorrowedFd::borrow_raw(self.state.fd) };

        if !self.handshake_done.get() {
            let elapsed_ms = self.handshake_begin.elapsed().as_millis() as u64;
            let max_ms = HANDSHAKE_MAX_MS.saturating_sub(elapsed_ms);

            let timeout = if block {
                poll::PollTimeout::try_from(max_ms as i32).unwrap_or(poll::PollTimeout::ZERO)
            } else {
                poll::PollTimeout::ZERO
            };

            let mut pfd = [poll::PollFd::new(borrowed_fd, poll::PollFlags::POLLIN)];

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
                self.state.fd,
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

            let mut pfd = [poll::PollFd::new(borrowed_fd, poll::PollFlags::POLLIN)];

            let ready = poll::poll(&mut pfd, timeout)
                .map_err(|e| io::Error::from_raw_os_error(e as i32))?;

            if ready == 0 {
                if block {
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        "connection closed",
                    ));
                }
                return Ok(());
            }

            // peek to check for HUP (0 bytes = connection closed)
            let mut peek_buf = [0u8; 1];
            match sys::socket::recv(
                self.state.fd,
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
            let stream = self.state.stream.borrow();
            match socket::SocketRawParsedMessage::read_from_socket(&stream) {
                Err(_) => {
                    drop(stream);
                    log::error!("fatal: received malformed message from server");
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

        if message::handle_message(&mut data, message::Role::Client(self)).is_err() {
            log::error!("fatal: failed to handle message on wire");
            self.disconnect_on_error();
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "failed to handle message",
            ));
        }

        let pending = std::mem::take(&mut self.pending_outgoing);
        for mut msg in pending {
            let seq = msg.depends_on_seq();
            let obj_id = self.object_for_seq(seq).map(|obj| obj.borrow().id);

            match obj_id {
                None => continue,
                Some(0) => {
                    self.pending_outgoing.push(msg);
                    continue;
                }
                Some(id) => {
                    msg.resolve_seq(id);
                    trace! {
                        eprintln!("[hw] trace: [{} @ {:.3}] -> Handle deferred {}", self.state.fd, steady_millis(), msg.parse_data())
                    }
                }
            }

            self.state.send_message(&msg);
        }

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
        if let Some(object) = objects.iter().find(|object| object.borrow().seq == seq) {
            object.borrow_mut().id = id;
        }
    }

    pub fn on_generic(&self, msg: &message::GenericProtocolMessage<ops::Range<usize>>) {
        let obj = self
            .objects
            .borrow()
            .iter()
            .find(|obj| obj.borrow().id == msg.object())
            .map(rc::Rc::clone);

        match obj {
            Some(obj) => {
                if let Err(e) = obj
                    .borrow()
                    .called(msg.method(), msg.data_span(), msg.fds())
                {
                    log::error!(
                        "[{} @ {:.3}] object {} called method error: {e}",
                        self.state.fd,
                        steady_millis(),
                        msg.object(),
                    );
                }
            }
            None => {
                trace! {
                    eprintln!(
                        "[hw] trace: [{} @ {:.3}] -> Generic message not handled. No object with id {}!",
                        self.state.fd,
                        steady_millis(),
                        msg.object(),
                    )
                }
            }
        }
    }

    pub fn object_for_id(
        &self,
        id: u32,
    ) -> Option<rc::Rc<cell::RefCell<client_object::ClientObject>>> {
        self.objects
            .borrow()
            .iter()
            .find(|object| object.borrow().id == id)
            .map(rc::Rc::clone)
    }

    pub fn object_for_seq(
        &self,
        seq: u32,
    ) -> Option<rc::Rc<cell::RefCell<client_object::ClientObject>>> {
        self.objects
            .borrow()
            .iter()
            .find(|object| object.borrow().seq == seq)
            .map(rc::Rc::clone)
    }
}
