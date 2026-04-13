use crate::implementation::types;
use crate::message;
use std::{borrow, error, fmt};

#[derive(Debug)]
pub enum Error {
    TooManyProtocols,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyProtocols => write!(f, "up to 2048 protocols allowed per connection"),
        }
    }
}

impl error::Error for Error {}

#[derive(Debug)]
pub struct HandshakeProtocols<'a> {
    protocols: Vec<Box<str>>,
    data: borrow::Cow<'a, [u8]>,
}

impl<'a> HandshakeProtocols<'a> {
    pub fn new<T>(protocols: &[T]) -> Self
    where
        T: AsRef<str>,
    {
        let mut data = Vec::new();

        data.push(message::MessageType::HandshakeProtocols as u8);
        data.push(types::MessageMagic::TypeArray as u8);
        data.push(types::MessageMagic::TypeVarchar as u8);

        let mut arr_len_buf = [0u8; 10];
        let var_int = message::encode_var_int(protocols.len(), &mut arr_len_buf);
        data.extend_from_slice(var_int);

        for protocol in protocols {
            let mut str_len_buf = [0u8; 10];
            let var_int = message::encode_var_int(protocol.as_ref().len(), &mut str_len_buf);
            data.extend_from_slice(var_int);
            data.extend_from_slice(protocol.as_ref().as_bytes());
        }

        data.push(types::MessageMagic::End as u8);

        Self {
            protocols: protocols.iter().map(|s| Box::from(s.as_ref())).collect(),
            data: borrow::Cow::Owned(data),
        }
    }

    pub fn protocols(&self) -> &[Box<str>] {
        &self.protocols
    }

    pub fn from_bytes(data: &'a [u8], offset: usize) -> super::Result<Self> {
        if *data.get(offset).ok_or(message::Error::UnexpectedEof)?
            != message::MessageType::HandshakeProtocols as u8
        {
            return Err(message::Error::InvalidMessageType);
        }
        if *data.get(offset + 1).ok_or(message::Error::UnexpectedEof)?
            != types::MessageMagic::TypeArray as u8
        {
            return Err(message::Error::InvalidFieldType);
        }
        if *data.get(offset + 2).ok_or(message::Error::UnexpectedEof)?
            != types::MessageMagic::TypeVarchar as u8
        {
            return Err(message::Error::InvalidFieldType);
        }

        let mut needle: usize = 3;

        let (els, var_int_len) = message::parse_var_int(data, offset + needle);
        needle += var_int_len;

        // max 2048 protocols per connection
        if els >= 2048 {
            return Err(message::Error::HandshakeProtocols(Error::TooManyProtocols));
        }

        let mut protocols = Vec::with_capacity(els);

        for _ in 0..els {
            data.get(offset + needle)
                .ok_or(message::Error::UnexpectedEof)?;

            let (str_len, var_int_len) = message::parse_var_int(data, offset + needle);
            needle += var_int_len;

            let protocol: Box<str> = std::str::from_utf8(
                data.get(offset + needle..offset + needle + str_len)
                    .ok_or(message::Error::UnexpectedEof)?,
            )
            .map_err(|_| message::Error::MalformedMessage)?
            .into();
            protocols.push(protocol);

            needle += str_len;
        }

        if *data
            .get(offset + needle)
            .ok_or(message::Error::UnexpectedEof)?
            != types::MessageMagic::End as u8
        {
            return Err(message::Error::MalformedMessage);
        }
        needle += 1;

        let message_len = needle;

        Ok(Self {
            protocols,
            data: borrow::Cow::Borrowed(&data[offset..offset + message_len]),
        })
    }
}

impl message::Message for HandshakeProtocols<'_> {
    fn data(&self) -> &[u8] {
        &self.data
    }

    fn message_type(&self) -> message::MessageType {
        message::MessageType::HandshakeProtocols
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use message::Message;

    #[test]
    fn handshake_protocols_new() {
        let msg = HandshakeProtocols::new(&["test@1", "test@2"]);
        let parsed = msg.parse_data();
        assert_eq!(parsed, "HandshakeProtocols ( { \"test@1\", \"test@2\" } ) ");
    }

    #[test]
    fn handshake_protocols_roundtrip() {
        let original = HandshakeProtocols::new(&["test@1", "test@2"]);
        let parsed = HandshakeProtocols::from_bytes(original.data(), 0).unwrap();
        assert_eq!(parsed.protocols, original.protocols);
        assert_eq!(parsed.data, original.data);
    }

    #[test]
    fn handshake_protocols_from_bytes_unexpected_eof() {
        let bytes: &[u8] = &[message::MessageType::HandshakeProtocols as u8];
        let err = HandshakeProtocols::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, message::Error::UnexpectedEof));
    }

    #[test]
    fn handshake_protocols_from_bytes_invalid_type() {
        let bytes: &[u8] = &[
            message::MessageType::Sup as u8,
            types::MessageMagic::TypeArray as u8,
            types::MessageMagic::TypeVarchar as u8,
            0x00,
            types::MessageMagic::End as u8,
        ];
        let err = HandshakeProtocols::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, message::Error::InvalidMessageType));
    }

    #[test]
    fn handshake_protocols_empty() {
        let msg = HandshakeProtocols::new::<&str>(&[]);
        let parsed = HandshakeProtocols::from_bytes(msg.data(), 0).unwrap();
        assert!(parsed.protocols.is_empty());
    }
}
