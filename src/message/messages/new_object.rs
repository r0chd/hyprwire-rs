use super::{Message, MessageError, MessageType, Result};
use crate::implementation::types::MessageMagic;

#[derive(Debug)]
pub struct NewObject {
    id: u32,
    seq: u32,
    data: [u8; 12],
}

impl NewObject {
    pub fn new(seq: u32, id: u32) -> Self {
        let mut data = [0u8; 12];

        data[0] = MessageType::NewObject as u8;
        data[1] = MessageMagic::TypeUint as u8;
        data[2..2 + 4].copy_from_slice(&id.to_le_bytes());
        data[7] = MessageMagic::TypeUint as u8;
        data[8..8 + 4].copy_from_slice(&seq.to_le_bytes());
        data[11] = MessageMagic::End as u8;

        Self { id, seq, data }
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn seq(&self) -> u32 {
        self.seq
    }

    pub fn from_bytes(data: &[u8], offset: usize) -> Result<Self> {
        if *data.get(offset).ok_or(MessageError::UnexpectedEof)? != MessageType::NewObject as u8 {
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
        let id = u32::from_le_bytes(bytes);

        if *data.get(offset + 6).ok_or(MessageError::UnexpectedEof)? != MessageMagic::TypeUint as u8
        {
            return Err(MessageError::InvalidFieldType);
        }

        let bytes: [u8; 4] = data
            .get(offset + 7..offset + 7 + size_of::<u32>())
            .ok_or(MessageError::UnexpectedEof)?
            .try_into()
            .unwrap();
        let seq = u32::from_le_bytes(bytes);

        if *data.get(offset + 11).ok_or(MessageError::UnexpectedEof)? != MessageMagic::End as u8 {
            return Err(MessageError::MalformedMessage);
        }

        Ok(Self {
            id,
            seq,
            data: data[offset..offset + 12].try_into().unwrap(),
        })
    }
}

impl Message for NewObject {
    fn data(&self) -> &[u8] {
        &self.data
    }

    fn message_type(&self) -> MessageType {
        MessageType::NewObject
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_object_new() {
        let msg = NewObject::new(3, 2);
        let parsed = msg.parse_data();
        assert_eq!(parsed, "NewObject ( 2, 3 ) ");
    }

    #[test]
    fn new_object_from_bytes() {
        let bytes: &[u8] = &[
            MessageType::NewObject as u8,
            MessageMagic::TypeUint as u8,
            0x02,
            0x00,
            0x00,
            0x00,
            MessageMagic::TypeUint as u8,
            0x03,
            0x00,
            0x00,
            0x00,
            MessageMagic::End as u8,
        ];
        let msg = NewObject::from_bytes(bytes, 0).unwrap();
        let parsed = msg.parse_data();
        assert_eq!(parsed, "NewObject ( 2, 3 ) ");
    }

    #[test]
    fn new_object_from_bytes_unexpected_eof() {
        let bytes: &[u8] = &[MessageType::NewObject as u8, MessageMagic::TypeUint as u8];
        let err = NewObject::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, MessageError::UnexpectedEof));
    }

    #[test]
    fn new_object_from_bytes_malformed() {
        let bytes: &[u8] = &[
            MessageType::NewObject as u8,
            MessageMagic::TypeUint as u8,
            0x02,
            0x00,
            0x00,
            0x00,
            MessageMagic::TypeUint as u8,
            0x03,
            0x00,
            0x00,
            0x00,
            MessageMagic::TypeUint as u8,
        ];
        let err = NewObject::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, MessageError::MalformedMessage));
    }
}
