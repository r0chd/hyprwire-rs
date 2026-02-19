use super::{Message, MessageError, MessageType};
use crate::implementation::types::MessageMagic;

#[derive(Debug)]
pub struct RoundtripRequest {
    seq: u32,
    data: [u8; 7],
}

impl RoundtripRequest {
    pub fn new(seq: u32) -> Self {
        let mut data = [0u8; 7];

        data[0] = MessageType::RoundtripRequest as u8;
        data[1] = MessageMagic::TypeUint as u8;
        data[2..2 + 4].copy_from_slice(&seq.to_le_bytes());
        data[6] = MessageMagic::End as u8;

        Self { seq, data }
    }

    pub fn from_bytes(data: &[u8], offset: usize) -> Result<Self, MessageError> {
        if *data.get(offset).ok_or(MessageError::UnexpectedEof)?
            != MessageType::RoundtripRequest as u8
        {
            return Err(MessageError::InvalidMessageType);
        }

        if *data.get(offset + 1).ok_or(MessageError::UnexpectedEof)? != MessageMagic::TypeUint as u8
        {
            return Err(MessageError::InvalidFieldType);
        }

        let bytes: [u8; 4] = data
            .get(offset + 2..offset + 2 + size_of::<u32>())
            .ok_or(MessageError::UnexpectedEof)?
            .try_into()
            .unwrap();
        let seq = u32::from_le_bytes(bytes);

        if *data.get(offset + 6).ok_or(MessageError::UnexpectedEof)? != MessageMagic::End as u8 {
            return Err(MessageError::MalformedMessage);
        }

        Ok(Self {
            seq,
            data: data[offset..offset + 7].try_into().unwrap(),
        })
    }
}

impl Message for RoundtripRequest {
    fn get_data(&self) -> &[u8] {
        &self.data
    }

    fn get_message_type(&self) -> MessageType {
        MessageType::RoundtripRequest
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_request_new() {
        let msg = RoundtripRequest::new(2);
        let parsed = msg.parse_data().unwrap();
        assert_eq!(parsed, "RoundtripRequest ( 2 ) ");
    }

    #[test]
    fn roundtrip_request_from_bytes() {
        let bytes: &[u8] = &[
            MessageType::RoundtripRequest as u8,
            MessageMagic::TypeUint as u8,
            0x2A,
            0x00,
            0x00,
            0x00,
            MessageMagic::End as u8,
        ];
        let msg = RoundtripRequest::from_bytes(bytes, 0).unwrap();
        let parsed = msg.parse_data().unwrap();
        assert_eq!(parsed, "RoundtripRequest ( 42 ) ");
    }

    #[test]
    fn roundtrip_request_from_bytes_unexpected_eof() {
        let bytes: &[u8] = &[
            MessageType::RoundtripRequest as u8,
            MessageMagic::TypeUint as u8,
        ];
        let err = RoundtripRequest::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, MessageError::UnexpectedEof));
    }

    #[test]
    fn roundtrip_request_from_bytes_malformed() {
        let bytes: &[u8] = &[
            MessageType::RoundtripRequest as u8,
            MessageMagic::TypeUint as u8,
            0x2A,
            0x00,
            0x00,
            0x00,
            MessageMagic::TypeUint as u8,
        ];
        let err = RoundtripRequest::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, MessageError::MalformedMessage));
    }
}
