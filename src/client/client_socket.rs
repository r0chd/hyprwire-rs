use super::client_object;
use crate::client::server_spec;
use crate::implementation::types::ProtocolSpec;
use crate::implementation::wire_object::WireObject;
use crate::message::Message;
use crate::{implementation, message, socket, steady_millis, trace};
use nix::sys;
use nix::{errno, poll};
use std::os::fd;
use std::os::fd::{AsFd, AsRawFd, FromRawFd};
use std::os::unix::net;
use std::{cell, io, ops, path, rc, time};

pub struct ClientSocket {
    pub(crate) stream: net::UnixStream,
    impls: Vec<Box<dyn implementation::client::ProtocolImplementations>>,
    server_specs: Vec<server_spec::ServerSpec>,
    objects: Vec<rc::Rc<cell::RefCell<client_object::ClientObject>>>,
    handshake_begin: time::Instant,
    pub(crate) error: bool,
    pub(crate) handshake_done: bool,
    pub(crate) last_ackd_roundtrip_seq: u32,
    last_sent_roundtrip_seq: u32,
    pub(crate) seq: u32,
    pending_socket_data: Vec<socket::SocketRawParsedMessage>,
    pub(crate) pending_outgoing: Vec<message::GenericProtocolMessage<ops::Range<usize>>>,
    _self: rc::Weak<cell::RefCell<Self>>,
}

const HANDSHAKE_MAX_MS: u64 = 5000;

