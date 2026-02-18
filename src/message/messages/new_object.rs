use super::{Message, MessageType};
use crate::implementation::types::MessageMagic;
use crate::message::MessageError;

#[derive(Debug)]
pub struct NewObject {
    id: u32,
    seq: u32,
    data: Vec<u8>,
    message_type: MessageType,
}

impl NewObject {
    pub fn new(seq: u32, id: u32) -> Self {
        let mut data: Vec<u8> = Vec::new();

        data.push(MessageType::NewObject as u8);
        data.push(MessageMagic::TypeUint as u8);
        data.append(&mut id.to_be_bytes().to_vec());
        data.push(MessageMagic::TypeUint as u8);
        data.append(&mut seq.to_be_bytes().to_vec());
        data.push(MessageMagic::End as u8);

        Self {
            id,
            seq,
            data,
            message_type: MessageType::NewObject,
        }
    }

    pub fn from_bytes(data: &[u8], offset: usize) -> Result<Self, MessageError> {
        if offset + 12 > data.len() {
            return Err(MessageError::UnexpectedEof);
        }

        if data[offset] != MessageType::NewObject as u8 {
            return Err(MessageError::InvalidMessageType);
        }

        if data[offset + 1] != MessageMagic::TypeUint as u8 {
            return Err(MessageError::InvalidFieldType);
        }

        let bytes: [u8; 4] = data
            .get(offset + 2..offset + 2 + size_of::<u32>())
            .ok_or(MessageError::UnexpectedEof)?
            .try_into()
            .unwrap();
        let id = u32::from_be_bytes(bytes);

        if data[offset + 6] != MessageMagic::TypeUint as u8 {
            return Err(MessageError::InvalidFieldType);
        }

        let bytes: [u8; 4] = data
            .get(offset + 7..offset + 7 + size_of::<u32>())
            .ok_or(MessageError::UnexpectedEof)?
            .try_into()
            .unwrap();
        let seq = u32::from_be_bytes(bytes);

        if data[offset + 11] != MessageMagic::End as u8 {
            return Err(MessageError::MalformedMessage);
        }

        Ok(Self {
            id,
            seq,
            data: data[offset..12].to_vec(),
            message_type: MessageType::NewObject,
        })
    }
}
