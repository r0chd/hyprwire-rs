mod messages;

use crate::client::client_socket;
use crate::server::server_client;
use crate::{socket, steady_millis, trace};
pub(crate) use messages::Message;
pub(crate) use messages::bind_protocol::BindProtocol;
pub(crate) use messages::fatal_protocol_error::FatalProtocolError;
use messages::generic_protocol_message;
pub(crate) use messages::generic_protocol_message::GenericProtocolMessage;
pub(crate) use messages::handshake_ack::HandshakeAck;
use messages::handshake_begin;
pub(crate) use messages::handshake_begin::HandshakeBegin;
use messages::handshake_protocols;
pub(crate) use messages::handshake_protocols::HandshakeProtocols;
pub(crate) use messages::hello::Hello;
pub(crate) use messages::new_object::NewObject;
pub(crate) use messages::roundtrip_done::RoundtripDone;
pub(crate) use messages::roundtrip_request::RoundtripRequest;
use std::os::fd::AsRawFd;
use std::{error, fmt};

#[derive(Debug)]
pub enum Error {
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
    ArrayTooLong,
    HandshakeBegin(handshake_begin::Error),
    HandshakeProtocols(handshake_protocols::Error),
    GenericProtocol(generic_protocol_message::Error),
}

impl fmt::Display for Error {
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
            Self::ArrayTooLong => write!(f, "array length exceeded 10000"),
            Self::HandshakeBegin(e) => write!(f, "handshake_begin: {e}"),
            Self::HandshakeProtocols(e) => write!(f, "handshake_protocols: {e}"),
            Self::GenericProtocol(e) => write!(f, "generic_protocol_error: {e}"),
        }
    }
}

impl error::Error for Error {}

#[repr(u8)]
#[derive(Debug, Copy, Clone)]
pub enum MessageType {
    // 0 is invalid in hyprwire
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
    type Error = Error;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
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
            _ => Err(Error::InvalidMessageType),
        }
    }
}

