mod hello;
mod new_object;

use super::{MessageError, MessageType};
use crate::implementation::types::MessageMagic;
use crate::message;

pub trait Message {
    fn get_data(&self) -> &[u8];

    fn get_message_type(&self) -> MessageType;

    fn get_fds(&self) -> &[i32] {
        &[]
    }

    fn parseData(&self) -> Result<String, MessageError> {
        let mut result = String::new();
        let data = self.get_data();

        let mut first = true;
        let mut needle: usize = 1;
        while needle < data.len() {
            if needle >= data.len() {
                return Err(MessageError::UnexpectedEof);
            }

            let magic = MessageMagic::try_from(data[needle])?;
            needle += 1;

            match magic {
                MessageMagic::End => {}
                MessageMagic::TypeSeq => {
                    if !first {
                        result.push_str(", ");
                    }
                    first = false;
                    let bytes: [u8; 4] = data
                        .get(needle..needle + 4)
                        .ok_or(MessageError::UnexpectedEof)?
                        .try_into()
                        .unwrap();
                    let value = u32::from_le_bytes(bytes);
                    result.push_str(&format!("seq: {value}"));
                    needle += 4;
                }
                MessageMagic::TypeUint => {
                    if !first {
                        result.push_str(", ");
                    }
                    first = false;
                    let bytes: [u8; 4] = data
                        .get(needle..needle + 4)
                        .ok_or(MessageError::UnexpectedEof)?
                        .try_into()
                        .unwrap();
                    let value = u32::from_le_bytes(bytes);
                    result.push_str(&format!("{value}"));
                    needle += 4;
                }
                MessageMagic::TypeInt => {
                    if !first {
                        result.push_str(", ");
                    }
                    first = false;
                    let bytes: [u8; 4] = data
                        .get(needle..needle + 4)
                        .ok_or(MessageError::UnexpectedEof)?
                        .try_into()
                        .unwrap();
                    let value = i32::from_le_bytes(bytes);
                    result.push_str(&format!("{value}"));
                    needle += 4;
                }
                MessageMagic::TypeF32 => {
                    if !first {
                        result.push_str(", ");
                    }
                    first = false;
                    let bytes: [u8; 4] = data
                        .get(needle..needle + 4)
                        .ok_or(MessageError::UnexpectedEof)?
                        .try_into()
                        .unwrap();
                    let value = f32::from_le_bytes(bytes);
                    result.push_str(&format!("{value}"));
                    needle += 4;
                }
                MessageMagic::TypeVarchar => {
                    if !first {
                        result.push_str(", ");
                    }
                    first = false;
                    let (len, int_len) = message::parse_var_int(data, needle);
                    if len > 0 {
                        let str_data = data
                            .get(needle + int_len..needle + int_len + len)
                            .ok_or(MessageError::UnexpectedEof)?;
                        let s = String::from_utf8(str_data.to_vec())
                            .map_err(|_| MessageError::MalformedMessage)?;
                        result.push_str(&format!("\"{s}\""));
                    } else {
                        result.push_str("\"\"");
                    }
                    needle += int_len + len;
                }
                MessageMagic::TypeArray => {
                    if !first {
                        result.push_str(", ");
                    }
                    first = false;
                    let type_byte = *data.get(needle).ok_or(MessageError::UnexpectedEof)?;
                    let this_type = MessageMagic::try_from(type_byte)?;
                    needle += 1;

                    let (els, int_len) = message::parse_var_int(data, needle);
                    result.push_str("{ ");
                    needle += int_len;

                    for i in 0..els {
                        let remaining = data.get(needle..).ok_or(MessageError::UnexpectedEof)?;
                        let (s, len) = format_primitive_type(remaining, this_type)?;

                        needle += len;
                        result.push_str(&s);
                        if i < els - 1 {
                            result.push_str(", ");
                        }
                    }

                    result.push_str(" }");
                }
                MessageMagic::TypeObject => {
                    if !first {
                        result.push_str(", ");
                    }
                    first = false;
                    let bytes: [u8; 4] = data
                        .get(needle..needle + 4)
                        .ok_or(MessageError::UnexpectedEof)?
                        .try_into()
                        .unwrap();
                    let id = u32::from_le_bytes(bytes);
                    result.push_str(&format!("object({id})"));
                    needle += 4;
                }
                MessageMagic::TypeFd => {
                    if !first {
                        result.push_str(", ");
                    }
                    first = false;
                    result.push_str("<fd>");
                }
                MessageMagic::TypeObjectId => {
                    return Err(MessageError::MalformedMessage);
                }
            }
        }

        result.push_str(" ) ");
        Ok(result)
    }
}

