use super::{Message, MessageType};
use crate::implementation::types::MessageMagic;
use crate::message::MessageError;

#[derive(Debug)]
pub struct Hello<'a> {
    data: &'a [u8],
    message_type: MessageType,
}

impl<'a> Hello<'a> {
    pub fn new() -> Self {
        let data: &[u8] = &[
            MessageType::Sup as u8,
            MessageMagic::TypeVarchar as u8,
            0x03,
            b'V',
            b'A',
            b'X',
            MessageMagic::End as u8,
        ];

        Self {
            data,
            message_type: MessageType::Sup,
        }
    }

    pub fn from_bytes(data: &'a [u8], offset: usize) -> Result<Self, MessageError> {
        if offset + 7 > data.len() {
            return Err(MessageError::UnexpectedEof);
        }

        let expected: &[u8] = &[
            MessageType::Sup as u8,
            MessageMagic::TypeVarchar as u8,
            0x03,
            b'V',
            b'A',
            b'X',
            MessageMagic::End as u8,
        ];

        if data != expected {
            return Err(MessageError::MalformedMessage);
        }

        Ok(Self {
            data,
            message_type: MessageType::Sup,
        })
    }
}

impl<'a> Message for Hello<'a> {
    fn get_data(&self) -> &[u8] {
        self.data
    }

    fn get_message_type(&self) -> MessageType {
        self.message_type
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hello_new() {
        let msg = Hello::new();
        let data = msg.parseData().unwrap();
        assert_eq!(data, "\"VAX\" ) ");
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
        let data = msg.parseData().unwrap();
        assert_eq!(data, "\"VAX\" ) ");
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