pub enum Role<'a> {
    Client(&'a client_socket::ClientSocket),
    Server(&'a server_client::ServerClientState),
}

impl<'a> Role<'a> {
    fn state(&self) -> &crate::SharedState {
        match self {
            Self::Client(client) => &client.state,
            Self::Server(client) => &client.state,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Role::Client(_) => "server",
            Role::Server(_) => "client",
        }
    }
}

pub fn handle_message<D>(
    raw: &mut socket::SocketRawParsedMessage,
    role: &Role,
    dispatch: &mut D,
) -> Result<(), Error> {
    let mut needle = 0;
    while needle < raw.data.len() {
        let Ok(message) = MessageType::try_from(raw.data[needle]) else {
            crate::log_error!(
                "server at fd {} core protocol error: invalid message recvd (invalid type code)",
                role.state().stream.as_raw_fd()
            );

            return Err(Error::InvalidMessage);
        };

        needle += match (role, message) {
            (Role::Client(client), MessageType::HandshakeBegin) => {
                let msg = HandshakeBegin::from_bytes(&raw.data, needle).inspect_err(|_| {
                    crate::log_error!(
                        "server at fd {} core protocol error...",
                        client.state.stream.as_raw_fd()
                    );
                })?;

                if !msg.versions().contains(&crate::PROTOCOL_VERSION) {
                    crate::log_error!(
                        "server at fd {} core protocol error: version negotiation failed",
                        client.state.stream.as_raw_fd()
                    );
                    client.state.error.set(true);
                    return Err(Error::VersionNegotiationFailed);
                }

                trace! {
                    crate::log_debug!("[hw] trace: [{} @ {:.3}] -> parse error: {}", client.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client
                    .state
                    .send_message(&HandshakeAck::new(crate::PROTOCOL_VERSION));

                Ok(msg.data().len())
            }
            (Role::Client(client), MessageType::HandshakeProtocols) => {
                let msg = HandshakeProtocols::from_bytes(&raw.data, needle).inspect_err(|_| {
                    crate::log_error!(
                        "server at fd {} core protocol error: malformed message recvd (HandshakeProtocols)",
                        client.state.stream.as_raw_fd()
                    );
                })?;

                trace! {
                    crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", client.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.server_specs(msg.protocols());
                client.handshake_done.set(true);

                Ok(msg.data().len())
            }
            (Role::Client(client), MessageType::NewObject) => {
                let msg = NewObject::from_bytes(&raw.data, needle).inspect_err(|_| {
                    crate::log_error!(
                        "server at fd {} core protocol error: malformed message recvd (NewObject)",
                        client.state.stream.as_raw_fd()
                    );
                })?;

                trace! {
                    crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", client.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.on_seq(msg.seq(), msg.id());

                Ok(msg.data().len())
            }
            (Role::Client(client), MessageType::GenericProtocolMessage) => {
                let msg = GenericProtocolMessage::from_bytes(&raw.data, &mut raw.fds, needle)
                    .inspect_err(|_| {
                        crate::log_error!(
                        "server at fd {} core protocol error: malformed message recvd (GenericProtocolMessage)",
                        client.state.stream.as_raw_fd()
                    );
                    })?;

                trace! {
                    crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", client.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                let msg_len = msg.data().len();
                client.on_generic(&msg, dispatch);

                Ok(msg_len)
            }
            (Role::Client(client), MessageType::FatalProtocolError) => {
                let msg = FatalProtocolError::from_bytes(&raw.data, needle)
                    .inspect_err(|_| {
                        crate::log_error!(
                        "server at fd {} core protocol error: malformed message recvd (FatalProtocolError)",
                        client.state.stream.as_raw_fd()
                    );
                    })?;

                crate::log_error!(
                    "fatal protocol error: object {} error {}: {}",
                    msg.object_id(),
                    msg.error_id(),
                    msg.error_msg()
                );
                client.state.error.set(true);

                Ok(msg.data().len())
            }
            (Role::Client(client), MessageType::RoundtripDone) => {
                let msg = RoundtripDone::from_bytes(&raw.data, needle)
                    .inspect_err(|_| {
                        crate::log_error!(
                        "server at fd {} core protocol error: malformed message recvd (RoundtripDone)",
                        client.state.stream.as_raw_fd()
                    );
                    })?;

                trace! {
                    crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", client.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.last_ackd_roundtrip_seq.set(msg.seq());

                Ok(msg.data().len())
            }
            (Role::Server(client), MessageType::Sup) => {
                let msg = Hello::from_bytes(&raw.data, needle).inspect_err(|_| {
                    crate::log_error!(
                        "client at fd {} core protocol error: malformed message recvd (Sup)",
                        client.state.stream.as_raw_fd()
                    );
                })?;

                trace! {
                    crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", client.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.dispatch_first_poll();
                client.state.send_message(&HandshakeBegin::new(&[1]));

                Ok(msg.data().len())
            }
            (Role::Server(client), MessageType::HandshakeAck) => {
                let msg = HandshakeAck::from_bytes(&raw.data, needle).inspect_err(|_| {
                    crate::log_error!(
                        "client at fd {} core protocol error: malformed message recvd (HandshakeAck)",
                        client.state.stream.as_raw_fd()
                    );
                })?;

                trace! {
                    crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", client.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.version.set(msg.version());

                let protocol_names = client
                    .state
                    .impls
                    .borrow()
                    .iter()
                    .map(|imp| {
                        format!(
                            "{}@{}",
                            imp.protocol().spec_name(),
                            imp.protocol().spec_ver()
                        )
                    })
                    .collect::<Vec<_>>();

                client
                    .state
                    .send_message(&HandshakeProtocols::new(&protocol_names));

                Ok(msg.data().len())
            }
            (Role::Server(client), MessageType::BindProtocol) => {
                let msg = BindProtocol::from_bytes(&raw.data, needle).inspect_err(|_| {
                    crate::log_error!(
                        "client at fd {} core protocol error: malformed message recvd (BindProtocol)",
                        client.state.stream.as_raw_fd()
                    );
                })?;

                trace! {
                    crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", client.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.create_object(msg.protocol(), "", msg.version(), msg.seq());

                Ok(msg.data().len())
            }
            (Role::Server(client), MessageType::GenericProtocolMessage) => {
                let msg = GenericProtocolMessage::from_bytes(&raw.data, &mut raw.fds, needle)
                    .inspect_err(|_| {
                        crate::log_error!(
                            "client at fd {} core protocol error: malformed message recvd (GenericProtocolMessage)",
                            client.state.stream.as_raw_fd()
                        );
                    })?;

                trace! {
                    crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", client.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.on_generic(&msg, dispatch);

                Ok(msg.data().len())
            }
            (Role::Server(client), MessageType::RoundtripRequest) => {
                let msg = RoundtripRequest::from_bytes(&raw.data, needle).inspect_err(|_| {
                    crate::log_error!(
                        "client at fd {} core protocol error: malformed message recvd (RoundtripRequest)",
                        client.state.stream.as_raw_fd()
                    );
                })?;

                trace! {
                    crate::log_debug!("[hw] trace: [{} @ {:.3}] <- {}", client.state.stream.as_raw_fd(), steady_millis(), msg.parse_data())
                }

                client.scheduled_roundtrip_seq.set(msg.seq());

                Ok(msg.data().len())
            }
            (
                Role::Client(_),
                MessageType::BindProtocol
                | MessageType::HandshakeAck
                | MessageType::RoundtripRequest
                | MessageType::Sup,
            )
            | (
                Role::Server(_),
                MessageType::NewObject
                | MessageType::HandshakeProtocols
                | MessageType::HandshakeBegin
                | MessageType::FatalProtocolError
                | MessageType::RoundtripDone,
            ) => {
                let state = role.state();
                state.error.set(true);

                crate::log_error!(
                    "{} at fd {} core protocol error: invalid message recvd ({message})",
                    role.label(),
                    state.stream.as_raw_fd()
                );

                Err(Error::InvalidMessage)
            }
        }?;
    }

    if !raw.fds.is_empty() {
        return Err(Error::MalformedMessage);
    }

    trace! {
        crate::log_debug!("[hw] trace: [{} @ {}] -- handleMessage: Finished read", role.state().stream.as_raw_fd(), steady_millis())
    }

    Ok(())
}

pub fn encode_var_int(num: usize, buffer: &mut [u8]) -> &[u8] {
    let mut n = num;
    let mut i = 0;

    loop {
        let chunk = (n & 0x7f) as u8;
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
    if offset >= data.len() {
        return (0, 0);
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_roundtrip_at_boundaries() {
        let values: &[usize] = &[0, 1, 127, 128, 16383, 16384, 2097151, 2097152, 268435455];
        for &value in values {
            let mut buf = [0u8; 10];
            let encoded = encode_var_int(value, &mut buf).to_vec();
            assert!(
                !encoded.is_empty(),
                "encoded should not be empty for value={value}"
            );
            let (decoded, n) = parse_var_int(&encoded, 0);
            assert_eq!(decoded, value, "decoded != original for value={value}");
            assert_eq!(
                n,
                encoded.len(),
                "consumed bytes mismatch for value={value}"
            );
            assert_eq!(
                encoded.last().unwrap() & 0x80,
                0,
                "last byte must have high bit clear for value={value}"
            );
        }
    }

    #[test]
    fn varint_parse_with_offset() {
        let mut buf = [0u8; 10];
        let encoded = encode_var_int(420, &mut buf).to_vec();

        let mut data = vec![0xAA_u8, 0xBB];
        data.extend_from_slice(&encoded);
        data.push(0xCC);

        let (decoded, n) = parse_var_int(&data, 2);
        assert_eq!(decoded, 420);
        assert_eq!(n, encoded.len());
    }

    #[test]
    fn varint_parse_out_of_bounds_returns_zero() {
        let data = vec![1u8, 2, 3];
        assert_eq!(parse_var_int(&data, data.len()), (0, 0));
        assert_eq!(parse_var_int(&data, data.len() + 42), (0, 0));
    }

    #[test]
    fn message_type_known_values_try_from() {
        let known: &[u8] = &[1, 2, 3, 4, 10, 11, 12, 13, 14, 100];
        for &byte in known {
            assert!(
                MessageType::try_from(byte).is_ok(),
                "expected Ok for byte={byte:#04x}"
            );
        }
    }

    #[test]
    fn message_type_unknown_value_fails() {
        assert!(MessageType::try_from(0xFF_u8).is_err());
    }
}
