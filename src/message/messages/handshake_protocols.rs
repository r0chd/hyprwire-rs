use super::{Message, MessageError, MessageType};
use crate::implementation::types::MessageMagic;
use crate::message;

#[derive(Debug)]
pub struct HandshakeProtocols {
    protocols: Vec<Vec<u8>>,
    data: Vec<u8>,
}

impl HandshakeProtocols {
    pub fn new(protocols: &[&[u8]]) -> Self {
        let mut data = Vec::new();

        data.push(MessageType::HandshakeProtocols as u8);
        data.push(MessageMagic::TypeArray as u8);
        data.push(MessageMagic::TypeVarchar as u8);

        let mut arr_len_buf = [0u8; 10];
        let var_int = message::encode_var_int(protocols.len(), &mut arr_len_buf);
        data.extend_from_slice(var_int);

        for protocol in protocols {
            let mut str_len_buf = [0u8; 10];
            let var_int = message::encode_var_int(protocol.len(), &mut str_len_buf);
            data.extend_from_slice(var_int);
            data.extend_from_slice(protocol);
        }

        data.push(MessageMagic::End as u8);

        Self {
            protocols: protocols.iter().map(|p| p.to_vec()).collect(),
            data,
        }
    }

    pub fn from_bytes(data: &[u8], offset: usize) -> Result<Self, MessageError> {
        if *data.get(offset).ok_or(MessageError::UnexpectedEof)?
            != MessageType::HandshakeProtocols as u8
        {
            return Err(MessageError::InvalidMessageType);
        }
        if *data.get(offset + 1).ok_or(MessageError::UnexpectedEof)?
            != MessageMagic::TypeArray as u8
        {
            return Err(MessageError::InvalidFieldType);
        }
        if *data.get(offset + 2).ok_or(MessageError::UnexpectedEof)?
            != MessageMagic::TypeVarchar as u8
        {
            return Err(MessageError::InvalidFieldType);
        }

        let mut needle: usize = 3;

        let (count, var_int_len) = message::parse_var_int(data, offset + needle);
        needle += var_int_len;

        let mut protocols = Vec::with_capacity(count);

        for _ in 0..count {
            data.get(offset + needle)
                .ok_or(MessageError::UnexpectedEof)?;

            let (str_len, var_int_len) = message::parse_var_int(data, offset + needle);
            needle += var_int_len;

            let protocol = data
                .get(offset + needle..offset + needle + str_len)
                .ok_or(MessageError::UnexpectedEof)?
                .to_vec();
            protocols.push(protocol);

            needle += str_len;
        }

        if *data
            .get(offset + needle)
            .ok_or(MessageError::UnexpectedEof)?
            != MessageMagic::End as u8
        {
            return Err(MessageError::MalformedMessage);
        }
        needle += 1;

        let message_len = needle;

        Ok(Self {
            protocols,
            data: data[offset..offset + message_len].to_vec(),
        })
    }
}

impl Message for HandshakeProtocols {
    fn get_data(&self) -> &[u8] {
        &self.data
    }

    fn get_message_type(&self) -> MessageType {
        MessageType::HandshakeProtocols
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_protocols_new() {
        let msg = HandshakeProtocols::new(&[b"test@1", b"test@2"]);
        let parsed = msg.parse_data().unwrap();
        assert_eq!(parsed, "HandshakeProtocols ( { \"test@1\", \"test@2\" } ) ");
    }

    #[test]
    fn handshake_protocols_roundtrip() {
        let original = HandshakeProtocols::new(&[b"test@1", b"test@2"]);
        let parsed = HandshakeProtocols::from_bytes(original.get_data(), 0).unwrap();
        assert_eq!(parsed.protocols, original.protocols);
        assert_eq!(parsed.data, original.data);
    }

    #[test]
    fn handshake_protocols_from_bytes_unexpected_eof() {
        let bytes: &[u8] = &[MessageType::HandshakeProtocols as u8];
        let err = HandshakeProtocols::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, MessageError::UnexpectedEof));
    }

    #[test]
    fn handshake_protocols_from_bytes_invalid_type() {
        let bytes: &[u8] = &[
            MessageType::Sup as u8,
            MessageMagic::TypeArray as u8,
            MessageMagic::TypeVarchar as u8,
            0x00,
            MessageMagic::End as u8,
        ];
        let err = HandshakeProtocols::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, MessageError::InvalidMessageType));
    }

    #[test]
    fn handshake_protocols_empty() {
        let msg = HandshakeProtocols::new(&[]);
        let parsed = HandshakeProtocols::from_bytes(msg.get_data(), 0).unwrap();
        assert!(parsed.protocols.is_empty());
    }
}
