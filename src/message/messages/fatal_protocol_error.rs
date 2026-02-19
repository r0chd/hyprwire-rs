use super::{Message, MessageError, MessageType};
use crate::implementation::types::MessageMagic;
use crate::message;

#[derive(Debug)]
pub struct FatalProtocolError {
    object_id: u32,
    error_id: u32,
    error_msg: String,
    data: Vec<u8>,
}

impl FatalProtocolError {
    pub fn new(object_id: u32, error_id: u32, error_msg: String) -> Self {
        let mut data = Vec::new();
        data.push(MessageType::FatalProtocolError as u8);
        data.push(MessageMagic::TypeUint as u8);
        data.extend_from_slice(&object_id.to_le_bytes());

        data.push(MessageMagic::TypeUint as u8);
        data.extend_from_slice(&error_id.to_le_bytes());

        data.push(MessageMagic::TypeVarchar as u8);
        let mut msg_len_buf = [0u8; 10];
        data.extend_from_slice(message::encode_var_int(error_msg.len(), &mut msg_len_buf));
        data.extend_from_slice(error_msg.as_bytes());

        data.push(MessageMagic::End as u8);

        Self {
            object_id,
            error_id,
            error_msg,
            data,
        }
    }

    pub fn from_bytes(data: &[u8], offset: usize) -> Result<Self, MessageError> {
        if *data.get(offset).ok_or(MessageError::UnexpectedEof)?
            != MessageType::FatalProtocolError as u8
        {
            return Err(MessageError::InvalidMessageType);
        }

        if *data.get(offset + 1).ok_or(MessageError::UnexpectedEof)? != MessageMagic::TypeUint as u8
        {
            return Err(MessageError::InvalidFieldType);
        }

        let object_id = u32::from_le_bytes(
            data.get(offset + 2..offset + 2 + 4)
                .ok_or(MessageError::UnexpectedEof)?
                .try_into()
                .unwrap(),
        );

        if *data.get(offset + 6).ok_or(MessageError::UnexpectedEof)? != MessageMagic::TypeUint as u8
        {
            return Err(MessageError::InvalidFieldType);
        }

        let error_id = u32::from_le_bytes(
            data.get(offset + 7..offset + 11)
                .ok_or(MessageError::UnexpectedEof)?
                .try_into()
                .unwrap(),
        );

        if *data.get(offset + 11).ok_or(MessageError::UnexpectedEof)?
            != MessageMagic::TypeVarchar as u8
        {
            return Err(MessageError::InvalidFieldType);
        }

        let mut needle: usize = 12;

        let (str_len, var_int_len) = message::parse_var_int(data, offset + needle);
        needle += var_int_len;

        let error_msg = std::str::from_utf8(
            data.get(offset + needle..offset + needle + str_len)
                .ok_or(MessageError::UnexpectedEof)?,
        )
        .map_err(|_| MessageError::MalformedMessage)?
        .to_string();
        needle += str_len;

        if *data
            .get(offset + needle)
            .ok_or(MessageError::UnexpectedEof)?
            != MessageMagic::End as u8
        {
            return Err(MessageError::MalformedMessage);
        }
        needle += 1;

        Ok(Self {
            object_id,
            error_id,
            error_msg,
            data: data[offset..offset + needle].to_vec(),
        })
    }
}

impl Message for FatalProtocolError {
    fn get_data(&self) -> &[u8] {
        &self.data
    }

    fn get_message_type(&self) -> MessageType {
        MessageType::FatalProtocolError
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fatal_protocol_error_new() {
        let msg = FatalProtocolError::new(1, 42, "something broke".to_string());
        let parsed = msg.parse_data().unwrap();
        assert_eq!(parsed, "FatalProtocolError ( 1, 42, \"something broke\" ) ");
    }

    #[test]
    fn fatal_protocol_error_roundtrip() {
        let original = FatalProtocolError::new(1, 42, "something broke".to_string());
        let parsed = FatalProtocolError::from_bytes(original.get_data(), 0).unwrap();
        assert_eq!(parsed.object_id, original.object_id);
        assert_eq!(parsed.error_id, original.error_id);
        assert_eq!(parsed.error_msg, original.error_msg);
        assert_eq!(parsed.data, original.data);
    }

    #[test]
    fn fatal_protocol_error_from_bytes_unexpected_eof() {
        let bytes: &[u8] = &[
            MessageType::FatalProtocolError as u8,
            MessageMagic::TypeUint as u8,
        ];
        let err = FatalProtocolError::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, MessageError::UnexpectedEof));
    }

    #[test]
    fn fatal_protocol_error_from_bytes_invalid_type() {
        let bytes: &[u8] = &[MessageType::Sup as u8];
        let err = FatalProtocolError::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, MessageError::InvalidMessageType));
    }
}
