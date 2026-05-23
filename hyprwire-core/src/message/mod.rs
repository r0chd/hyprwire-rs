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

#[cfg(test)]
mod tests {
    use super::*;

    // parse_var_int returns (value, bytes_consumed)

    #[test]
    fn parse_var_int_single_byte() {
        // Any byte 0–127 is a complete varint in 1 byte
        assert_eq!(parse_var_int(&[0x00], 0), (0, 1));
        assert_eq!(parse_var_int(&[0x01], 0), (1, 1));
        assert_eq!(parse_var_int(&[0x7F], 0), (127, 1));
    }

    #[test]
    fn parse_var_int_two_bytes() {
        // 128 = 0x80 encodes as [0x80, 0x01]: high bit set means "more bytes follow"
        assert_eq!(parse_var_int(&[0x80, 0x01], 0), (128, 2));
        // 300 = 0x12C encodes as [0xAC, 0x02]
        assert_eq!(parse_var_int(&[0xAC, 0x02], 0), (300, 2));
    }

    #[test]
    fn parse_var_int_with_offset() {
        // The reported case: data[1] = 2, a single-byte varint for value 2
        let data = [
            19u8, 2, 0, 0, 0, 32, 8, 112, 97, 115, 115, 119, 111, 114, 100, 0,
        ];
        assert_eq!(parse_var_int(&data, 1), (2, 1));
        // offset 0 gives value 19
        assert_eq!(parse_var_int(&data, 0), (19, 1));
    }

    #[test]
    fn parse_var_int_offset_out_of_bounds() {
        let data = [0x01u8];
        assert_eq!(parse_var_int(&data, 1), (0, 0));
        assert_eq!(parse_var_int(&[], 0), (0, 0));
    }

    #[test]
    fn parse_var_int_roundtrip() {
        let cases = [0usize, 1, 127, 128, 300, 16383, 16384, usize::MAX >> 8];
        let mut buf = [0u8; 10];
        for &n in &cases {
            let encoded = encode_var_int(n, &mut buf);
            let (value, consumed) = parse_var_int(encoded, 0);
            assert_eq!(value, n, "roundtrip failed for {n}");
            assert_eq!(consumed, encoded.len(), "consumed != encoded len for {n}");
        }
    }
}
