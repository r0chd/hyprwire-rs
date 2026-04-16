use crate::implementation::types;
use crate::message;
use std::{borrow, error, fmt};

#[derive(Debug)]
pub enum Error {
    TooManyVersions,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooManyVersions => write!(f, "up to 256 handshake versions allowed"),
        }
    }
}

impl error::Error for Error {}

#[derive(Debug)]
pub struct HandshakeBegin<'a> {
    versions: Vec<u32>,
    data: borrow::Cow<'a, [u8]>,
}

impl<'a> HandshakeBegin<'a> {
    pub fn new(versions: &[u32]) -> Self {
        let mut data = Vec::new();

        data.push(message::MessageType::HandshakeBegin as u8);
        data.push(types::MessageMagic::TypeArray as u8);
        data.push(types::MessageMagic::TypeUint as u8);

        let mut var_int_buffer = [0u8; 10];
        let var_int = message::encode_var_int(versions.len(), &mut var_int_buffer);
        for int in var_int {
            data.push(*int);
        }

        for version in versions {
            data.extend_from_slice(&version.to_le_bytes());
        }

        data.push(types::MessageMagic::End as u8);

        Self {
            versions: versions.to_vec(),
            data: borrow::Cow::Owned(data),
        }
    }

    pub fn versions(&self) -> &[u32] {
        &self.versions
    }

    pub fn from_bytes(data: &'a [u8], offset: usize) -> super::Result<Self> {
        if *data.get(offset).ok_or(message::Error::UnexpectedEof)?
            != message::MessageType::HandshakeBegin as u8
        {
            return Err(message::Error::InvalidMessageType);
        }

        let mut needle = offset + 1;

        if *data.get(needle).ok_or(message::Error::UnexpectedEof)?
            != types::MessageMagic::TypeArray as u8
        {
            return Err(message::Error::InvalidFieldType);
        }

        needle += 1;

        if *data.get(needle).ok_or(message::Error::UnexpectedEof)?
            != types::MessageMagic::TypeUint as u8
        {
            return Err(message::Error::InvalidFieldType);
        }

        needle += 1;

        let (n_vars, var_int_len) = message::parse_var_int(data, needle);
        needle += var_int_len;

        // Limit the amount of versions to 256, doesn't make sense otherwise.
        if n_vars >= 256 {
            return Err(message::Error::HandshakeBegin(Error::TooManyVersions));
        }

        let versions = (0..n_vars)
            .map(|i| {
                let bytes: [u8; 4] = data
                    .get(needle + (i * 4)..needle + (i * 4) + 4)
                    .ok_or(message::Error::UnexpectedEof)?
                    .try_into()
                    .unwrap();
                Ok(u32::from_le_bytes(bytes))
            })
            .collect::<super::Result<Vec<_>>>()?;

        needle += n_vars * 4;

        if *data.get(needle).ok_or(message::Error::UnexpectedEof)? != types::MessageMagic::End as u8
        {
            return Err(message::Error::MalformedMessage);
        }
        needle += 1;

        let message_len = needle - offset;

        Ok(Self {
            versions,
            data: borrow::Cow::Borrowed(&data[offset..offset + message_len]),
        })
    }
}

impl super::Message for HandshakeBegin<'_> {
    fn data(&self) -> &[u8] {
        &self.data
    }

    fn message_type(&self) -> message::MessageType {
        message::MessageType::HandshakeBegin
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use message::Message;

    #[test]
    fn handshake_begin_new() {
        let msg = HandshakeBegin::new(&[1, 2]);
        let parsed = msg.parse_data();
        assert_eq!(parsed, "HandshakeBegin ( { 1, 2 } ) ");
    }

    #[test]
    fn handshake_begin_from_bytes() {
        let bytes: &[u8] = &[
            message::MessageType::HandshakeBegin as u8,
            types::MessageMagic::TypeArray as u8,
            types::MessageMagic::TypeUint as u8,
            0x02, // length
            0x01,
            0x00,
            0x00,
            0x00, // version = 1
            0x02,
            0x00,
            0x00,
            0x00, // version = 2
            types::MessageMagic::End as u8,
        ];
        let msg = HandshakeBegin::from_bytes(bytes, 0).unwrap();
        let parsed = msg.parse_data();
        assert_eq!(parsed, "HandshakeBegin ( { 1, 2 } ) ");
    }

    #[test]
    fn handshake_begin_from_bytes_unexpected_eof() {
        let bytes: &[u8] = &[message::MessageType::HandshakeBegin as u8];
        let err = HandshakeBegin::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, message::Error::UnexpectedEof));
    }

    #[test]
    fn roundtrip_request_from_bytes_malformed() {
        let bytes: &[u8] = &[
            message::MessageType::HandshakeBegin as u8,
            types::MessageMagic::TypeArray as u8,
            types::MessageMagic::TypeUint as u8,
            0x02, // length
            0x01,
            0x00,
            0x00,
            0x00, // version = 1
            0x02,
            0x00,
            0x00,
            0x00, // version = 2
            types::MessageMagic::TypeUint as u8,
        ];
        let err = HandshakeBegin::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, message::Error::MalformedMessage));
    }

    #[test]
    fn handshake_begin_roundtrip_parses_versions() {
        let versions = [1u32, 2, 255];
        let out = HandshakeBegin::new(&versions);
        let in_msg = HandshakeBegin::from_bytes(out.data(), 0).unwrap();
        assert_eq!(in_msg.data(), out.data());
        assert_eq!(in_msg.versions(), &versions);
    }

    #[test]
    fn handshake_begin_rejects_wrong_array_element_type() {
        // element type is TypeVarchar instead of TypeUint
        let bytes: &[u8] = &[
            message::MessageType::HandshakeBegin as u8,
            types::MessageMagic::TypeArray as u8,
            types::MessageMagic::TypeVarchar as u8,
            0x00,
            types::MessageMagic::End as u8,
        ];
        let err = HandshakeBegin::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, message::Error::InvalidFieldType));
    }

    #[test]
    fn handshake_begin_rejects_too_many_versions() {
        let mut data = vec![
            message::MessageType::HandshakeBegin as u8,
            types::MessageMagic::TypeArray as u8,
            types::MessageMagic::TypeUint as u8,
        ];
        let mut buf = [0u8; 10];
        let var_int = message::encode_var_int(256, &mut buf);
        data.extend_from_slice(var_int);

        let err = HandshakeBegin::from_bytes(&data, 0).unwrap_err();
        assert!(matches!(err, message::Error::HandshakeBegin(_)));
    }
}
