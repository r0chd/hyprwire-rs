mod messages;

use crate::client::client_socket;
use crate::server::server_client;
use crate::{socket, steady_millis, trace};
pub(crate) use messages::bind_protocol::BindProtocol;
pub(crate) use messages::fatal_protocol_error::FatalProtocolError;
pub(crate) use messages::generic_protocol_message::GenericProtocolMessage;
pub(crate) use messages::handshake_ack::HandshakeAck;
pub(crate) use messages::handshake_begin::HandshakeBegin;
pub(crate) use messages::handshake_protocols::HandshakeProtocols;
pub(crate) use messages::hello::Hello;
pub(crate) use messages::new_object::NewObject;
pub(crate) use messages::roundtrip_done::RoundtripDone;
pub(crate) use messages::roundtrip_request::RoundtripRequest;
pub(crate) use messages::Message;
use std::fmt;
use std::os::fd::AsRawFd;
use std::sync::atomic;

#[derive(Debug)]
pub enum MessageError {
    UnexpectedEof,
    InvalidMessageType,
    InvalidFieldType,
    InvalidVarInt,
    InvalidProtocolLength,
    InvalidVersion,
    InvalidMessage,
    InvalidMethod,
    InvalidParameter,
    IncorrectParamIdx,
    ProtocolVersionTooLow,
    DemarshalingFailed,
    Unimplemented,
    VersionNegotiationFailed,
    MalformedMessage,
    NoSpec,
}

impl fmt::Display for MessageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "unexpected end of data"),
            Self::InvalidMessageType => write!(f, "invalid message type"),
            Self::InvalidFieldType => write!(f, "invalid field type"),
            Self::InvalidVarInt => write!(f, "invalid variable-length integer"),
            Self::InvalidProtocolLength => write!(f, "invalid protocol length"),
            Self::InvalidVersion => write!(f, "invalid version"),
            Self::InvalidMessage => write!(f, "invalid message"),
            Self::InvalidMethod => write!(f, "invalid method"),
            Self::InvalidParameter => write!(f, "invalid parameter"),
            Self::IncorrectParamIdx => write!(f, "incorrect param index"),
            Self::ProtocolVersionTooLow => write!(f, "protocol version too low"),
            Self::DemarshalingFailed => write!(f, "demarshaling failed"),
            Self::Unimplemented => write!(f, "unimplemented"),
            Self::VersionNegotiationFailed => write!(f, "version negotiation failed"),
            Self::MalformedMessage => write!(f, "malformed message"),
            Self::NoSpec => write!(f, "no spec found for object"),
        }
    }
}

impl std::error::Error for MessageError {}

#[repr(u8)]
#[derive(Debug, Copy, Clone)]
pub enum MessageType {
    Invalid = 0,
    Sup = 1,
    HandshakeBegin = 2,
    HandshakeAck = 3,
    HandshakeProtocols = 4,
    BindProtocol = 10,
    NewObject = 11,
    FatalProtocolError = 12,
    RoundtripRequest = 13,
    RoundtripDone = 14,
    GenericProtocolMessage = 100,
}

impl fmt::Display for MessageType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let str = match self {
            MessageType::Sup => "Sup",
            MessageType::Invalid => "Invalid",
            MessageType::HandshakeBegin => "HandshakeBegin",
            MessageType::HandshakeAck => "HandshakeAck",
            MessageType::HandshakeProtocols => "HandshakeProtocols",
            MessageType::BindProtocol => "BindProtocol",
            MessageType::NewObject => "NewObject",
            MessageType::FatalProtocolError => "FatalProtocolError",
            MessageType::RoundtripRequest => "RoundtripRequest",
            MessageType::RoundtripDone => "RoundtripDone",
            MessageType::GenericProtocolMessage => "GenericProtocolMessage",
        };

        write!(f, "{str}")
    }
}