fn format_primitive_type(s: &[u8], r#type: MessageMagic) -> Result<(String, usize), MessageError> {
    match r#type {
        MessageMagic::TypeUint => {
            let bytes: [u8; 4] = s
                .get(0..4)
                .ok_or(MessageError::UnexpectedEof)?
                .try_into()
                .unwrap();
            let value = u32::from_le_bytes(bytes);
            Ok((value.to_string(), 4))
        }
        MessageMagic::TypeInt => {
            let bytes: [u8; 4] = s
                .get(0..4)
                .ok_or(MessageError::UnexpectedEof)?
                .try_into()
                .unwrap();
            let value = i32::from_le_bytes(bytes);
            Ok((value.to_string(), 4))
        }
        MessageMagic::TypeF32 => {
            let bytes: [u8; 4] = s
                .get(0..4)
                .ok_or(MessageError::UnexpectedEof)?
                .try_into()
                .unwrap();
            let value = f32::from_le_bytes(bytes);
            Ok((value.to_string(), 4))
        }
        MessageMagic::TypeFd => Ok(("<fd>".to_string(), 0)),
        MessageMagic::TypeObject => {
            let bytes: [u8; 4] = s
                .get(0..4)
                .ok_or(MessageError::UnexpectedEof)?
                .try_into()
                .unwrap();
            let id = u32::from_le_bytes(bytes);
            let obj_str = if id == 0 {
                "null".to_string()
            } else {
                id.to_string()
            };
            Ok((format!("object: {obj_str}"), 4))
        }
        MessageMagic::TypeVarchar => {
            let (len, int_len) = crate::message::parse_var_int(s, 0);
            let str_data = s
                .get(int_len..int_len + len)
                .ok_or(MessageError::UnexpectedEof)?;
            let value =
                String::from_utf8(str_data.to_vec()).map_err(|_| MessageError::MalformedMessage)?;
            Ok((format!("\"{value}\""), len + int_len))
        }
        _ => Err(MessageError::MalformedMessage),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::implementation::types::MessageMagic;

    struct TestMessage<'a> {
        data: &'a [u8],
        message_type: MessageType,
    }

    impl<'a> Message for TestMessage<'a> {
        fn get_data(&self) -> &[u8] {
            self.data
        }
        fn get_message_type(&self) -> MessageType {
            self.message_type
        }
    }

    #[test]
    fn parse_data_integer_types() {
        let bytes: &[u8] = &[
            MessageType::GenericProtocolMessage as u8,
            MessageMagic::TypeSeq as u8,
            0x01,
            0x00,
            0x00,
            0x00,
            MessageMagic::TypeInt as u8,
            0x01,
            0x00,
            0x00,
            0x00,
            MessageMagic::TypeF32 as u8,
            0x01,
            0x00,
            0x00,
            0x00,
            MessageMagic::End as u8,
        ];
        let msg = TestMessage {
            data: bytes,
            message_type: MessageType::GenericProtocolMessage,
        };
        let data = msg.parseData().unwrap();
        let expected_f32 = f32::from_le_bytes([0x01, 0x00, 0x00, 0x00]);
        assert_eq!(data, format!("seq: 1, 1, {expected_f32} ) "));
    }

    #[test]
    fn parse_data_varchar_empty() {
        let bytes: &[u8] = &[
            MessageType::GenericProtocolMessage as u8,
            MessageMagic::TypeVarchar as u8,
            0x00,
            MessageMagic::End as u8,
        ];
        let msg = TestMessage {
            data: bytes,
            message_type: MessageType::GenericProtocolMessage,
        };
        let data = msg.parseData().unwrap();
        assert_eq!(data, "\"\" ) ");
    }
}
