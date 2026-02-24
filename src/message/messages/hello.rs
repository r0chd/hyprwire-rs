use super::{Message, MessageError, MessageType, Result};
use crate::implementation::types::MessageMagic;

#[derive(Debug)]
pub struct Hello {
    data: [u8; 7],
}

impl Hello {
    pub fn new() -> Self {
        let data: [u8; 7] = [
            MessageType::Sup as u8,
            MessageMagic::TypeVarchar as u8,
            0x03,
            b'V',
            b'A',
            b'X',
            MessageMagic::End as u8,
        ];

        Self { data }
    }

    pub fn from_bytes(data: &[u8], offset: usize) -> Result<Self> {
        let expected: &[u8] = &[
            MessageType::Sup as u8,
            MessageMagic::TypeVarchar as u8,
            0x03,
            b'V',
            b'A',
            b'X',
            MessageMagic::End as u8,
        ];

        let msg_data = data
            .get(offset..offset + 7)
            .ok_or(MessageError::UnexpectedEof)?;

        if msg_data != expected {
            return Err(MessageError::MalformedMessage);
        }

        Ok(Self {
            data: msg_data.try_into().unwrap(),
        })
    }
}

impl Message for Hello {
    fn data(&self) -> &[u8] {
        &self.data
    }

    fn message_type(&self) -> MessageType {
        MessageType::Sup
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_new() {
        let msg = Hello::new();
        let data = msg.parse_data();
        assert_eq!(data, "Sup ( \"VAX\" ) ");
    }

    #[test]
    fn hello_from_bytes() {
        let bytes: &[u8] = &[
            MessageType::Sup as u8,
            MessageMagic::TypeVarchar as u8,
            0x03,
            b'V',
            b'A',
            b'X',
            MessageMagic::End as u8,
        ];
        let msg = Hello::from_bytes(bytes, 0).unwrap();
        let data = msg.parse_data();
        assert_eq!(data, "Sup ( \"VAX\" ) ");
    }

    #[test]
    fn hello_from_bytes_unexpected_eof() {
        let bytes: &[u8] = &[0x01, 0x20, 0x03];
        let err = Hello::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, MessageError::UnexpectedEof));
    }

    #[test]
    fn hello_from_bytes_malformed() {
        let bytes: &[u8] = &[0x01, 0x20, 0x03, b'A', b'B', b'C', 0x00];
        let err = Hello::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, MessageError::MalformedMessage));
    }
}
