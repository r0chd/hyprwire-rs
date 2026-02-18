use super::MessageType;
use crate::implementation::types::MessageMagic;
use crate::message;
use std::mem;

pub trait Message {
    fn get_data(&self) -> String;

    fn get_message_type(&self) -> MessageType;

    fn get_fds(&self) -> &[i32] {
        &[]
    }

    fn parseData(&self) -> String {
        let mut result = String::new();

        let mut first = true;
        let mut needle: usize = 1;
        while needle < self.get_data().len() {
            let magic_byte = self.get_data().as_bytes()[needle];
            needle += 1;

            let magic: MessageMagic = unsafe { mem::transmute(magic_byte) };
            match magic {
                MessageMagic::End => {}
                MessageMagic::TypeSeq => {
                    if !first {
                        result.push_str(", ");
                        first = false;
                    }
                    let bytes: [u8; 4] = self.get_data().as_bytes()[needle..needle + 4]
                        .try_into()
                        .unwrap();
                    let value = u32::from_ne_bytes(bytes);
                    result.push_str(&format!("seq: {value}"));
                    needle += 4;
                }
                MessageMagic::TypeUint => {
                    if !first {
                        result.push_str(", ");
                        first = false;
                    }
                    let bytes: [u8; 4] = self.get_data().as_bytes()[needle..needle + 4]
                        .try_into()
                        .unwrap();
                    let value = u32::from_ne_bytes(bytes);
                    result.push_str(&format!("{value}"));
                    needle += 4;
                }
                MessageMagic::TypeInt => {
                    if !first {
                        result.push_str(", ");
                        first = false;
                    }
                    let bytes: [u8; 4] = self.get_data().as_bytes()[needle..needle + 4]
                        .try_into()
                        .unwrap();
                    let value = i32::from_ne_bytes(bytes);
                    result.push_str(&format!("{value}"));
                    needle += 4;
                }
                MessageMagic::TypeF32 => {
                    if !first {
                        result.push_str(", ");
                        first = false;
                    }
                    let bytes: [u8; 4] = self.get_data().as_bytes()[needle..needle + 4]
                        .try_into()
                        .unwrap();
                    let value = f32::from_ne_bytes(bytes);
                    result.push_str(&format!("{value}"));
                    needle += 4;
                }
                MessageMagic::TypeVarchar => {
                    if !first {
                        result.push_str(", ");
                        first = false;
                    }

                    let (len, int_len) = message::parse_var_int(self.get_data().as_bytes(), needle);
                    if len > 0 {
                        result.push_str(&format!(
                            "\"{}\"",
                            self.get_data()
                                .get(needle + int_len..needle + int_len + len)
                                .unwrap()
                        ));
                    } else {
                        result.push_str("\"\"");
                    }
                    needle += int_len + len;
                }
                MessageMagic::TypeArray => {
                    if !first {
                        result.push_str(", ");
                        first = false;
                    }
                    let this_type: MessageMagic =
                        unsafe { mem::transmute(self.get_data().into_bytes()[needle]) };
                    needle += 1;

                    let (els, int_len) = message::parse_var_int(self.get_data().as_bytes(), 0);
                    result.push_str("{ ");
                    needle += int_len;

                    for i in 0..els {
                        let (str, len) =
                            formatPrimitiveType(&self.get_data().as_bytes()[needle..], this_type);

                        needle += len;
                        result.push_str(&str);
                        if i < els - 1 {
                            result.push_str(", ");
                        }
                    }

                    result.push_str(" }");
                }
                MessageMagic::TypeObject => {
                    if !first {
                        result.push_str(", ");
                        first = false;
                    }
                    let bytes: [u8; 4] = self.get_data().as_bytes()[needle..needle + 4]
                        .try_into()
                        .unwrap();
                    let id = u32::from_ne_bytes(bytes);
                    result.push_str(&format!("object({id})"));
                    needle += 4;
                }
                MessageMagic::TypeFd => {
                    if !first {
                        result.push_str(", ");
                        first = false;
                    }
                    result.push_str("<fd>");
                }
                _ => {
                    todo!("put error here");
                }
            }
        }

        result.push_str(" ) ");
        result
    }
}

fn formatPrimitiveType(s: &[u8], r#type: MessageMagic) -> (String, usize) {
    match r#type {
        MessageMagic::TypeUint => {
            let bytes: [u8; 4] = s[0..4].try_into().unwrap();
            let value = u32::from_ne_bytes(bytes);

            (value.to_string(), 4)
        }
        MessageMagic::TypeInt => {
            let bytes: [u8; 4] = s[0..4].try_into().unwrap();
            let value = i32::from_ne_bytes(bytes);

            (value.to_string(), 4)
        }
        MessageMagic::TypeF32 => {
            let bytes: [u8; 4] = s[0..4].try_into().unwrap();
            let value = f32::from_ne_bytes(bytes);

            (value.to_string(), 4)
        }
        MessageMagic::TypeFd => ("<fd>".to_string(), 0),
        MessageMagic::TypeObject => {
            let bytes: [u8; 4] = s[0..4].try_into().unwrap();
            let id = u32::from_ne_bytes(bytes);
            let obj_str = if id == 0 {
                "null".to_string()
            } else {
                id.to_string()
            };

            (format!("object: {obj_str}"), 4)
        }
        MessageMagic::TypeVarchar => {
            let (len, int_len) = crate::message::parse_var_int(s, 0);
            (
                format!(
                    "\"{}\"",
                    String::from_utf8(s[int_len..int_len + len].to_vec()).unwrap()
                ),
                len + int_len,
            )
        }
        _ => (String::new(), 0),
    }
}
