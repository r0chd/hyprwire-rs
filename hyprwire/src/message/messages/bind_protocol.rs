use crate::implementation::types;
use crate::message;
use std::borrow;

#[derive(Debug)]
pub struct BindProtocol<'a> {
    seq: u32,
    version: u32,
    protocol: &'a str,
    data: borrow::Cow<'a, [u8]>,
}

impl<'a> BindProtocol<'a> {
    pub fn new(protocol: &'a str, seq: u32, version: u32) -> Self {
        let mut data = Vec::new();

        data.push(message::MessageType::BindProtocol as u8);
        data.push(types::MessageMagic::TypeUint as u8);
        data.extend_from_slice(&seq.to_le_bytes());

        data.push(types::MessageMagic::TypeVarchar as u8);
        let mut proto_len_buf = [0u8; 10];
        let proto_len_int = message::encode_var_int(protocol.len(), &mut proto_len_buf);
        data.extend_from_slice(proto_len_int);
        data.extend_from_slice(protocol.as_bytes());

        data.push(types::MessageMagic::TypeUint as u8);
        data.extend_from_slice(&version.to_le_bytes());

        data.push(types::MessageMagic::End as u8);

        Self {
            data: borrow::Cow::Owned(data),
            protocol,
            seq,
            version,
        }
    }

    pub fn seq(&self) -> u32 {
        self.seq
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn protocol(&self) -> &str {
        self.protocol
    }

    pub fn from_bytes(data: &'a [u8], offset: usize) -> super::Result<Self> {
        if *data.get(offset).ok_or(message::Error::UnexpectedEof)?
            != message::MessageType::BindProtocol as u8
        {
            return Err(message::Error::InvalidMessageType);
        }

        let mut needle = offset + 1;

        // seq field
        if *data.get(needle).ok_or(message::Error::UnexpectedEof)?
            != types::MessageMagic::TypeUint as u8
        {
            return Err(message::Error::InvalidFieldType);
        }
        needle += 1;
        let seq = u32::from_le_bytes(
            data.get(needle..needle + 4)
                .ok_or(message::Error::UnexpectedEof)?
                .try_into()
                .unwrap(),
        );
        needle += 4;

        // protocol field (varchar)
        if *data.get(needle).ok_or(message::Error::UnexpectedEof)?
            != types::MessageMagic::TypeVarchar as u8
        {
            return Err(message::Error::InvalidFieldType);
        }
        needle += 1;

        let (protocol_len, var_int_len) = message::parse_var_int(data, needle);
        needle += var_int_len;

        let protocol = std::str::from_utf8(
            data.get(needle..needle + protocol_len)
                .ok_or(message::Error::UnexpectedEof)?,
        )
        .map_err(|_| message::Error::MalformedMessage)?;
        needle += protocol_len;

        // version field
        if *data.get(needle).ok_or(message::Error::UnexpectedEof)?
            != types::MessageMagic::TypeUint as u8
        {
            return Err(message::Error::InvalidFieldType);
        }
        needle += 1;
        let version = u32::from_le_bytes(
            data.get(needle..needle + 4)
                .ok_or(message::Error::UnexpectedEof)?
                .try_into()
                .unwrap(),
        );
        if version == 0 {
            return Err(message::Error::InvalidVersion);
        }
        needle += 4;

        // end marker
        if *data.get(needle).ok_or(message::Error::UnexpectedEof)? != types::MessageMagic::End as u8
        {
            return Err(message::Error::MalformedMessage);
        }
        needle += 1;

        Ok(Self {
            seq,
            protocol,
            version,
            data: borrow::Cow::Borrowed(&data[offset..offset + needle - offset]),
        })
    }
}

impl message::Message for BindProtocol<'_> {
    fn data(&self) -> &[u8] {
        &self.data
    }

    fn message_type(&self) -> message::MessageType {
        message::MessageType::BindProtocol
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use message::Message;

    #[test]
    fn bind_protocol_new() {
        let msg = BindProtocol::new("test@1", 5, 1);
        let parsed = msg.parse_data();
        assert_eq!(parsed, "BindProtocol ( 5, \"test@1\", 1 ) ");
    }

    #[test]
    fn bind_protocol_from_bytes() {
        let bytes: &[u8] = &[
            message::MessageType::BindProtocol as u8,
            types::MessageMagic::TypeUint as u8,
            0x05,
            0x00,
            0x00,
            0x00,
            types::MessageMagic::TypeVarchar as u8,
            0x06, // length
            b't',
            b'e',
            b's',
            b't',
            b'@',
            b'1',
            types::MessageMagic::TypeUint as u8,
            0x01,
            0x00,
            0x00,
            0x00,
            types::MessageMagic::End as u8,
        ];
        let msg = BindProtocol::from_bytes(bytes, 0).unwrap();
        let parsed = msg.parse_data();
        assert_eq!(parsed, "BindProtocol ( 5, \"test@1\", 1 ) ");
    }

    #[test]
    fn bind_protocol_from_bytes_unexpected_eof() {
        let bytes: &[u8] = &[
            message::MessageType::BindProtocol as u8,
            types::MessageMagic::TypeUint as u8,
        ];
        let err = BindProtocol::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, message::Error::UnexpectedEof));
    }

    #[test]
    fn bind_protocol_from_bytes_malformed() {
        let bytes: &[u8] = &[
            message::MessageType::BindProtocol as u8,
            types::MessageMagic::TypeUint as u8,
            0x05,
            0x00,
            0x00,
            0x00,
            types::MessageMagic::TypeVarchar as u8,
            0x06, // length
            b't',
            b'e',
            b's',
            b't',
            b'@',
            b'1',
            types::MessageMagic::TypeUint as u8,
            0x01,
            0x00,
            0x00,
            0x00,
            types::MessageMagic::TypeUint as u8,
        ];
        let err = BindProtocol::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, message::Error::MalformedMessage));
    }

    #[test]
    fn bind_protocol_roundtrip_parses_fields() {
        let out = BindProtocol::new("my_proto", 42, 7);
        let in_msg = BindProtocol::from_bytes(out.data(), 0).unwrap();
        assert_eq!(in_msg.data(), out.data());
        assert_eq!(in_msg.protocol(), "my_proto");
        assert_eq!(in_msg.seq(), 42);
        assert_eq!(in_msg.version(), 7);
    }

    #[test]
    fn bind_protocol_rejects_zero_version() {
        let out = BindProtocol::new("my_proto", 42, 0);
        let err = BindProtocol::from_bytes(out.data(), 0).unwrap_err();
        assert!(matches!(err, message::Error::InvalidVersion));
    }
}
