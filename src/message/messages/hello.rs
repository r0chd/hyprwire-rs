use crate::implementation::types::MessageMagic;
use crate::message;

#[derive(Debug)]
pub struct Hello {
    data: [u8; 7],
}

impl Hello {
    pub fn new() -> Self {
        let data: [u8; 7] = [
            message::MessageType::Sup as u8,
            MessageMagic::TypeVarchar as u8,
            0x03,
            b'V',
            b'A',
            b'X',
            MessageMagic::End as u8,
        ];

        Self { data }
    }

    pub fn from_bytes(data: &[u8], offset: usize) -> super::Result<Self> {
        let expected: &[u8] = &[
            message::MessageType::Sup as u8,
            MessageMagic::TypeVarchar as u8,
            0x03,
            b'V',
            b'A',
            b'X',
            MessageMagic::End as u8,
        ];

        let msg_data = data
            .get(offset..offset + 7)
            .ok_or(message::Error::UnexpectedEof)?;

        if msg_data != expected {
            return Err(message::Error::MalformedMessage);
        }

        Ok(Self {
            data: msg_data.try_into().unwrap(),
        })
    }
}

impl message::Message for Hello {
    fn data(&self) -> &[u8] {
        &self.data
    }

    fn message_type(&self) -> message::MessageType {
        message::MessageType::Sup
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use message::Message;

    #[test]
    fn hello_new() {
        let msg = Hello::new();
        let data = msg.parse_data();
        assert_eq!(data, "Sup ( \"VAX\" ) ");
    }

    #[test]
    fn hello_from_bytes() {
        let bytes: &[u8] = &[
            message::MessageType::Sup as u8,
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
        assert!(matches!(err, message::Error::UnexpectedEof));
    }

    #[test]
    fn hello_from_bytes_malformed() {
        let bytes: &[u8] = &[0x01, 0x20, 0x03, b'A', b'B', b'C', 0x00];
        let err = Hello::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, message::Error::MalformedMessage));
    }
}
