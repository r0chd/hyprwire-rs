extern crate alloc;

mod error;
pub mod wire;

use alloc::fmt;

pub use error::Error;
pub use wire::Message;

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
