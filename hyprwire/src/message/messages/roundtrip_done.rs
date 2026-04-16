use crate::implementation::types;
use crate::message;

#[derive(Debug)]
pub struct RoundtripDone {
    seq: u32,
    data: [u8; 7],
}

impl RoundtripDone {
    pub fn new(seq: u32) -> Self {
        let mut data = [0u8; 7];

        data[0] = message::MessageType::RoundtripDone as u8;
        data[1] = types::MessageMagic::TypeUint as u8;
        data[2..2 + 4].copy_from_slice(&seq.to_le_bytes());
        data[6] = types::MessageMagic::End as u8;

        Self { seq, data }
    }

    pub fn seq(&self) -> u32 {
        self.seq
    }

    pub fn from_bytes(data: &[u8], offset: usize) -> super::Result<Self> {
        if *data.get(offset).ok_or(message::Error::UnexpectedEof)?
            != message::MessageType::RoundtripDone as u8
        {
            return Err(message::Error::InvalidMessageType);
        }

        if *data.get(offset + 1).ok_or(message::Error::UnexpectedEof)?
            != types::MessageMagic::TypeUint as u8
        {
            return Err(message::Error::InvalidFieldType);
        }

        let bytes: [u8; 4] = data
            .get(offset + 2..offset + 2 + size_of::<u32>())
            .ok_or(message::Error::UnexpectedEof)?
            .try_into()
            .unwrap();
        let seq = u32::from_le_bytes(bytes);

        if *data.get(offset + 6).ok_or(message::Error::UnexpectedEof)?
            != types::MessageMagic::End as u8
        {
            return Err(message::Error::MalformedMessage);
        }

        Ok(Self {
            seq,
            data: data[offset..offset + 7].try_into().unwrap(),
        })
    }
}

impl message::Message for RoundtripDone {
    fn data(&self) -> &[u8] {
        &self.data
    }

    fn message_type(&self) -> message::MessageType {
        message::MessageType::RoundtripDone
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use message::Message;

    #[test]
    fn roundtrip_request_new() {
        let msg = RoundtripDone::new(2);
        let parsed = msg.parse_data();
        assert_eq!(parsed, "RoundtripDone ( 2 ) ");
    }

    #[test]
    fn roundtrip_request_from_bytes() {
        let bytes: &[u8] = &[
            message::MessageType::RoundtripDone as u8,
            types::MessageMagic::TypeUint as u8,
            0x2A,
            0x00,
            0x00,
            0x00,
            types::MessageMagic::End as u8,
        ];
        let msg = RoundtripDone::from_bytes(bytes, 0).unwrap();
        let parsed = msg.parse_data();
        assert_eq!(parsed, "RoundtripDone ( 42 ) ");
    }

    #[test]
    fn roundtrip_request_from_bytes_unexpected_eof() {
        let bytes: &[u8] = &[
            message::MessageType::RoundtripDone as u8,
            types::MessageMagic::TypeUint as u8,
        ];
        let err = RoundtripDone::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, message::Error::UnexpectedEof));
    }

    #[test]
    fn roundtrip_request_from_bytes_malformed() {
        let bytes: &[u8] = &[
            message::MessageType::RoundtripDone as u8,
            types::MessageMagic::TypeUint as u8,
            0x2A,
            0x00,
            0x00,
            0x00,
            types::MessageMagic::TypeUint as u8,
        ];
        let err = RoundtripDone::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, message::Error::MalformedMessage));
    }

    #[test]
    fn roundtrip_done_roundtrip_parses_seq() {
        let out = RoundtripDone::new(888);
        let in_msg = RoundtripDone::from_bytes(out.data(), 0).unwrap();
        assert_eq!(in_msg.data(), out.data());
        assert_eq!(in_msg.seq(), 888);
    }

    #[test]
    fn roundtrip_done_rejects_wrong_field_type() {
        // varchar instead of uint
        let bytes: &[u8] = &[
            message::MessageType::RoundtripDone as u8,
            types::MessageMagic::TypeVarchar as u8,
            0x01,
            b'x',
            types::MessageMagic::End as u8,
        ];
        let err = RoundtripDone::from_bytes(bytes, 0).unwrap_err();
        assert!(matches!(err, message::Error::InvalidFieldType));
    }
}
