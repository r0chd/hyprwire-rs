use crate::implementation::types;
use crate::message;

#[derive(Debug)]
pub struct HandshakeAck {
    version: u32,
    data: [u8; 7],
}

impl HandshakeAck {
    pub fn new(version: u32) -> Self {
        let mut data = [0u8; 7];

        data[0] = message::MessageType::HandshakeAck as u8;
        data[1] = types::MessageMagic::TypeUint as u8;
        data[2..2 + 4].copy_from_slice(&version.to_le_bytes());
        data[6] = types::MessageMagic::End as u8;

        Self { version, data }
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn from_bytes(data: &[u8], offset: usize) -> super::Result<Self> {
        if *data.get(offset).ok_or(message::Error::UnexpectedEof)?
            != message::MessageType::HandshakeAck as u8
        {
            return Err(message::Error::InvalidMessageType);
        }

        if *data.get(offset + 1).ok_or(message::Error::UnexpectedEof)?
            != types::MessageMagic::TypeUint as u8
        {
            return Err(message::Error::InvalidFieldType);
        }

        let mut needle = 2;

        if *data
            .get(offset + needle + 4)
            .ok_or(message::Error::UnexpectedEof)?
            != types::MessageMagic::End as u8
        {
            return Err(message::Error::MalformedMessage);
        }

        let bytes: [u8; 4] = data
            .get(offset + 2..offset + 6)
            .ok_or(message::Error::UnexpectedEof)?
            .try_into()
            .unwrap();
        let version = u32::from_le_bytes(bytes);

        needle += 4;

        Ok(Self {
            version,
            data: data[offset..=(offset + needle)].try_into().unwrap(),
        })
    }
}

impl message::Message for HandshakeAck {
    fn data(&self) -> &[u8] {
        &self.data
    }

    fn message_type(&self) -> message::MessageType {
        message::MessageType::HandshakeAck
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use message::Message;

    #[test]
    fn handshake_ack_new() {
        let msg = HandshakeAck::new(1);
        let parsed = msg.parse_data();
        assert_eq!(parsed, "HandshakeAck ( 1 ) ");
    }

    #[test]
    fn handshake_ack_from_bytes() {
        let bytes: &[u8] = &[
            message::MessageType::HandshakeAck as u8,
            types::MessageMagic::TypeUint as u8,
            0x01,
            0x00,
            0x00,
            0x00,
            types::MessageMagic::End as u8,
        ];
        let msg = HandshakeAck::from_bytes(bytes, 0).unwrap();
        let parsed = msg.parse_data();
        assert_eq!(parsed, "HandshakeAck ( 1 ) ");
    }

    #[test]
    fn handshake_ack_from_bytes_unexpected_eof() {
        let bytes: &[u8] = &[
            message::MessageType::HandshakeAck as u8,
            types::MessageMagic::TypeUint as u8,
        ];
        let err = HandshakeAck::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, message::Error::UnexpectedEof));
    }

    #[test]
    fn handshake_ack_from_bytes_malformed() {
        let bytes: &[u8] = &[
            message::MessageType::HandshakeAck as u8,
            types::MessageMagic::TypeUint as u8,
            0x2A,
            0x00,
            0x00,
            0x00,
            types::MessageMagic::TypeUint as u8,
        ];
        let err = HandshakeAck::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, message::Error::MalformedMessage));
    }

    #[test]
    fn handshake_ack_roundtrip_parses_version() {
        let out = HandshakeAck::new(7);
        let in_msg = HandshakeAck::from_bytes(out.data(), 0).unwrap();
        assert_eq!(in_msg.data(), out.data());
        assert_eq!(in_msg.version(), 7);
    }
}
