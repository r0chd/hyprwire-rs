extern crate alloc;

pub mod bind_protocol;
pub mod fatal_protocol_error;
pub mod generic_protocol_message;
pub mod handshake_ack;
pub mod handshake_begin;
pub mod handshake_protocols;
pub mod hello;
pub mod new_object;
pub mod roundtrip_done;
pub mod roundtrip_request;

use crate::{message, types};
use alloc::fmt::Write;
use alloc::string::ToString;
use alloc::{format, string};
use core::result;

pub type Result<T> = result::Result<T, message::Error>;

pub(crate) fn read_u32(data: &[u8], offset: usize) -> Result<u32> {
    let bytes = data
        .get(offset..offset + size_of::<u32>())
        .ok_or(message::Error::UnexpectedEof)?;
    Ok(u32::from_le_bytes(
        bytes
            .try_into()
            .map_err(|_| message::Error::UnexpectedEof)?,
    ))
}

fn read_i32(data: &[u8], offset: usize) -> Result<i32> {
    let bytes = data
        .get(offset..offset + size_of::<i32>())
        .ok_or(message::Error::UnexpectedEof)?;
    Ok(i32::from_le_bytes(
        bytes
            .try_into()
            .map_err(|_| message::Error::UnexpectedEof)?,
    ))
}

fn read_f32(data: &[u8], offset: usize) -> Result<f32> {
    let bytes = data
        .get(offset..offset + size_of::<f32>())
        .ok_or(message::Error::UnexpectedEof)?;
    Ok(f32::from_le_bytes(
        bytes
            .try_into()
            .map_err(|_| message::Error::UnexpectedEof)?,
    ))
}

pub trait Message {
    fn data(&self) -> &[u8];

    fn message_type(&self) -> message::MessageType;

    fn fds(&self) -> &[i32] {
        &[]
    }

    fn parse_data(&self) -> string::String {
        let mut result = string::String::new();
        let data = self.data();

        let _ = write!(result, "{} ( ", self.message_type());

        let mut first = true;
        let mut needle: usize = 1;
        while needle < data.len() {
            let Ok(magic) = types::MessageMagic::try_from(data[needle]) else {
                result.push_str("<malformed>");
                break;
            };
            needle += 1;

            match magic {
                types::MessageMagic::TypeSeq => {
                    if !first {
                        result.push_str(", ");
                    }
                    first = false;
                    let Ok(value) = read_u32(data, needle) else {
                        result.push_str("<eof>");
                        break;
                    };
                    let _ = write!(result, "seq: {value}");
                    needle += 4;
                }
                types::MessageMagic::TypeUint => {
                    if !first {
                        result.push_str(", ");
                    }
                    first = false;
                    let Ok(value) = read_u32(data, needle) else {
                        result.push_str("<eof>");
                        break;
                    };
                    let _ = write!(result, "{value}");
                    needle += 4;
                }
                types::MessageMagic::TypeInt => {
                    if !first {
                        result.push_str(", ");
                    }
                    first = false;
                    let Ok(value) = read_i32(data, needle) else {
                        result.push_str("<eof>");
                        break;
                    };
                    let _ = write!(result, "{value}");
                    needle += 4;
                }
                types::MessageMagic::TypeF32 => {
                    if !first {
                        result.push_str(", ");
                    }
                    first = false;
                    let Ok(value) = read_f32(data, needle) else {
                        result.push_str("<eof>");
                        break;
                    };
                    let _ = write!(result, "{value}");
                    needle += 4;
                }
                types::MessageMagic::TypeVarchar => {
                    if !first {
                        result.push_str(", ");
                    }
                    first = false;
                    let (len, int_len) = message::parse_var_int(data, needle);
                    if len > 0 {
                        let str_data = &data[needle + int_len..needle + int_len + len];
                        let s = string::String::from_utf8_lossy(str_data);
                        let _ = write!(result, "\"{s}\"");
                    } else {
                        result.push_str("\"\"");
                    }
                    needle += int_len + len;
                }
                types::MessageMagic::TypeArray => {
                    if !first {
                        result.push_str(", ");
                    }
                    first = false;
                    let Some(&type_byte) = data.get(needle) else {
                        result.push_str("<eof>");
                        break;
                    };
                    let Ok(this_type) = types::MessageMagic::try_from(type_byte) else {
                        result.push_str("<malformed>");
                        break;
                    };
                    needle += 1;

                    let (els, int_len) = message::parse_var_int(data, needle);
                    result.push_str("{ ");
                    needle += int_len;

                    for i in 0..els {
                        let Ok((s, len)) = format_primitive_type(&data[needle..], this_type) else {
                            result.push_str("<malformed>");
                            break;
                        };

                        needle += len;
                        result.push_str(&s);
                        if i < els - 1 {
                            result.push_str(", ");
                        }
                    }

                    result.push_str(" }");
                }
                types::MessageMagic::TypeObject => {
                    if !first {
                        result.push_str(", ");
                    }
                    first = false;
                    let Ok(id) = read_u32(data, needle) else {
                        result.push_str("<eof>");
                        break;
                    };
                    let _ = write!(result, "object({id})");
                    needle += 4;
                }
                types::MessageMagic::TypeFd => {
                    if !first {
                        result.push_str(", ");
                    }
                    first = false;
                    result.push_str("<fd>");
                }
                types::MessageMagic::End | types::MessageMagic::TypeObjectId => {}
            }
        }

        result.push_str(" ) ");
        result
    }
}

