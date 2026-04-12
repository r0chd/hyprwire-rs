use super::{Message, MessageError, MessageType, Result};
use crate::implementation::types::MessageMagic;
use crate::message;
use crate::trace;
use std::ops;

#[derive(Debug, Clone)]
pub struct GenericProtocolMessage<R>
where
    R: ops::RangeBounds<usize>,
{
    depends_on_seq: u32,
    object: u32,
    method: u32,
    fds: Vec<i32>,
    data: Vec<u8>,
    range: R,
}

impl GenericProtocolMessage<ops::Range<usize>> {
    pub fn new(data: Vec<u8>, fds: Vec<i32>) -> Self {
        let len = data.len();
        Self {
            depends_on_seq: 0,
            object: 0,
            method: 0,
            fds,
            data,
            range: 0..len,
        }
    }

    pub fn from_bytes(data: &[u8], fds: &mut Vec<i32>, offset: usize) -> Result<Self> {
        let mut fds_cursor = 0;

        if *data.get(offset).ok_or(MessageError::UnexpectedEof)?
            != MessageType::GenericProtocolMessage as u8
        {
            return Err(MessageError::InvalidMessageType);
        }

        if *data.get(offset + 1).ok_or(MessageError::UnexpectedEof)?
            != MessageMagic::TypeObject as u8
        {
            return Err(MessageError::InvalidFieldType);
        }

        let object = u32::from_le_bytes(
            data.get(offset + 2..offset + 6)
                .ok_or(MessageError::UnexpectedEof)?
                .try_into()
                .unwrap(),
        );

        if *data.get(offset + 6).ok_or(MessageError::UnexpectedEof)? != MessageMagic::TypeUint as u8
        {
            return Err(MessageError::InvalidFieldType);
        }

        let method = u32::from_le_bytes(
            data.get(offset + 7..offset + 11)
                .ok_or(MessageError::UnexpectedEof)?
                .try_into()
                .unwrap(),
        );

        let mut consumed_fds = Vec::new();

        let mut i: usize = 11;
        while *data.get(offset + i).ok_or(MessageError::UnexpectedEof)? != MessageMagic::End as u8 {
            let magic =
                MessageMagic::try_from(*data.get(offset + i).ok_or(MessageError::UnexpectedEof)?)?;

            match magic {
                MessageMagic::TypeUint
                | MessageMagic::TypeF32
                | MessageMagic::TypeInt
                | MessageMagic::TypeObject
                | MessageMagic::TypeSeq => {
                    i += 5;
                }
                MessageMagic::TypeVarchar => {
                    let (str_len, var_int_len) = message::parse_var_int(data, offset + i + 1);
                    i += str_len + var_int_len + 1;
                }
                MessageMagic::TypeArray => {
                    let arr_type = MessageMagic::try_from(
                        *data
                            .get(offset + i + 1)
                            .ok_or(MessageError::UnexpectedEof)?,
                    )?;
                    let (arr_len, len_len) = message::parse_var_int(data, offset + i + 2);
                    let mut arr_message_len: usize = 2 + len_len;

                    match arr_type {
                        MessageMagic::TypeUint
                        | MessageMagic::TypeF32
                        | MessageMagic::TypeInt
                        | MessageMagic::TypeObject
                        | MessageMagic::TypeSeq => {
                            arr_message_len += 4 * arr_len;
                        }
                        MessageMagic::TypeVarchar => {
                            for _ in 0..arr_len {
                                let (str_len, str_len_len) =
                                    message::parse_var_int(data, offset + i + arr_message_len);
                                arr_message_len += str_len + str_len_len;
                            }
                        }
                        MessageMagic::TypeFd => {
                            for _ in 0..arr_len {
                                if let Some(fd) = fds.get(fds_cursor) {
                                    consumed_fds.push(*fd);
                                    fds_cursor += 1;
                                } else {
                                    return Err(MessageError::MalformedMessage);
                                }
                            }
                        }
                        _ => {
                            trace! {
                                eprintln!("[hw] trace: GenericProtocolMessage: failed demarshaling array message")
                            }
                            return Err(MessageError::MalformedMessage);
                        }
                    }

                    i += arr_message_len;
                }
                MessageMagic::TypeFd => {
                    if let Some(fd) = fds.get(fds_cursor) {
                        consumed_fds.push(*fd);
                        fds_cursor += 1;
                    } else {
                        trace! {
                            eprintln!("[hw] trace: GenericProtocolMessage: MessageMagic::TypeFd but fd queue is empty")
                        }
                        return Err(MessageError::MalformedMessage);
                    }
                    i += 1;
                }
                _ => {
                    trace! {
                        eprintln!("[hw] trace: GenericProtocolMessage: failed demarshaling array message")
                    }
                    return Err(MessageError::MalformedMessage);
                }
            }
        }

        let len = i + 1; // include the End byte

        if fds_cursor > 0 {
            fds.drain(..fds_cursor);
        }

        Ok(Self {
            depends_on_seq: 0,
            object,
            method,
            fds: consumed_fds,
            data: data[offset..offset + len].to_vec(),
            range: 11..len,
        })
    }

