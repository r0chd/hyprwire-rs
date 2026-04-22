pub mod wire;

use std::{error, fmt};
pub use wire::Message;
use wire::{generic_protocol_message, handshake_begin, handshake_protocols};

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
    // Invalid = 0,
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

pub fn encode_var_int(num: usize, buffer: &mut [u8]) -> &[u8] {
    let mut n = num;
    let mut i = 0;

    loop {
        let Ok(chunk) = u8::try_from(n & 0x7F) else {
            continue;
        };
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
