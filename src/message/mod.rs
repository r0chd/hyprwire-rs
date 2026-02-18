mod messages;

use std::fmt;

#[derive(Debug)]
pub enum MessageError {
    UnexpectedEof,
    InvalidMessageType,
    InvalidFieldType,
    InvalidVarInt,
    InvalidProtocolLength,
    InvalidVersion,
    MalformedMessage,
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
            Self::MalformedMessage => write!(f, "malformed message"),
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
