use super::client_object;
use crate::client::server_spec;
use crate::implementation::wire_object::WireObject;
use crate::message::Message;
use crate::{implementation, message, socket, steady_millis, trace};
use nix::sys;
use nix::{errno, poll};
use std::os::fd;
use std::os::fd::{AsFd, AsRawFd, FromRawFd};
use std::os::unix::net;
use std::{io, ops, path, time};

pub enum SocketSource<'a> {
    Path(&'a path::Path),
    Fd(fd::RawFd),
}

pub struct ClientSocket<'a> {
    pub(crate) stream: net::UnixStream,
    impls: Vec<Box<dyn implementation::client::ProtocolImplementations>>,
    server_specs: Vec<server_spec::ServerSpec>,
    objects: Vec<client_object::ClientObject<'a>>,
    handshake_begin: time::Instant,
    pub(crate) error: bool,
    handshake_done: bool,
    pub(crate) last_ackd_roundtrip_seq: u32,
    last_sent_roundtrip_seq: u32,
    seq: u32,
    pending_socket_data: Vec<socket::SocketRawParsedMessage>,
    pending_outgoing: Vec<message::GenericProtocolMessage<'a, ops::Range<usize>>>,
    waiting_on_object: Option<Box<dyn WireObject>>,
}

const HANDSHAKE_MAX_MS: u64 = 5000;

impl ClientSocket<'_> {
    pub fn open(source: SocketSource) -> Self {
        let stream = match source {
            SocketSource::Path(path) => {
                net::UnixStream::connect(path).expect("Failed to connect to Unix socket")
            }
            SocketSource::Fd(fd) => unsafe { net::UnixStream::from_raw_fd(fd) },
        };

        let client_socket = Self {
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
            waiting_on_object: None,
        };
        client_socket.send_message(&message::Hello::new());

        client_socket
    }

    pub fn add_implementation(
        &mut self,
        p_impl: Box<dyn implementation::client::ProtocolImplementations>,
    ) {
        self.impls.push(p_impl);
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
                    _ = poll::poll(&mut pfd, poll::PollTimeout::NONE);
                    continue;
                }
                Err(_) => {
                    break;
                }
            }
        }
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

        let mut i = self.pending_outgoing.len();
        while i > 0 {
            i -= 1;
            let seq = self.pending_outgoing[i].depends_on_seq();
            let obj_id = self.object_for_seq(seq).map(|obj| obj.id);

            match obj_id {
                None => {
                    _ = self.pending_outgoing.remove(i);
                    continue;
                }
                Some(0) => continue,
                Some(id) => {
                    self.pending_outgoing[i].resolve_seq(id);
                    trace! {
                        log::debug!("[{} @ {:.3}] -> Handle deferred {}", self.stream.as_raw_fd(), steady_millis(), self.pending_outgoing[i].parse_data())
                    }
                }
            }

            self.send_message(&self.pending_outgoing[i]);
            _ = self.pending_outgoing.remove(i);
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
        if let Some(object) = self.objects.iter_mut().find(|object| object.seq == seq) {
            object.id = id;
        }
    }

    pub fn on_generic<R>(&mut self, msg: &message::GenericProtocolMessage<'_, ops::Range<usize>>) {
        if let Some(obj) = self.objects.iter_mut().find(|obj| obj.id == msg.object()) {
            obj.called(msg.method(), msg.data_span(), msg.fds());
        }

        log::debug!(
            "[{} @ {:.3}] -> Generic message not handled. No object with id {}!",
            self.stream.as_raw_fd(),
            steady_millis(),
            msg.object(),
        );
    }

    pub fn object_for_id(&self, id: u32) -> Option<&client_object::ClientObject<'_>> {
        self.objects.iter().find(|object| object.id == id)
    }

    pub fn object_for_seq(&self, seq: u32) -> Option<&client_object::ClientObject<'_>> {
        self.objects.iter().find(|object| object.seq == seq)
    }
}