    pub fn data_span(&self) -> &[u8] {
        &self.data[self.range.clone()]
    }
}

impl<R> GenericProtocolMessage<R>
where
    R: ops::RangeBounds<usize>,
{
    pub fn object(&self) -> u32 {
        self.object
    }

    pub fn method(&self) -> u32 {
        self.method
    }

    pub fn depends_on_seq(&self) -> u32 {
        self.depends_on_seq
    }

    pub fn set_depends_on_seq(&mut self, seq: u32) {
        self.depends_on_seq = seq;
    }

    pub fn resolve_seq(&mut self, id: u32) {
        self.object = id;
        if self.data.len() >= 6 {
            self.data[2..6].copy_from_slice(&id.to_le_bytes());
        }
    }
}

impl<R> Message for GenericProtocolMessage<R>
where
    R: ops::RangeBounds<usize>,
{
    fn data(&self) -> &[u8] {
        &self.data
    }

    fn message_type(&self) -> MessageType {
        MessageType::GenericProtocolMessage
    }

    fn fds(&self) -> &[i32] {
        &self.fds
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_bytes_minimal() {
        let bytes = [
            MessageType::GenericProtocolMessage as u8,
            MessageMagic::TypeObject as u8,
            0x01,
            0x00,
            0x00,
            0x00, // object = 1
            MessageMagic::TypeUint as u8,
            0x02,
            0x00,
            0x00,
            0x00, // method = 2
            MessageMagic::End as u8,
        ];
        let mut fds = Vec::new();
        let msg = GenericProtocolMessage::from_bytes(&bytes, &mut fds, 0).unwrap();
        assert_eq!(msg.object, 1);
        assert_eq!(msg.method, 2);
        assert!(msg.fds.is_empty());
    }

    #[test]
    fn from_bytes_with_primitives() {
        let bytes = [
            MessageType::GenericProtocolMessage as u8,
            MessageMagic::TypeObject as u8,
            0x05,
            0x00,
            0x00,
            0x00, // object = 5
            MessageMagic::TypeUint as u8,
            0x03,
            0x00,
            0x00,
            0x00, // method = 3
            // payload fields
            MessageMagic::TypeUint as u8,
            0x0A,
            0x00,
            0x00,
            0x00,
            MessageMagic::TypeInt as u8,
            0xFF,
            0xFF,
            0xFF,
            0xFF,
            MessageMagic::End as u8,
        ];
        let mut fds = Vec::new();
        let msg = GenericProtocolMessage::from_bytes(&bytes, &mut fds, 0).unwrap();
        assert_eq!(msg.object, 5);
        assert_eq!(msg.method, 3);
    }

    #[test]
    fn from_bytes_with_varchar() {
        let bytes = [
            MessageType::GenericProtocolMessage as u8,
            MessageMagic::TypeObject as u8,
            0x01,
            0x00,
            0x00,
            0x00,
            MessageMagic::TypeUint as u8,
            0x01,
            0x00,
            0x00,
            0x00,
            // varchar "hi"
            MessageMagic::TypeVarchar as u8,
            0x02, // varint length = 2
            b'h',
            b'i',
            MessageMagic::End as u8,
        ];
        let mut fds = Vec::new();
        let msg = GenericProtocolMessage::from_bytes(&bytes, &mut fds, 0).unwrap();
        assert_eq!(msg.object, 1);
        assert_eq!(msg.method, 1);
    }

    #[test]
    fn from_bytes_with_fd() {
        let bytes = [
            MessageType::GenericProtocolMessage as u8,
            MessageMagic::TypeObject as u8,
            0x01,
            0x00,
            0x00,
            0x00,
            MessageMagic::TypeUint as u8,
            0x01,
            0x00,
            0x00,
            0x00,
            MessageMagic::TypeFd as u8,
            MessageMagic::End as u8,
        ];
        let mut fds = vec![42];
        let msg = GenericProtocolMessage::from_bytes(&bytes, &mut fds, 0).unwrap();
        assert_eq!(msg.fds, vec![42]);
        assert!(fds.is_empty());
    }

    #[test]
    fn from_bytes_fd_empty_queue() {
        let bytes = [
            MessageType::GenericProtocolMessage as u8,
            MessageMagic::TypeObject as u8,
            0x01,
            0x00,
            0x00,
            0x00,
            MessageMagic::TypeUint as u8,
            0x01,
            0x00,
            0x00,
            0x00,
            MessageMagic::TypeFd as u8,
            MessageMagic::End as u8,
        ];
        let mut fds = Vec::new();
        let err = GenericProtocolMessage::from_bytes(&bytes, &mut fds, 0).unwrap_err();
        assert!(matches!(err, MessageError::MalformedMessage));
    }

    #[test]
    fn from_bytes_with_uint_array() {
        let bytes = [
            MessageType::GenericProtocolMessage as u8,
            MessageMagic::TypeObject as u8,
            0x01,
            0x00,
            0x00,
            0x00,
            MessageMagic::TypeUint as u8,
            0x01,
            0x00,
            0x00,
            0x00,
            // array of 2 uints
            MessageMagic::TypeArray as u8,
            MessageMagic::TypeUint as u8,
            0x02, // varint count = 2
            0x0A,
            0x00,
            0x00,
            0x00,
            0x0B,
            0x00,
            0x00,
            0x00,
            MessageMagic::End as u8,
        ];
        let mut fds = Vec::new();
        let msg = GenericProtocolMessage::from_bytes(&bytes, &mut fds, 0).unwrap();
        assert_eq!(msg.object, 1);
        assert_eq!(msg.method, 1);
    }

    #[test]
    fn from_bytes_with_fd_array() {
        let bytes = [
            MessageType::GenericProtocolMessage as u8,
            MessageMagic::TypeObject as u8,
            0x01,
            0x00,
            0x00,
            0x00,
            MessageMagic::TypeUint as u8,
            0x01,
            0x00,
            0x00,
            0x00,
            // array of 2 fds
            MessageMagic::TypeArray as u8,
            MessageMagic::TypeFd as u8,
            0x02, // varint count = 2
            MessageMagic::End as u8,
        ];
        let mut fds = vec![10, 20, 30];
        let msg = GenericProtocolMessage::from_bytes(&bytes, &mut fds, 0).unwrap();
        assert_eq!(msg.fds, vec![10, 20]);
        assert_eq!(fds, vec![30]);
    }

    #[test]
    fn from_bytes_with_offset() {
        let bytes = [
            0xAA,
            0xBB,
            MessageType::GenericProtocolMessage as u8,
            MessageMagic::TypeObject as u8,
            0x07,
            0x00,
            0x00,
            0x00,
            MessageMagic::TypeUint as u8,
            0x09,
            0x00,
            0x00,
            0x00,
            MessageMagic::End as u8,
        ];
        let mut fds = Vec::new();
        let msg = GenericProtocolMessage::from_bytes(&bytes, &mut fds, 2).unwrap();
        assert_eq!(msg.object, 7);
        assert_eq!(msg.method, 9);
    }

    #[test]
    fn from_bytes_invalid_message_type() {
        let bytes = [MessageType::Sup as u8];
        let mut fds = Vec::new();
        let err = GenericProtocolMessage::from_bytes(&bytes, &mut fds, 0).unwrap_err();
        assert!(matches!(err, MessageError::InvalidMessageType));
    }

    #[test]
    fn from_bytes_unexpected_eof() {
        let bytes = [
            MessageType::GenericProtocolMessage as u8,
            MessageMagic::TypeObject as u8,
        ];
        let mut fds = Vec::new();
        let err = GenericProtocolMessage::from_bytes(&bytes, &mut fds, 0).unwrap_err();
        assert!(matches!(err, MessageError::UnexpectedEof));
    }

    #[test]
    fn new_ownership() {
        let data = vec![
            MessageType::GenericProtocolMessage as u8,
            MessageMagic::TypeObject as u8,
            0x01,
            0x00,
            0x00,
            0x00,
            MessageMagic::TypeUint as u8,
            0x01,
            0x00,
            0x00,
            0x00,
            MessageMagic::End as u8,
        ];
        let fds = vec![1, 2, 3];
        let expected_data = data.clone();
        let expected_fds = fds.clone();
        let msg = GenericProtocolMessage::new(data, fds);
        assert_eq!(msg.data(), &expected_data[..]);
        assert_eq!(msg.fds(), &expected_fds[..]);
        assert_eq!(
            msg.message_type() as u8,
            MessageType::GenericProtocolMessage as u8
        );
    }
}