impl ClientSocket {
    fn new(stream: net::UnixStream) -> rc::Rc<cell::RefCell<Self>> {
        let client_socket = rc::Rc::new_cyclic(|weak_self| {
            cell::RefCell::new(Self {
                last_ackd_roundtrip_seq: 0,
                last_sent_roundtrip_seq: 0,
                seq: 0,
                stream,
                impls: Vec::new(),
                server_specs: Vec::new(),
                error: false,
                objects: Vec::new(),
                handshake_done: false,
                handshake_begin: time::Instant::now(),
                pending_socket_data: Vec::new(),
                pending_outgoing: Vec::new(),
                _self: weak_self.clone(),
            })
        });
        client_socket.borrow().send_message(&message::Hello::new());

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

        while !self.error && !self.handshake_done {
            self.dispatch_events(true)?;
        }

        if self.error {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "handshake failed",
            ));
        }

        Ok(())
    }

    pub fn get_spec(&self, name: &str) -> Option<&server_spec::ServerSpec> {
        self.server_specs
            .iter()
            .find(|spec| spec.spec_name() == name)
    }

    pub fn bind_protocol(
        &mut self,
        spec: &dyn ProtocolSpec,
        version: u32,
    ) -> Result<rc::Rc<cell::RefCell<dyn implementation::object::Object>>, io::Error> {
        if version > spec.spec_ver() {
            log::debug!(
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

        let mut object = client_object::ClientObject::new(self._self.clone());
        let objects = spec.objects();
        // SAFETY: spec reference comes from static protocol definitions that outlive the socket.
        object.spec = Some(unsafe { std::mem::transmute(objects[0]) });
        self.seq += 1;
        object.seq = self.seq;
        object.version = version;
        object.protocol_name = spec.spec_name().to_string();

        let object = rc::Rc::new(cell::RefCell::new(object));
        self.objects.push(rc::Rc::clone(&object));

        let bind_message = message::BindProtocol::new(spec.spec_name(), self.seq, version);
        self.send_message(&bind_message);

        self.wait_for_object(&object)?;

        Ok(object)
    }

    fn wait_for_object(
        &mut self,
        object: &rc::Rc<cell::RefCell<client_object::ClientObject>>,
    ) -> Result<(), io::Error> {
        while object.borrow().id == 0 && !self.error {
            self.dispatch_events(true)?;
        }

        if self.error {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "connection error while waiting for object",
            ));
        }

        Ok(())
    }

    pub fn send_message<T>(&self, message: &T)
    where
        T: Message + ?Sized,
    {
        trace! { log::trace!("[{} @ {:.3}] -> {}", self.stream.as_raw_fd(), steady_millis(), message.parse_data()) };

        let buf = message.data();
        let iov = [io::IoSlice::new(buf)];
        let cmsg = [sys::socket::ControlMessage::ScmRights(message.fds())];
        loop {
            match sys::socket::sendmsg::<()>(
                self.stream.as_raw_fd(),
                &iov,
                &cmsg,
                sys::socket::MsgFlags::empty(),
                None,
            ) {
                Ok(_) => break,
                Err(errno::Errno::EAGAIN) => {
                    let mut pfd = [poll::PollFd::new(
                        self.stream.as_fd(),
                        poll::PollFlags::POLLOUT | poll::PollFlags::POLLWRBAND,
                    )];
                    poll::poll(&mut pfd, poll::PollTimeout::NONE);
                    continue;
                }
                Err(_) => {
                    break;
                }
            }
        }
    }

    pub fn make_object(
        &mut self,
        protocol_name: &str,
        object_name: &str,
        seq: u32,
    ) -> Result<rc::Rc<cell::RefCell<client_object::ClientObject>>, message::MessageError> {
        let mut object = client_object::ClientObject::new(self._self.clone());
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
                    .copied()
            })
        {
            // SAFETY: The spec reference comes from self.impls which outlives self.objects.
            // ClientObject only accesses spec while ClientSocket is alive.
            object.spec = Some(unsafe { std::mem::transmute(obj) });
        }

        if object.spec.is_none() {
            return Err(message::MessageError::NoSpec);
        }

        object.seq = seq;
        object.set_version(0); // TODO: client version doesn't matter that much, but for verification's sake we could fix this

        let object = rc::Rc::new(cell::RefCell::new(object));
        self.objects.push(rc::Rc::clone(&object));
        Ok(object)
    }

    pub fn extract_loop_fd(&self) -> i32 {
        self.stream.as_raw_fd()
    }

    // TODO: Consider using a shared pointer instead
    pub fn server_specs(&mut self, specs: &[String]) {
        for spec in specs.iter() {
            let at_pos = spec.rfind('@').unwrap();

            let s = server_spec::ServerSpec::new(
                spec[0..at_pos].to_string(),
                spec[at_pos + 1..].parse().unwrap(),
            );
            self.server_specs.push(s);
        }
    }

    pub fn disconnect_on_error(&mut self) {
        self.error = true;
        self.stream.shutdown(std::net::Shutdown::Both).unwrap();
    }

    pub fn roundtrip(&mut self) -> Result<(), io::Error> {
        if self.error {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "connection closed",
            ));
        }

        self.last_sent_roundtrip_seq += 1;
        let next_seq = self.last_sent_roundtrip_seq;
        self.send_message(&message::RoundtripRequest::new(next_seq));

        while self.last_ackd_roundtrip_seq < next_seq {
            self.dispatch_events(true)?;
        }

        Ok(())
    }

    pub fn dispatch_events(&mut self, block: bool) -> Result<(), io::Error> {
        if self.error {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "connection closed",
            ));
        }

        if !self.handshake_done {
            let elapsed_ms = self.handshake_begin.elapsed().as_millis() as u64;
            let max_ms = HANDSHAKE_MAX_MS.saturating_sub(elapsed_ms);

            let timeout = if block {
                poll::PollTimeout::try_from(max_ms as i32).unwrap_or(poll::PollTimeout::ZERO)
            } else {
                poll::PollTimeout::ZERO
            };

            let mut pfd = [poll::PollFd::new(
                self.stream.as_fd(),
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
                self.stream.as_raw_fd(),
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

        if self.handshake_done {
            let timeout = if block {
                poll::PollTimeout::NONE
            } else {
                poll::PollTimeout::ZERO
            };

            let mut pfd = [poll::PollFd::new(
                self.stream.as_fd(),
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
                return Ok(());
            }

            // peek to check for HUP (0 bytes = connection closed)
            let mut peek_buf = [0u8; 1];
            match sys::socket::recv(
                self.stream.as_raw_fd(),
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

        let mut data = match socket::SocketRawParsedMessage::read_from_socket(&self.stream) {
            Err(_) => {
                log::debug!("fatal: received malformed message from server");
                self.disconnect_on_error();
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "received malformed message from server",
                ));
            }
            Ok(data) => data,
        };

        if data.data.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "connection closed",
            ));
        }

        if message::handle_message(&mut data, message::Role::Client(self)).is_err() {
            log::debug!("fatal: failed to handle message on wire");
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
                        log::debug!("[{} @ {:.3}] -> Handle deferred {}", self.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                    }
                }
            }

            self.send_message(&msg);
        }

        if self.error {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "connection closed",
            ));
        }

        Ok(())
    }

    pub fn on_seq(&mut self, seq: u32, id: u32) {
        if let Some(object) = self
            .objects
            .iter()
            .find(|object| object.borrow().seq == seq)
        {
            object.borrow_mut().id = id;
        }
    }

    pub fn on_generic(&mut self, msg: &message::GenericProtocolMessage<ops::Range<usize>>) {
        if let Some(obj) = self
            .objects
            .iter()
            .find(|obj| obj.borrow().id == msg.object())
        {
            obj.borrow_mut()
                .called(msg.method(), msg.data_span(), msg.fds());
        } else {
            log::debug!(
                "[{} @ {:.3}] -> Generic message not handled. No object with id {}!",
                self.stream.as_raw_fd(),
                steady_millis(),
                msg.object(),
            );
        }
    }

    pub fn object_for_id(
        &self,
        id: u32,
    ) -> Option<rc::Rc<cell::RefCell<client_object::ClientObject>>> {
        self.objects
            .iter()
            .find(|object| object.borrow().id == id)
            .map(rc::Rc::clone)
    }

    pub fn object_for_seq(
        &self,
        seq: u32,
    ) -> Option<rc::Rc<cell::RefCell<client_object::ClientObject>>> {
        self.objects
            .iter()
            .find(|object| object.borrow().seq == seq)
            .map(rc::Rc::clone)
    }
}
