extern crate alloc;

use crate::{message, types};
use alloc::string::ToString;
use alloc::{borrow, str, vec};

#[derive(Debug)]
pub struct FatalProtocolError<'a> {
    object_id: u32,
    error_id: u32,
    error_msg: borrow::Cow<'a, str>,
    data: borrow::Cow<'a, [u8]>,
}

impl<'a> FatalProtocolError<'a> {
    pub fn new(object_id: u32, error_id: u32, error_msg: &'a str) -> Self {
        let mut data = vec::Vec::new();
        data.push(message::MessageType::FatalProtocolError as u8);
        data.push(types::MessageMagic::TypeUint as u8);
        data.extend_from_slice(&object_id.to_le_bytes());

        data.push(types::MessageMagic::TypeUint as u8);
        data.extend_from_slice(&error_id.to_le_bytes());

        data.push(types::MessageMagic::TypeVarchar as u8);
        let mut msg_len_buf = [0u8; 10];
        data.extend_from_slice(message::encode_var_int(error_msg.len(), &mut msg_len_buf));
        data.extend_from_slice(error_msg.as_bytes());

        data.push(types::MessageMagic::End as u8);

        Self {
            object_id,
            error_id,
            error_msg: borrow::Cow::Borrowed(error_msg),
            data: borrow::Cow::Owned(data),
        }
    }

    pub fn object_id(&self) -> u32 {
        self.object_id
    }

    pub fn error_id(&self) -> u32 {
        self.error_id
    }

    pub fn error_msg(&self) -> &str {
        &self.error_msg
    }

    pub fn from_bytes(data: &'a [u8], offset: usize) -> super::Result<Self> {
        if *data.get(offset).ok_or(message::Error::UnexpectedEof)?
            != message::MessageType::FatalProtocolError as u8
        {
            return Err(message::Error::InvalidMessageType);
        }

        if *data.get(offset + 1).ok_or(message::Error::UnexpectedEof)?
            != types::MessageMagic::TypeUint as u8
        {
            return Err(message::Error::InvalidFieldType);
        }

        let object_id = u32::from_le_bytes(
            data.get(offset + 2..offset + 2 + 4)
                .ok_or(message::Error::UnexpectedEof)?
                .try_into()
                .unwrap(),
        );

        if *data.get(offset + 6).ok_or(message::Error::UnexpectedEof)?
            != types::MessageMagic::TypeUint as u8
        {
            return Err(message::Error::InvalidFieldType);
        }

        let error_id = u32::from_le_bytes(
            data.get(offset + 7..offset + 11)
                .ok_or(message::Error::UnexpectedEof)?
                .try_into()
                .unwrap(),
        );

        if *data.get(offset + 11).ok_or(message::Error::UnexpectedEof)?
            != types::MessageMagic::TypeVarchar as u8
        {
            return Err(message::Error::InvalidFieldType);
        }

        let mut needle: usize = 12;

        let (str_len, var_int_len) = message::parse_var_int(data, offset + needle);
        needle += var_int_len;

        let error_msg = str::from_utf8(
            data.get(offset + needle..offset + needle + str_len)
                .ok_or(message::Error::UnexpectedEof)?,
        )
        .map_err(|_| message::Error::MalformedMessage)?
        .to_string();
        needle += str_len;

        if *data
            .get(offset + needle)
            .ok_or(message::Error::UnexpectedEof)?
            != types::MessageMagic::End as u8
        {
            return Err(message::Error::MalformedMessage);
        }
        needle += 1;

        Ok(Self {
            object_id,
            error_id,
            error_msg: borrow::Cow::Owned(error_msg),
            data: borrow::Cow::Borrowed(&data[offset..offset + needle]),
        })
    }
}

impl message::Message for FatalProtocolError<'_> {
    fn data(&self) -> &[u8] {
        &self.data
    }

    fn message_type(&self) -> message::MessageType {
        message::MessageType::FatalProtocolError
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use message::Message;

    #[test]
    fn fatal_protocol_error_new() {
        let msg = FatalProtocolError::new(1, 42, "something broke");
        let parsed = msg.parse_data();
        assert_eq!(parsed, "FatalProtocolError ( 1, 42, \"something broke\" ) ");
    }

    #[test]
    fn fatal_protocol_error_roundtrip() {
        let original = FatalProtocolError::new(1, 42, "something broke");
        let parsed = FatalProtocolError::from_bytes(original.data(), 0).unwrap();
        assert_eq!(parsed.object_id, original.object_id);
        assert_eq!(parsed.error_id, original.error_id);
        assert_eq!(parsed.error_msg, original.error_msg);
        assert_eq!(parsed.data, original.data);
    }

    #[test]
    fn fatal_protocol_error_from_bytes_unexpected_eof() {
        let bytes: &[u8] = &[
            message::MessageType::FatalProtocolError as u8,
            types::MessageMagic::TypeUint as u8,
        ];
        let err = FatalProtocolError::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, message::Error::UnexpectedEof));
    }

    #[test]
    fn fatal_protocol_error_from_bytes_invalid_type() {
        let bytes: &[u8] = &[message::MessageType::Sup as u8];
        let err = FatalProtocolError::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, message::Error::InvalidMessageType));
    }
}
