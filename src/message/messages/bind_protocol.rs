use super::{Message, MessageError, MessageType};
use crate::implementation::types::MessageMagic;
use crate::message;

#[derive(Debug)]
pub struct BindProtocol<'a> {
    seq: u32,
    version: u32,
    protocol: &'a str,
    data: Vec<u8>,
}

impl<'a> BindProtocol<'a> {
    pub fn new(protocol: &'a str, seq: u32, version: u32) -> Self {
        let mut data = Vec::new();

        data.push(MessageType::BindProtocol as u8);
        data.push(MessageMagic::TypeUint as u8);
        data.extend_from_slice(&seq.to_le_bytes());

        data.push(MessageMagic::TypeVarchar as u8);
        let mut proto_len_buf = [0u8; 10];
        let proto_len_int = message::encode_var_int(protocol.len(), &mut proto_len_buf);
        data.extend_from_slice(proto_len_int);
        data.extend_from_slice(protocol.as_bytes());

        data.push(MessageMagic::TypeUint as u8);
        data.extend_from_slice(&version.to_le_bytes());

        data.push(MessageMagic::End as u8);

        Self {
            data,
            protocol,
            seq,
            version,
        }
    }

    pub fn from_bytes(data: &'a [u8], offset: usize) -> Result<Self, MessageError> {
        if *data.get(offset).ok_or(MessageError::UnexpectedEof)? != MessageType::BindProtocol as u8
        {
            return Err(MessageError::InvalidMessageType);
        }

        let mut needle = offset + 1;

        // seq field
        if *data.get(needle).ok_or(MessageError::UnexpectedEof)? != MessageMagic::TypeUint as u8 {
            return Err(MessageError::InvalidFieldType);
        }
        needle += 1;
        let seq = u32::from_le_bytes(
            data.get(needle..needle + 4)
                .ok_or(MessageError::UnexpectedEof)?
                .try_into()
                .unwrap(),
        );
        needle += 4;

        // protocol field (varchar)
        if *data.get(needle).ok_or(MessageError::UnexpectedEof)? != MessageMagic::TypeVarchar as u8
        {
            return Err(MessageError::InvalidFieldType);
        }
        needle += 1;

        let (protocol_len, var_int_len) = message::parse_var_int(data, needle);
        needle += var_int_len;

        let protocol = std::str::from_utf8(
            data.get(needle..needle + protocol_len)
                .ok_or(MessageError::UnexpectedEof)?,
        )
        .map_err(|_| MessageError::MalformedMessage)?;
        needle += protocol_len;

        // version field
        if *data.get(needle).ok_or(MessageError::UnexpectedEof)? != MessageMagic::TypeUint as u8 {
            return Err(MessageError::InvalidFieldType);
        }
        needle += 1;
        let version = u32::from_le_bytes(
            data.get(needle..needle + 4)
                .ok_or(MessageError::UnexpectedEof)?
                .try_into()
                .unwrap(),
        );
        if version == 0 {
            return Err(MessageError::InvalidVersion);
        }
        needle += 4;

        // end marker
        if *data.get(needle).ok_or(MessageError::UnexpectedEof)? != MessageMagic::End as u8 {
            return Err(MessageError::MalformedMessage);
        }
        needle += 1;

        Ok(Self {
            seq,
            protocol,
            version,
            data: data[offset..offset + needle - offset].to_vec(),
        })
    }
}

impl Message for BindProtocol<'_> {
    fn get_data(&self) -> &[u8] {
        &self.data
    }

    fn get_message_type(&self) -> MessageType {
        MessageType::BindProtocol
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_protocol_new() {
        let msg = BindProtocol::new("test@1", 5, 1);
        let parsed = msg.parse_data().unwrap();
        assert_eq!(parsed, "BindProtocol ( 5, \"test@1\", 1 ) ");
    }

    #[test]
    fn bind_protocol_from_bytes() {
        let bytes: &[u8] = &[
            MessageType::BindProtocol as u8,
            MessageMagic::TypeUint as u8,
            0x05,
            0x00,
            0x00,
            0x00,
            MessageMagic::TypeVarchar as u8,
            0x06, // length
            b't',
            b'e',
            b's',
            b't',
            b'@',
            b'1',
            MessageMagic::TypeUint as u8,
            0x01,
            0x00,
            0x00,
            0x00,
            MessageMagic::End as u8,
        ];
        let msg = BindProtocol::from_bytes(bytes, 0).unwrap();
        let parsed = msg.parse_data().unwrap();
        assert_eq!(parsed, "BindProtocol ( 5, \"test@1\", 1 ) ");
    }

    #[test]
    fn bind_protocol_from_bytes_unexpected_eof() {
        let bytes: &[u8] = &[
            MessageType::BindProtocol as u8,
            MessageMagic::TypeUint as u8,
        ];
        let err = BindProtocol::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, MessageError::UnexpectedEof));
    }

    #[test]
    fn bind_protocol_from_bytes_malformed() {
        let bytes: &[u8] = &[
            MessageType::BindProtocol as u8,
            MessageMagic::TypeUint as u8,
            0x05,
            0x00,
            0x00,
            0x00,
            MessageMagic::TypeVarchar as u8,
            0x06, // length
            b't',
            b'e',
            b's',
            b't',
            b'@',
            b'1',
            MessageMagic::TypeUint as u8,
            0x01,
            0x00,
            0x00,
            0x00,
            MessageMagic::TypeUint as u8,
        ];
        let err = BindProtocol::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, MessageError::MalformedMessage));
    }
}