impl TryFrom<u8> for MessageType {
    type Error = MessageError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Invalid),
            1 => Ok(Self::Sup),
            2 => Ok(Self::HandshakeBegin),
            3 => Ok(Self::HandshakeAck),
            4 => Ok(Self::HandshakeProtocols),
            10 => Ok(Self::BindProtocol),
            11 => Ok(Self::NewObject),
            12 => Ok(Self::FatalProtocolError),
            13 => Ok(Self::RoundtripRequest),
            14 => Ok(Self::RoundtripDone),
            100 => Ok(Self::GenericProtocolMessage),
            _ => Err(MessageError::InvalidMessageType),
        }
    }
}

pub enum Role<'a> {
    Client(&'a mut client_socket::ClientSocket),
    Server(&'a mut server_client::ServerClient),
}

pub fn handle_message(
    data: &mut socket::SocketRawParsedMessage,
    role: Role,
) -> Result<(), MessageError> {
    match role {
        Role::Client(client) => {
            let mut needle = 0;

            while needle < data.data.len() {
                needle += parse_single_message_client(data, needle, client)?;
            }

            if !data.fds.is_empty() {
                return Err(MessageError::MalformedMessage);
            }

            trace! {
                log::debug!("[{} @ {}] -- handleMessage: Finished read", client.stream.as_raw_fd(), steady_millis())
            }
        }
        Role::Server(client) => {
            let mut needle = 0;

            while needle < data.data.len() {
                needle += parse_single_message_server(data, needle, client)?;
            }

            if !data.fds.is_empty() {
                return Err(MessageError::MalformedMessage);
            }

            trace! {
                log::debug!("[{} @ {}] -- handleMessage: Finished read", client.stream.as_raw_fd(), steady_millis())
            }
        }
    }

    Ok(())
}

fn parse_single_message_client(
    raw: &mut socket::SocketRawParsedMessage,
    off: usize,
    client: &mut client_socket::ClientSocket,
) -> Result<usize, MessageError> {
    if let Ok(message) = MessageType::try_from(raw.data[off]) {
        match message {
            MessageType::HandshakeBegin => {
                let msg = HandshakeBegin::from_bytes(&raw.data, off).inspect_err(|_| {
                    log::error!(
                        "server at fd {:?} core protocol error...",
                        client.stream.as_raw_fd()
                    );
                })?;

                // TODO: make it globally accessible ig
                let protocol_version = 1;

                let mut version_supported = false;
                if msg.versions().contains(&protocol_version) {
                    version_supported = true;
                }

                if !version_supported {
                    log::error!(
                        "server at fd {} core protocol error: version negotiation failed",
                        client.stream.as_raw_fd()
                    );
                    client.state.error.store(true, atomic::Ordering::Relaxed);
                    return Err(MessageError::VersionNegotiationFailed);
                }

                trace! {
                    log::debug!("[{} @ {:.3}] -> parse error: {}", client.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.send_message(&HandshakeAck::new(protocol_version));

                return Ok(msg.data().len());
            }
            MessageType::HandshakeProtocols => {
                let msg = HandshakeProtocols::from_bytes(&raw.data, off).inspect_err(|_| {
                    log::error!(
                        "server at fd {} core protocol error: malformed message recvd (HandshakeProtocols)",
                        client.stream.as_raw_fd()
                    );
                })?;

                trace! {
                    log::debug!("[{} @ {:.3}] <- {}", client.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.server_specs(msg.protocols());
                client.handshake_done = true;

                return Ok(msg.data().len());
            }
            MessageType::NewObject => {
                let msg = NewObject::from_bytes(&raw.data, off).inspect_err(|_| {
                    log::error!(
                        "server at fd {} core protocol error: malformed message recvd (NewObject)",
                        client.stream.as_raw_fd()
                    );
                })?;

                trace! {
                    log::debug!("[{} @ {:.3}] <- {}", client.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.on_seq(msg.seq(), msg.id());

                return Ok(msg.data().len());
            }
            MessageType::GenericProtocolMessage => {
                let msg = GenericProtocolMessage::from_bytes(&raw.data, &mut raw.fds, off)
                    .inspect_err(|_| {
                        log::error!(
                        "server at fd {} core protocol error: malformed message recvd (GenericProtocolMessage)",
                        client.stream.as_raw_fd()
                    );
                    })?;

                trace! {
                    log::debug!("[{} @ {:.3}] <- {}", client.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.on_generic(&msg);

                return Ok(msg.data().len());
            }
            MessageType::FatalProtocolError => {
                let msg = FatalProtocolError::from_bytes(&raw.data, off)
                    .inspect_err(|_| {
                        log::error!(
                        "server at fd {} core protocol error: malformed message recvd (FatalProtocolError)",
                        client.stream.as_raw_fd()
                    );
                    })?;

                log::error!(
                    "fatal protocol error: object {} error {}: {}",
                    msg.object_id(),
                    msg.error_id(),
                    msg.error_msg()
                );
                client.state.error.store(true, atomic::Ordering::Relaxed);

                return Ok(msg.data().len());
            }
            MessageType::RoundtripDone => {
                let msg = RoundtripDone::from_bytes(&raw.data, off)
                    .inspect_err(|_| {
                        log::error!(
                        "server at fd {} core protocol error: malformed message recvd (RoundtripDone)",
                        client.stream.as_raw_fd()
                    );
                    })?;

                trace! {
                    log::debug!("[{} @ {:.3}] <- {}", client.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.last_ackd_roundtrip_seq = msg.seq();

                return Ok(msg.data().len());
            }
            MessageType::BindProtocol
            | MessageType::HandshakeAck
            | MessageType::RoundtripRequest
            | MessageType::Sup => {
                client.state.error.store(true, atomic::Ordering::Relaxed);
                log::error!(
                    "server at fd {} core protocol error: invalid message recvd ({message})",
                    client.stream.as_raw_fd()
                );
                return Err(MessageError::InvalidMessage);
            }
            MessageType::Invalid => {}
        }
    }

    log::error!(
        "server at fd {} core protocol error: invalid message recvd (invalid type code)",
        client.stream.as_raw_fd()
    );

    Err(MessageError::InvalidMessage)
}

fn parse_single_message_server(
    raw: &mut socket::SocketRawParsedMessage,
    off: usize,
    client: &mut server_client::ServerClient,
) -> Result<usize, MessageError> {
    if let Ok(message) = MessageType::try_from(raw.data[off]) {
        match message {
            MessageType::Sup => {
                let msg = Hello::from_bytes(&raw.data, off).inspect_err(|_| {
                    log::error!(
                        "client at fd {} core protocol error: malformed message recvd (Sup)",
                        client.stream.as_raw_fd()
                    );
                })?;

                trace! {
                    log::debug!("[{} @ {:.3}] <- {}", client.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.dispatch_first_poll();
                client.send_message(&HandshakeBegin::new(&[1]));

                return Ok(msg.data().len());
            }
            MessageType::HandshakeBegin => {
                client.state.error.store(true, atomic::Ordering::Relaxed);
                log::error!(
                    "client at fd {} core protocol error: invalid message recvd (HandshakeBegin)",
                    client.stream.as_raw_fd()
                );
                return Err(MessageError::InvalidMessage);
            }
            MessageType::HandshakeAck => {
                let msg = HandshakeAck::from_bytes(&raw.data, off).inspect_err(|_| {
                    log::error!(
                        "client at fd {} core protocol error: malformed message recvd (HandshakeAck)",
                        client.stream.as_raw_fd()
                    );
                })?;

                trace! {
                    log::debug!("[{} @ {:.3}] <- {}", client.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.version = msg.version();

                let protocol_names = client.protocol_names();
                let protocol_refs: Vec<&str> = protocol_names.iter().map(|s| s.as_str()).collect();
                client.send_message(&HandshakeProtocols::new(&protocol_refs));

                return Ok(msg.data().len());
            }
            MessageType::HandshakeProtocols => {
                client.state.error.store(true, atomic::Ordering::Relaxed);
                log::error!(
                    "client at fd {} core protocol error: invalid message recvd (HandshakeProtocols)",
                    client.stream.as_raw_fd()
                );
                return Err(MessageError::InvalidMessage);
            }
            MessageType::BindProtocol => {
                let msg = BindProtocol::from_bytes(&raw.data, off).inspect_err(|_| {
                    log::error!(
                        "client at fd {} core protocol error: malformed message recvd (BindProtocol)",
                        client.stream.as_raw_fd()
                    );
                })?;

                trace! {
                    log::debug!("[{} @ {:.3}] <- {}", client.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.create_object(msg.protocol(), "", msg.version(), msg.seq());

                return Ok(msg.data().len());
            }
            MessageType::NewObject => {
                client.state.error.store(true, atomic::Ordering::Relaxed);
                log::error!(
                    "client at fd {} core protocol error: invalid message recvd (NewObject)",
                    client.stream.as_raw_fd()
                );
                return Err(MessageError::InvalidMessage);
            }
            MessageType::GenericProtocolMessage => {
                let msg = GenericProtocolMessage::from_bytes(&raw.data, &mut raw.fds, off)
                    .inspect_err(|_| {
                        log::error!(
                            "client at fd {} core protocol error: malformed message recvd (GenericProtocolMessage)",
                            client.stream.as_raw_fd()
                        );
                    })?;

                trace! {
                    log::debug!("[{} @ {:.3}] <- {}", client.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.on_generic(&msg);

                return Ok(msg.data().len());
            }
            MessageType::FatalProtocolError => {
                client.state.error.store(true, atomic::Ordering::Relaxed);
                log::error!(
                    "client at fd {} core protocol error: invalid message recvd (FatalProtocolError)",
                    client.stream.as_raw_fd()
                );
                return Err(MessageError::InvalidMessage);
            }
            MessageType::RoundtripRequest => {
                let msg = RoundtripRequest::from_bytes(&raw.data, off).inspect_err(|_| {
                    log::error!(
                        "client at fd {} core protocol error: malformed message recvd (RoundtripRequest)",
                        client.stream.as_raw_fd()
                    );
                })?;

                trace! {
                    log::debug!("[{} @ {:.3}] <- {}", client.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.scheduled_roundtrip_seq = msg.seq();

                return Ok(msg.data().len());
            }
            MessageType::RoundtripDone => {
                client.state.error.store(true, atomic::Ordering::Relaxed);
                log::error!(
                    "client at fd {} core protocol error: invalid message recvd (RoundtripDone)",
                    client.stream.as_raw_fd()
                );
                return Err(MessageError::InvalidMessage);
            }
            MessageType::Invalid => {}
        }
    }

    log::error!(
        "client at fd {} core protocol error: malformed message recvd (invalid type code)",
        client.stream.as_raw_fd()
    );
    client.state.error.store(true, atomic::Ordering::Relaxed);

    Err(MessageError::InvalidMessage)
}

pub fn encode_var_int(num: usize, buffer: &mut [u8]) -> &[u8] {
    let mut n = num;
    let mut i = 0;

    loop {
        let chunk = (n & 0x7F) as u8;
        n >>= 7;
        buffer[i] = if n == 0 { chunk } else { chunk | 0x80 };
        i += 1;
        if n == 0 {
            break;
        }
    }

    &buffer[..i]
}

pub fn parse_var_int(data: &[u8], offset: usize) -> (usize, usize) {
    parse_var_int_span(&data[offset..])
}

fn parse_var_int_span(data: &[u8]) -> (usize, usize) {
    let mut rolling: usize = 0;
    let mut i: usize = 0;
    let len = data.len();

    while i < len {
        let byte = data[i];

        // Take lower 7 bits and shift into place
        rolling += ((byte & 0x7F) as usize) << (i * 7);

        i += 1;

        // If high bit is not set, we're done
        if (byte & 0x80) == 0 {
            break;
        }
    }

    (rolling, i)
}
