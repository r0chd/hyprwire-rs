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

use crate::implementation::types;
use crate::message;
use std::fmt::Write;
use std::result;

pub type Result<T> = result::Result<T, message::Error>;

pub trait Message {
    fn data(&self) -> &[u8];

    fn message_type(&self) -> message::MessageType;

    fn fds(&self) -> &[i32] {
        &[]
    }

    fn parse_data(&self) -> String {
        let mut result = String::new();
        let data = self.data();

        let _ = write!(result, "{} ( ", self.message_type());

        let mut first = true;
        let mut needle: usize = 1;
        while needle < data.len() {
            let magic = types::MessageMagic::try_from(data[needle]).unwrap();
            needle += 1;

            match magic {
                types::MessageMagic::TypeSeq => {
                    if !first {
                        result.push_str(", ");
                    }
                    first = false;
                    let bytes: [u8; 4] = data[needle..needle + 4].try_into().unwrap();
                    let value = u32::from_le_bytes(bytes);
                    let _ = write!(result, "seq: {value}");
                    needle += 4;
                }
                types::MessageMagic::TypeUint => {
                    if !first {
                        result.push_str(", ");
                    }
                    first = false;
                    let bytes: [u8; 4] = data[needle..needle + 4].try_into().unwrap();
                    let value = u32::from_le_bytes(bytes);
                    let _ = write!(result, "{value}");
                    needle += 4;
                }
                types::MessageMagic::TypeInt => {
                    if !first {
                        result.push_str(", ");
                    }
                    first = false;
                    let bytes: [u8; 4] = data[needle..needle + 4].try_into().unwrap();
                    let value = i32::from_le_bytes(bytes);
                    let _ = write!(result, "{value}");
                    needle += 4;
                }
                types::MessageMagic::TypeF32 => {
                    if !first {
                        result.push_str(", ");
                    }
                    first = false;
                    let bytes: [u8; 4] = data[needle..needle + 4].try_into().unwrap();
                    let value = f32::from_le_bytes(bytes);
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
                        let s = String::from_utf8_lossy(str_data);
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
                    let type_byte = data[needle];
                    let this_type = types::MessageMagic::try_from(type_byte).unwrap();
                    needle += 1;

                    let (els, int_len) = message::parse_var_int(data, needle);
                    result.push_str("{ ");
                    needle += int_len;

                    for i in 0..els {
                        let (s, len) = format_primitive_type(&data[needle..], this_type).unwrap();

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
                    let bytes: [u8; 4] = data[needle..needle + 4].try_into().unwrap();
                    let id = u32::from_le_bytes(bytes);
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

fn format_primitive_type(s: &[u8], r#type: types::MessageMagic) -> Result<(String, usize)> {
    match r#type {
        types::MessageMagic::TypeUint => {
            let bytes: [u8; 4] = s
                .get(0..4)
                .ok_or(message::Error::UnexpectedEof)?
                .try_into()
                .unwrap();
            let value = u32::from_le_bytes(bytes);
            Ok((value.to_string(), 4))
        }
        types::MessageMagic::TypeInt => {
            let bytes: [u8; 4] = s
                .get(0..4)
                .ok_or(message::Error::UnexpectedEof)?
                .try_into()
                .unwrap();
            let value = i32::from_le_bytes(bytes);
            Ok((value.to_string(), 4))
        }
        types::MessageMagic::TypeF32 => {
            let bytes: [u8; 4] = s
                .get(0..4)
                .ok_or(message::Error::UnexpectedEof)?
                .try_into()
                .unwrap();
            let value = f32::from_le_bytes(bytes);
            Ok((value.to_string(), 4))
        }
        types::MessageMagic::TypeFd => Ok(("<fd>".to_string(), 0)),
        types::MessageMagic::TypeObject => {
            let bytes: [u8; 4] = s
                .get(0..4)
                .ok_or(message::Error::UnexpectedEof)?
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
        types::MessageMagic::TypeVarchar => {
            let (len, int_len) = crate::message::parse_var_int(s, 0);
            let str_data = s
                .get(int_len..int_len + len)
                .ok_or(message::Error::UnexpectedEof)?;
            let value = String::from_utf8(str_data.to_vec())
                .map_err(|_| message::Error::MalformedMessage)?;
            Ok((format!("\"{value}\""), len + int_len))
        }
        _ => Err(message::Error::MalformedMessage),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::implementation::types;
    use crate::message::messages::{
        bind_protocol::BindProtocol, fatal_protocol_error::FatalProtocolError,
        handshake_ack::HandshakeAck, handshake_protocols::HandshakeProtocols, hello::Hello,
        new_object::NewObject, roundtrip_done::RoundtripDone, roundtrip_request::RoundtripRequest,
    };

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
    fn parse_data_hello_contains_type_and_payload() {
        let msg = Hello::new();
        let parsed = msg.parse_data();
        assert!(parsed.contains("Sup"), "missing type in: {parsed}");
        assert!(parsed.contains("\"VAX\""), "missing payload in: {parsed}");
    }

    #[test]
    fn parse_data_handshake_ack_contains_type_and_version() {
        let msg = HandshakeAck::new(7);
        let parsed = msg.parse_data();
        assert!(parsed.contains("HandshakeAck"), "missing type in: {parsed}");
        assert!(parsed.contains('7'), "missing version in: {parsed}");
    }

    #[test]
    fn parse_data_handshake_protocols_contains_protocol_names() {
        let msg = HandshakeProtocols::new(&["proto@1", "second@2"]);
        let parsed = msg.parse_data();
        assert!(
            parsed.contains("HandshakeProtocols"),
            "missing type in: {parsed}"
        );
        assert!(
            parsed.contains("\"proto@1\""),
            "missing proto@1 in: {parsed}"
        );
        assert!(
            parsed.contains("\"second@2\""),
            "missing second@2 in: {parsed}"
        );
    }

    #[test]
    fn parse_data_bind_protocol_contains_core_fields() {
        let msg = BindProtocol::new("my_proto", 12, 3);
        let parsed = msg.parse_data();
        assert!(parsed.contains("BindProtocol"), "missing type in: {parsed}");
        assert!(parsed.contains("12"), "missing seq in: {parsed}");
        assert!(
            parsed.contains("\"my_proto\""),
            "missing protocol in: {parsed}"
        );
        assert!(parsed.contains('3'), "missing version in: {parsed}");
    }

    #[test]
    fn parse_data_new_object_contains_object_and_seq() {
        let msg = NewObject::new(9, 77);
        let parsed = msg.parse_data();
        assert!(parsed.contains("NewObject"), "missing type in: {parsed}");
        assert!(parsed.contains("77"), "missing id in: {parsed}");
        assert!(parsed.contains('9'), "missing seq in: {parsed}");
    }

    #[test]
    fn parse_data_fatal_error_contains_identifiers_and_message() {
        let msg = FatalProtocolError::new(0, 123, "oops");
        let parsed = msg.parse_data();
        assert!(
            parsed.contains("FatalProtocolError"),
            "missing type in: {parsed}"
        );
        assert!(parsed.contains("123"), "missing error_id in: {parsed}");
        assert!(
            parsed.contains("\"oops\""),
            "missing error_msg in: {parsed}"
        );
    }

    #[test]
    fn parse_data_roundtrip_messages_contain_type_and_sequence() {
        let req = RoundtripRequest::new(777);
        let done = RoundtripDone::new(888);

        let req_parsed = req.parse_data();
        let done_parsed = done.parse_data();

        assert!(
            req_parsed.contains("RoundtripRequest"),
            "missing type in: {req_parsed}"
        );
        assert!(req_parsed.contains("777"), "missing seq in: {req_parsed}");

        assert!(
            done_parsed.contains("RoundtripDone"),
            "missing type in: {done_parsed}"
        );
        assert!(done_parsed.contains("888"), "missing seq in: {done_parsed}");
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
