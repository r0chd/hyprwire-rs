use super::{Message, MessageError, MessageType, Result};
use crate::implementation::types::MessageMagic;
use crate::message;
use std::borrow;

#[derive(Debug)]
pub struct HandshakeBegin<'a> {
    versions: Vec<u32>,
    data: borrow::Cow<'a, [u8]>,
}

impl<'a> HandshakeBegin<'a> {
    pub fn new(versions: &[u32]) -> Self {
        let mut data = Vec::new();

        data.push(MessageType::HandshakeBegin as u8);
        data.push(MessageMagic::TypeArray as u8);
        data.push(MessageMagic::TypeUint as u8);

        let mut var_int_buffer = [0u8; 10];
        let var_int = message::encode_var_int(versions.len(), &mut var_int_buffer);
        for int in var_int {
            data.push(*int);
        }

        for version in versions {
            data.extend_from_slice(&version.to_le_bytes());
        }

        data.push(MessageMagic::End as u8);

        Self {
            versions: versions.to_vec(),
            data: borrow::Cow::Owned(data),
        }
    }

    pub fn versions(&self) -> &[u32] {
        &self.versions
    }

    pub fn from_bytes(data: &'a [u8], offset: usize) -> Result<Self> {
        if *data.get(offset).ok_or(MessageError::UnexpectedEof)?
            != MessageType::HandshakeBegin as u8
        {
            return Err(MessageError::InvalidMessageType);
        }

        let mut needle = offset + 1;

        if *data.get(needle).ok_or(MessageError::UnexpectedEof)? != MessageMagic::TypeArray as u8 {
            return Err(MessageError::InvalidFieldType);
        }

        needle += 1;

        if *data.get(needle).ok_or(MessageError::UnexpectedEof)? != MessageMagic::TypeUint as u8 {
            return Err(MessageError::InvalidFieldType);
        }

        needle += 1;

        let (arr_len, var_int_len) = message::parse_var_int(data, needle);
        needle += var_int_len;

        let versions = (0..arr_len)
            .map(|i| {
                let bytes: [u8; 4] = data
                    .get(needle + (i * 4)..needle + (i * 4) + 4)
                    .ok_or(MessageError::UnexpectedEof)?
                    .try_into()
                    .unwrap();
                Ok(u32::from_le_bytes(bytes))
            })
            .collect::<Result<Vec<_>>>()?;

        needle += arr_len * 4;

        if *data.get(needle).ok_or(MessageError::UnexpectedEof)? != MessageMagic::End as u8 {
            return Err(MessageError::MalformedMessage);
        }
        needle += 1;

        let message_len = needle - offset;

        Ok(Self {
            versions,
            data: borrow::Cow::Borrowed(&data[offset..offset + message_len]),
        })
    }
}

impl Message for HandshakeBegin<'_> {
    fn data(&self) -> &[u8] {
        &self.data
    }

    fn message_type(&self) -> MessageType {
        MessageType::HandshakeBegin
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_begin_new() {
        let msg = HandshakeBegin::new(&[1, 2]);
        let parsed = msg.parse_data();
        assert_eq!(parsed, "HandshakeBegin ( { 1, 2 } ) ");
    }

    #[test]
    fn handshake_begin_from_bytes() {
        let bytes: &[u8] = &[
            MessageType::HandshakeBegin as u8,
            MessageMagic::TypeArray as u8,
            MessageMagic::TypeUint as u8,
            0x02, // length
            0x01,
            0x00,
            0x00,
            0x00, // version = 1
            0x02,
            0x00,
            0x00,
            0x00, // version = 2
            MessageMagic::End as u8,
        ];
        let msg = HandshakeBegin::from_bytes(bytes, 0).unwrap();
        let parsed = msg.parse_data();
        assert_eq!(parsed, "HandshakeBegin ( { 1, 2 } ) ");
    }

    #[test]
    fn handshake_begin_from_bytes_unexpected_eof() {
        let bytes: &[u8] = &[MessageType::HandshakeBegin as u8];
        let err = HandshakeBegin::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, MessageError::UnexpectedEof));
    }

    #[test]
    fn roundtrip_request_from_bytes_malformed() {
        let bytes: &[u8] = &[
            MessageType::HandshakeBegin as u8,
            MessageMagic::TypeArray as u8,
            MessageMagic::TypeUint as u8,
            0x02, // length
            0x01,
            0x00,
            0x00,
            0x00, // version = 1
            0x02,
            0x00,
            0x00,
            0x00, // version = 2
            MessageMagic::TypeUint as u8,
        ];
        let err = HandshakeBegin::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, MessageError::MalformedMessage));
    }
}
