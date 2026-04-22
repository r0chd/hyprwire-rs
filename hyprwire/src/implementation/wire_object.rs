use crate::implementation::object;
use crate::{steady_millis, trace};
use hyprwire_core::message;
use hyprwire_core::message::wire::generic_protocol_message;
use hyprwire_core::types;
use std::os::fd::AsRawFd;

pub trait WireObject: object::Object {
    fn set_version(&self, version: u32);

    fn version(&self) -> u32;

    fn methods_out(&self) -> &[types::Method];

    fn errd(&self);

    fn mark_destroyed(&self) {}

    fn send_message(&self, msg: &dyn message::Message);

    fn protocol_name(&self) -> &str;

    fn server(&self) -> bool;

    fn id(&self) -> u32;

    fn seq(&self) -> u32;

    fn call(&self, id: u32, args: &[types::CallArg]) -> Result<u32, message::Error> {
        let methods = self.methods_out();

        if methods.len() <= id as usize {
            let msg = format!("invalid method {} for object {}", id, self.id());
            crate::log_error!("core protocol error: {msg}");
            self.error(self.id(), &msg);
            return Ok(0);
        }

        let method = &methods[id as usize];

        if method.since > self.version() {
            let msg = format!(
                "method {} since {} but has {}",
                id,
                method.since,
                self.version()
            );
            crate::log_error!("core protocol error: {msg}");
            self.error(self.id(), &msg);
            return Ok(0);
        }

        if !method.returns_type.is_empty() && self.server() {
            let msg = format!(
                "invalid method spec {} for object {} -> server cannot call returnsType methods",
                id,
                self.id()
            );
            crate::log_error!("core protocol error: {msg}");
            self.error(self.id(), &msg);
            return Ok(0);
        }

        let method_params = method.params;
        let method_returns_type = method.returns_type;
        let method_destructor = method.destructor;

        if method_destructor {
            self.mark_destroyed();
        }

        // encode the message
        let mut data: Vec<u8> = Vec::new();
        let mut fds: Vec<i32> = Vec::new();

        data.push(message::MessageType::GenericProtocolMessage as u8);
        data.push(types::MessageMagic::TypeObject as u8);

        let obj_id = self.id();
        data.extend_from_slice(&obj_id.to_le_bytes());

        data.push(types::MessageMagic::TypeUint as u8);
        data.extend_from_slice(&id.to_le_bytes());

        let mut return_seq: u32 = 0;

        if !method_returns_type.is_empty() {
            trace! {
                if let Some(client) = self.client_sock() {
                    crate::log_debug!("[hw] trace: [{} @ {:.3}] -- call {}: returnsType has {}", client.0.state.stream.as_raw_fd(), steady_millis(), id, method_returns_type);
                }
            }

            data.push(types::MessageMagic::TypeSeq as u8);
            if let Some(client) = self.client_sock() {
                return_seq = client.0.seq.get() + 1;
                client.0.seq.set(return_seq);
            }
            data.extend_from_slice(&return_seq.to_le_bytes());
        }

        let mut arg_idx: usize = 0;
        let mut i: usize = 0;
        while i < method_params.len() {
            let Ok(param) = types::MessageMagic::try_from(method_params[i]) else {
                break;
            };

            match param {
                types::MessageMagic::TypeUint => {
                    data.push(types::MessageMagic::TypeUint as u8);
                    if let Some(types::CallArg::Uint(val)) = args.get(arg_idx) {
                        data.extend_from_slice(&val.to_le_bytes());
                    }
                    arg_idx += 1;
                }
                types::MessageMagic::TypeInt => {
                    data.push(types::MessageMagic::TypeInt as u8);
                    if let Some(types::CallArg::Int(val)) = args.get(arg_idx) {
                        data.extend_from_slice(&val.to_le_bytes());
                    }
                    arg_idx += 1;
                }
                types::MessageMagic::TypeObject => {
                    data.push(types::MessageMagic::TypeObject as u8);
                    if let Some(types::CallArg::Object(val)) = args.get(arg_idx) {
                        data.extend_from_slice(&val.to_le_bytes());
                    }
                    arg_idx += 1;
                }
                types::MessageMagic::TypeF32 => {
                    data.push(types::MessageMagic::TypeF32 as u8);
                    if let Some(types::CallArg::F32(val)) = args.get(arg_idx) {
                        data.extend_from_slice(&val.to_le_bytes());
                    }
                    arg_idx += 1;
                }
                types::MessageMagic::TypeVarchar => {
                    data.push(types::MessageMagic::TypeVarchar as u8);
                    if let Some(types::CallArg::Varchar(s)) = args.get(arg_idx) {
                        let mut var_int_buf = [0u8; 10];
                        let encoded = message::encode_var_int(s.len(), &mut var_int_buf);
                        data.extend_from_slice(encoded);
                        data.extend_from_slice(s);
                    }
                    arg_idx += 1;
                }
                types::MessageMagic::TypeFd => {
                    data.push(types::MessageMagic::TypeFd as u8);
                    if let Some(types::CallArg::Fd(fd)) = args.get(arg_idx) {
                        fds.push(*fd);
                    }
                    arg_idx += 1;
                }
                types::MessageMagic::TypeArray => {
                    i += 1;
                    let Ok(arr_type) = types::MessageMagic::try_from(method_params[i]) else {
                        break;
                    };

                    data.push(types::MessageMagic::TypeArray as u8);
                    data.push(arr_type as u8);

                    match args.get(arg_idx) {
                        Some(types::CallArg::UintArray(arr) | types::CallArg::ObjectArray(arr)) => {
                            let mut var_int_buf = [0u8; 10];
                            let encoded = message::encode_var_int(arr.len(), &mut var_int_buf);
                            data.extend_from_slice(encoded);
                            for val in *arr {
                                data.extend_from_slice(&val.to_le_bytes());
                            }
                        }
                        Some(types::CallArg::IntArray(arr)) => {
                            let mut var_int_buf = [0u8; 10];
                            let encoded = message::encode_var_int(arr.len(), &mut var_int_buf);
                            data.extend_from_slice(encoded);
                            for val in *arr {
                                data.extend_from_slice(&val.to_le_bytes());
                            }
                        }
                        Some(types::CallArg::F32Array(arr)) => {
                            let mut var_int_buf = [0u8; 10];
                            let encoded = message::encode_var_int(arr.len(), &mut var_int_buf);
                            data.extend_from_slice(encoded);
                            for val in *arr {
                                data.extend_from_slice(&val.to_le_bytes());
                            }
                        }
                        Some(types::CallArg::FdArray(arr)) => {
                            let mut var_int_buf = [0u8; 10];
                            let encoded = message::encode_var_int(arr.len(), &mut var_int_buf);
                            data.extend_from_slice(encoded);
                            for fd in *arr {
                                fds.push(*fd);
                            }
                        }
                        Some(types::CallArg::VarcharArray(arr)) => {
                            let mut var_int_buf = [0u8; 10];
                            let encoded = message::encode_var_int(arr.len(), &mut var_int_buf);
                            data.extend_from_slice(encoded);
                            for s in *arr {
                                let encoded = message::encode_var_int(s.len(), &mut var_int_buf);
                                data.extend_from_slice(encoded);
                                data.extend_from_slice(s);
                            }
                        }
                        _ => {
                            crate::log_error!("core protocol error: failed marshaling array type");
                            self.errd();
                            return Ok(0);
                        }
                    }

                    arg_idx += 1;
                }
                _ => break,
            }

            i += 1;
        }

        data.push(types::MessageMagic::End as u8);

        let mut msg = generic_protocol_message::GenericProtocolMessage::new(data, fds);

        if self.id() == 0 && !self.server() {
            trace! {
                if let Some(client) = self.client_sock() {
                    crate::log_debug!("[hw] trace: [{} @ {:.3}] -- call: waiting on object of type {}", client.0.state.stream.as_raw_fd(), steady_millis(), method_returns_type);
                }
            }

            let protocol_name = self.protocol_name();
            msg.set_depends_on_seq(self.seq());
            if let Some(client) = self.client_sock() {
                client.0.pending_outgoing.borrow_mut().push(msg);
                if return_seq != 0 {
                    client
                        .0
                        .make_object(protocol_name, method_returns_type, return_seq)?;
                    return Ok(return_seq);
                }
            }
        } else {
            self.send_message(&msg);
            if return_seq != 0 {
                let protocol_name = self.protocol_name();
                if let Some(client) = self.client_sock() {
                    client
                        .0
                        .make_object(protocol_name, method_returns_type, return_seq)?;
                    return Ok(return_seq);
                }
            }
        }

        Ok(0)
    }
}