fn format_primitive_type(s: &[u8], r#type: types::MessageMagic) -> Result<(string::String, usize)> {
    match r#type {
        types::MessageMagic::TypeUint => {
            let value = read_u32(s, 0)?;
            Ok((value.to_string(), 4))
        }
        types::MessageMagic::TypeInt => {
            let value = read_i32(s, 0)?;
            Ok((value.to_string(), 4))
        }
        types::MessageMagic::TypeF32 => {
            let value = read_f32(s, 0)?;
            Ok((value.to_string(), 4))
        }
        types::MessageMagic::TypeFd => Ok(("<fd>".to_string(), 0)),
        types::MessageMagic::TypeObject => {
            let id = read_u32(s, 0)?;
            let obj_str = if id == 0 {
                "null".to_string()
            } else {
                id.to_string()
            };
            Ok((format!("object: {obj_str}"), 4))
        }
        types::MessageMagic::TypeVarchar => {
            let (len, int_len) = crate::message::parse_var_int(s, 0);
            let str_data = s
                .get(int_len..int_len + len)
                .ok_or(message::Error::UnexpectedEof)?;
            let value = string::String::from_utf8(str_data.to_vec())
                .map_err(|_| message::Error::MalformedMessage)?;
            Ok((format!("\"{value}\""), len + int_len))
        }
        _ => Err(message::Error::MalformedMessage),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::types;

    struct TestMessage<'a> {
        data: &'a [u8],
        message_type: message::MessageType,
    }

    impl<'a> Message for TestMessage<'a> {
        fn data(&self) -> &[u8] {
            self.data
        }
        fn message_type(&self) -> message::MessageType {
            self.message_type
        }
    }

    #[test]
    fn parse_data_integer_types() {
        let bytes: &[u8] = &[
            message::MessageType::GenericProtocolMessage as u8,
            types::MessageMagic::TypeSeq as u8,
            0x01,
            0x00,
            0x00,
            0x00,
            types::MessageMagic::TypeInt as u8,
            0x01,
            0x00,
            0x00,
            0x00,
            types::MessageMagic::TypeF32 as u8,
            0x01,
            0x00,
            0x00,
            0x00,
            types::MessageMagic::End as u8,
        ];
        let msg = TestMessage {
            data: bytes,
            message_type: message::MessageType::GenericProtocolMessage,
        };
        let data = msg.parse_data();
        let expected_f32 = f32::from_le_bytes([0x01, 0x00, 0x00, 0x00]);
        assert_eq!(
            data,
            format!("GenericProtocolMessage ( seq: 1, 1, {expected_f32} ) ")
        );
    }

    #[test]
    fn parse_data_varchar_empty() {
        let bytes: &[u8] = &[
            message::MessageType::GenericProtocolMessage as u8,
            types::MessageMagic::TypeVarchar as u8,
            0x00,
            types::MessageMagic::End as u8,
        ];
        let msg = TestMessage {
            data: bytes,
            message_type: message::MessageType::GenericProtocolMessage,
        };
        let data = msg.parse_data();
        assert_eq!(data, "GenericProtocolMessage ( \"\" ) ");
    }
}
