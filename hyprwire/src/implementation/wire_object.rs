use super::types;
use crate::implementation::object;
use crate::server::server_object;
use crate::{message, steady_millis, trace};
use libffi::low as ffi;
use std::os::fd::AsRawFd;
use std::os::raw;
use std::{any, ptr};

pub trait WireObject: object::Object {
    fn set_version(&self, version: u32);

    fn version(&self) -> u32;

    fn listener(&self, idx: usize) -> *mut raw::c_void;

    fn listener_count(&self) -> usize;

    fn methods_out(&self) -> &[types::Method];

    fn methods_in(&self) -> &[types::Method];

    fn errd(&self);

    fn mark_destroyed(&self) {}

    fn on_destructor(&self) {}

    fn send_message(&self, msg: &dyn message::Message);

    fn protocol_name(&self) -> &str;

    fn server(&self) -> bool;

    fn id(&self) -> u32;

    fn seq(&self) -> u32;

    fn called<D>(
        &self,
        id: u32,
        data: &[u8],
        fds: &[i32],
        dispatch: &mut D,
    ) -> Result<(), message::Error> {
        let methods = self.methods_in();

        if methods.len() <= id as usize {
            let msg = format!("invalid method {} for object {}", id, self.id());
            crate::log_error!("core protocol error: {msg}");
            self.error(self.id(), &msg);
            return Err(message::Error::InvalidMethod);
        }

        let method = &methods[id as usize];

        if self.listener_count() <= id as usize || self.listener(id as usize).is_null() {
            self.destroy_if_needed(method);
            return Ok(());
        }

        if self.listener(id as usize).is_null() {
            if method.destructor {
                self.on_destructor();
            }
            return Ok(());
        }
        let mut params: Vec<u8> = Vec::new();

        if !method.returns_type.is_empty() {
            params.push(types::MessageMagic::TypeSeq as u8);
        }

        params.extend_from_slice(method.params);

        if method.since > self.version() {
            let msg = format!(
                "method {} since {} but has {}",
                id,
                method.since,
                self.version()
            );
            crate::log_error!("core protocol error: {msg}");
            self.error(self.id(), &msg);
            return Err(message::Error::ProtocolVersionTooLow);
        }

        let mut ffi_types: Vec<*mut ffi::ffi_type> = Vec::new();
        // Prepend data pointer type so trampolines receive user data as first arg
        ffi_types.push(&raw mut ffi::types::pointer);

        let mut data_idx: usize = 0;
        let mut i: usize = 0;
        while i < params.len() {
            let param = types::MessageMagic::try_from(params[i])?;
            let wire_param = types::MessageMagic::try_from(data[data_idx])?;

            if param != wire_param {
                let msg =
                    format!("method {id} param idx {i} should be {param:?} but was {wire_param:?}");
                crate::log_error!("core protocol error: {msg}");
                self.error(self.id(), &msg);
                return Err(message::Error::InvalidParameter);
            }

            ffi_types.push(param.to_ffi_type());

            match param {
                types::MessageMagic::End => i += 1, // BUG if this happens or malformed message
                types::MessageMagic::TypeFd => data_idx += 1,
                types::MessageMagic::TypeUint
                | types::MessageMagic::TypeF32
                | types::MessageMagic::TypeInt
                | types::MessageMagic::TypeObject
                | types::MessageMagic::TypeSeq => data_idx += 5,
                types::MessageMagic::TypeVarchar => {
                    let (str_len, var_int_len) = message::parse_var_int(data, data_idx + 1);
                    data_idx += str_len + var_int_len + 1;
                }
                types::MessageMagic::TypeArray => {
                    i += 1;
                    let arr_type = types::MessageMagic::try_from(params[i])?;
                    let wire_type = types::MessageMagic::try_from(data[data_idx + 1])?;

                    if arr_type != wire_type {
                        // raise protocol error
                        let msg = format!(
                            "method {id} param idx {i} should be {arr_type:?} but was {wire_type:?}"
                        );
                        crate::log_error!("core protocol error: {msg}");
                        self.error(self.id(), &msg);
                        return Err(message::Error::IncorrectParamIdx);
                    }

                    let (arr_len, len_len) = message::parse_var_int(data, data_idx + 2);
                    let mut arr_message_len: usize = 2 + len_len;

                    if arr_len > 10000 {
                        let msg =
                            format!("method {id} param idx {i} max array size of 10000 exceeded",);
                        crate::log_debug!("core protocol error: {msg}",);
                        self.error(self.id(), &msg);
                        return Err(message::Error::ArrayTooLong);
                    }

                    ffi_types.push(types::MessageMagic::TypeUint.to_ffi_type());

                    match arr_type {
                        types::MessageMagic::TypeUint
                        | types::MessageMagic::TypeF32
                        | types::MessageMagic::TypeInt
                        | types::MessageMagic::TypeObject
                        | types::MessageMagic::TypeSeq => arr_message_len += 4 * arr_len,
                        types::MessageMagic::TypeVarchar => {
                            for _ in 0..arr_len {
                                if data_idx + arr_message_len > data.len() {
                                    let msg = "failed demarshaling array message";
                                    crate::log_error!("core protocol error: {msg}");
                                    self.error(self.id(), msg);
                                    return Err(message::Error::DemarshalingFailed);
                                }

                                let (str_len, str_len_len) =
                                    message::parse_var_int(data, data_idx + arr_message_len);
                                arr_message_len += str_len + str_len_len;
                            }
                        }
                        types::MessageMagic::TypeFd => {}
                        _ => {
                            let msg = "failed demarshaling array message";
                            crate::log_error!("core protocol error: {msg}");
                            self.error(self.id(), msg);
                            return Err(message::Error::DemarshalingFailed);
                        }
                    }

                    data_idx += arr_message_len;
                }
                types::MessageMagic::TypeObjectId => {
                    let msg = "object type is not implemented";
                    crate::log_error!("core protocol error: {msg}");
                    self.error(self.id(), msg);
                    return Err(message::Error::Unimplemented);
                }
            }

            i += 1;
        }

        let mut cif = ffi::ffi_cif::default();
        unsafe {
            if ffi::prep_cif(
                &raw mut cif,
                ffi::ffi_abi_FFI_DEFAULT_ABI,
                ffi_types.len(),
                &raw mut libffi::raw::ffi_type_void,
                ffi_types.as_mut_ptr(),
            )
            .is_err()
            {
                crate::log_error!("core protocol error: ffi failed");
                self.errd();
                return Ok(());
            }
        }

        let mut avalues: Vec<*mut raw::c_void> = Vec::with_capacity(ffi_types.len());
        let mut other_buffers: Vec<Vec<u8>> = Vec::new();
        let mut strings: Vec<Vec<u8>> = Vec::new();
        let mut fd_no: usize = 0;

        // Prepend the per-call dispatch context so trampolines can access both
        // the object and the current dispatch target without TLS.
        let object_data = unsafe { &*(self.get_data() as *const crate::DispatchData) };
        let call_ctx = crate::DispatchContext {
            object: object_data.object,
            dispatch: ptr::from_mut(dispatch),
        };
        let mut data_ptr_slot = vec![0u8; std::mem::size_of::<*mut raw::c_void>()];
        data_ptr_slot.copy_from_slice(&((&raw const call_ctx) as usize).to_ne_bytes());
        avalues.push(data_ptr_slot.as_mut_ptr().cast::<raw::c_void>());
        other_buffers.push(data_ptr_slot);

        let mut i: usize = 0;
        while i < data.len() {
            let mut buf: Option<*mut raw::c_void> = None;
            let param = types::MessageMagic::try_from(data[i])?;

            match param {
                types::MessageMagic::End => break,
                types::MessageMagic::TypeUint
                | types::MessageMagic::TypeObject
                | types::MessageMagic::TypeSeq => {
                    let mut storage = vec![0u8; std::mem::size_of::<u32>()];
                    storage.copy_from_slice(&data[i + 1..i + 1 + std::mem::size_of::<u32>()]);
                    buf = Some(storage.as_mut_ptr().cast::<raw::c_void>());
                    other_buffers.push(storage);
                    i += std::mem::size_of::<u32>();
                }
                types::MessageMagic::TypeF32 => {
                    let mut storage = vec![0u8; std::mem::size_of::<f32>()];
                    storage.copy_from_slice(&data[i + 1..i + 1 + std::mem::size_of::<f32>()]);
                    buf = Some(storage.as_mut_ptr().cast::<raw::c_void>());
                    other_buffers.push(storage);
                    i += std::mem::size_of::<f32>();
                }
                types::MessageMagic::TypeInt => {
                    let mut storage = vec![0u8; std::mem::size_of::<i32>()];
                    storage.copy_from_slice(&data[i + 1..i + 1 + std::mem::size_of::<i32>()]);
                    buf = Some(storage.as_mut_ptr().cast::<raw::c_void>());
                    other_buffers.push(storage);
                    i += std::mem::size_of::<i32>();
                }
                types::MessageMagic::TypeVarchar => {
                    let (str_len, len_len) = message::parse_var_int(data, i + 1);
                    let str_bytes = &data[i + 1 + len_len..i + 1 + len_len + str_len];

                    let mut owned_str = Vec::with_capacity(str_len + 1);
                    owned_str.extend_from_slice(str_bytes);
                    owned_str.push(0); // null terminator
                    strings.push(owned_str);

                    let str_ptr = strings.last().unwrap().as_ptr();
                    let mut slot = vec![0u8; std::mem::size_of::<*const u8>()];
                    slot.copy_from_slice(&(str_ptr as usize).to_ne_bytes());
                    buf = Some(slot.as_mut_ptr().cast::<raw::c_void>());
                    other_buffers.push(slot);

                    i += str_len + len_len;
                }
                types::MessageMagic::TypeArray => {
                    let arr_type = types::MessageMagic::try_from(data[i + 1])?;
                    let (arr_len, len_len) = message::parse_var_int(data, i + 2);
                    let mut arr_message_len: usize = 2 + len_len;

                    match arr_type {
                        types::MessageMagic::TypeUint
                        | types::MessageMagic::TypeF32
                        | types::MessageMagic::TypeInt
                        | types::MessageMagic::TypeObject
                        | types::MessageMagic::TypeSeq => {
                            let elem_size = std::mem::size_of::<u32>();
                            let alloc_len = if arr_len == 0 { 1 } else { arr_len };
                            let mut data_buf = vec![0u8; alloc_len * elem_size];

                            for j in 0..arr_len {
                                let src =
                                    &data[i + arr_message_len..i + arr_message_len + elem_size];
                                data_buf[j * elem_size..(j + 1) * elem_size].copy_from_slice(src);
                                arr_message_len += elem_size;
                            }

                            let data_ptr = data_buf.as_mut_ptr();
                            other_buffers.push(data_buf);

                            let mut data_slot = vec![0u8; std::mem::size_of::<*mut u8>()];
                            data_slot.copy_from_slice(&(data_ptr as usize).to_ne_bytes());
                            avalues.push(data_slot.as_mut_ptr().cast::<raw::c_void>());
                            other_buffers.push(data_slot);

                            let mut size_slot = vec![0u8; std::mem::size_of::<u32>()];
                            #[allow(clippy::cast_possible_truncation)]
                            let arr_len_u32 = arr_len as u32;
                            size_slot.copy_from_slice(&arr_len_u32.to_le_bytes());
                            avalues.push(size_slot.as_mut_ptr().cast::<raw::c_void>());
                            other_buffers.push(size_slot);
                        }
                        types::MessageMagic::TypeVarchar => {
                            let alloc_len = if arr_len == 0 { 1 } else { arr_len };
                            let ptr_size = std::mem::size_of::<*const u8>();
                            let mut data_buf = vec![0u8; alloc_len * ptr_size];

                            for j in 0..arr_len {
                                let (str_len, strlen_len) =
                                    message::parse_var_int(data, i + arr_message_len);
                                let str_data = &data[i + arr_message_len + strlen_len
                                    ..i + arr_message_len + strlen_len + str_len];

                                let mut owned_str = Vec::with_capacity(str_data.len() + 1);
                                owned_str.extend_from_slice(str_data);
                                owned_str.push(0);
                                let str_ptr = owned_str.as_ptr() as usize;
                                strings.push(owned_str);

                                data_buf[j * ptr_size..(j + 1) * ptr_size]
                                    .copy_from_slice(&str_ptr.to_ne_bytes());

                                arr_message_len += strlen_len + str_len;
                            }

                            let data_ptr = data_buf.as_mut_ptr();
                            other_buffers.push(data_buf);

                            let mut data_slot = vec![0u8; std::mem::size_of::<*mut u8>()];
                            data_slot.copy_from_slice(&(data_ptr as usize).to_ne_bytes());
                            avalues.push(data_slot.as_mut_ptr().cast::<raw::c_void>());
                            other_buffers.push(data_slot);

                            let mut size_slot = vec![0u8; std::mem::size_of::<u32>()];
                            #[allow(clippy::cast_possible_truncation)]
                            let arr_len_u32 = arr_len as u32;
                            size_slot.copy_from_slice(&arr_len_u32.to_le_bytes());
                            avalues.push(size_slot.as_mut_ptr().cast::<raw::c_void>());
                            other_buffers.push(size_slot);
                        }
                        types::MessageMagic::TypeFd => {
                            let alloc_len = if arr_len == 0 { 1 } else { arr_len };
                            let elem_size = std::mem::size_of::<i32>();
                            let mut data_buf = vec![0u8; alloc_len * elem_size];

                            for j in 0..arr_len {
                                if fd_no >= fds.len() {
                                    let msg = "failed demarshaling array message";
                                    crate::log_error!("core protocol error: {msg}");
                                    self.error(self.id(), msg);
                                    return Err(message::Error::DemarshalingFailed);
                                }
                                data_buf[j * elem_size..(j + 1) * elem_size]
                                    .copy_from_slice(&fds[fd_no].to_le_bytes());
                                fd_no += 1;
                            }

                            let data_ptr = data_buf.as_mut_ptr();
                            other_buffers.push(data_buf);

                            let mut data_slot = vec![0u8; std::mem::size_of::<*mut u8>()];
                            data_slot.copy_from_slice(&(data_ptr as usize).to_ne_bytes());
                            avalues.push(data_slot.as_mut_ptr().cast::<raw::c_void>());
                            other_buffers.push(data_slot);

                            let mut size_slot = vec![0u8; std::mem::size_of::<u32>()];
                            #[allow(clippy::cast_possible_truncation)]
                            let arr_len_u32 = arr_len as u32;
                            size_slot.copy_from_slice(&arr_len_u32.to_le_bytes());
                            avalues.push(size_slot.as_mut_ptr().cast::<raw::c_void>());
                            other_buffers.push(size_slot);
                        }
                        _ => {
                            let msg = "failed demarshaling array message";
                            crate::log_error!("core protocol error: {msg}");
                            self.error(self.id(), msg);
                            return Err(message::Error::DemarshalingFailed);
                        }
                    }

                    i += arr_message_len - 1; // loop does += 1
                }
                types::MessageMagic::TypeObjectId => {
                    let msg = "object type is not implemented";
                    crate::log_error!("core protocol error: {msg}");
                    self.error(self.id(), msg);
                    return Err(message::Error::Unimplemented);
                }
                types::MessageMagic::TypeFd => {
                    if fd_no >= fds.len() {
                        let msg = "failed demarshaling fd";
                        crate::log_error!("core protocol error: {msg}");
                        self.error(self.id(), msg);
                        return Err(message::Error::DemarshalingFailed);
                    }
                    let mut storage = vec![0u8; std::mem::size_of::<i32>()];
                    storage.copy_from_slice(&fds[fd_no].to_le_bytes());
                    fd_no += 1;
                    buf = Some(storage.as_mut_ptr().cast::<raw::c_void>());
                    other_buffers.push(storage);
                }
            }

            if let Some(b) = buf {
                avalues.push(b);
            }

            i += 1;
        }

        let listener = self.listener(id as usize);
        unsafe {
            ffi::call::<()>(
                &raw mut cif,
                libffi::high::CodePtr::from_ptr(listener),
                avalues.as_mut_ptr(),
            );
        };

        self.destroy_if_needed(method);

        Ok(())
    }

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

        let mut msg = message::GenericProtocolMessage::new(data, fds);

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

    fn destroy_if_needed(&self, method: &types::Method) {
        if !method.destructor {
            return;
        }

        self.mark_destroyed();

        if self.id() == 0 {
            return;
        }

        if self.server() {
            if let Some(server_object) = self.as_any().downcast_ref::<server_object::ServerObject>()
                && let Some(client) = server_object.client.upgrade()
            {
                client.destroy_object(self.id());
            }

            return;
        }

        if let Some(client) = self.client_sock() {
            client.0.destroy_object(self.id());
        }
    }

    fn as_any(&self) -> &dyn any::Any;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell;

    thread_local! {
        static DESTRUCTOR_CALLS: cell::Cell<usize> = const { cell::Cell::new(0) };
    }

    extern "C" fn on_noop(_: *mut raw::c_void) {}
    extern "C" fn on_destructor(_: *mut raw::c_void) {
        DESTRUCTOR_CALLS.with(|c| c.set(c.get() + 1));
    }

    #[derive(Default)]
    struct TestWireObject {
        id: cell::Cell<u32>,
        version: cell::Cell<u32>,
        seq: u32,
        listeners: cell::RefCell<Vec<*mut raw::c_void>>,
        server: bool,
        data: cell::Cell<*mut raw::c_void>,
        methods_in: Vec<types::Method>,
        methods_out: Vec<types::Method>,
        protocol_name: &'static str,
        last_error_id: cell::Cell<u32>,
        last_error_msg: cell::RefCell<String>,
        errd_called: cell::Cell<bool>,
        destroyed: cell::Cell<bool>,
        last_sent_data: cell::RefCell<Vec<u8>>,
    }

    impl object::Object for TestWireObject {
        fn call(&self, _id: u32, _args: &[types::CallArg]) -> u32 {
            0
        }

        fn listen(&self, id: u32, func: *mut raw::c_void) {
            let mut listeners = self.listeners.borrow_mut();
            let idx = id as usize;
            if idx >= listeners.len() {
                listeners.resize(idx + 1, ptr::null_mut());
            }
            listeners[idx] = func;
        }

        fn set_data(
            &self,
            data: *mut raw::c_void,
            _destructor: Option<unsafe fn(*mut raw::c_void)>,
        ) {
            self.data.set(data);
        }

        fn get_data(&self) -> *mut raw::c_void {
            self.data.get()
        }

        fn error(&self, error_id: u32, error_msg: &str) {
            self.last_error_id.set(error_id);
            *self.last_error_msg.borrow_mut() = error_msg.to_string();
        }
    }

    impl WireObject for TestWireObject {
        fn set_version(&self, version: u32) {
            self.version.set(version);
        }

        fn version(&self) -> u32 {
            self.version.get()
        }

        fn id(&self) -> u32 {
            self.id.get()
        }

        fn seq(&self) -> u32 {
            self.seq
        }

        fn listener(&self, idx: usize) -> *mut raw::c_void {
            self.listeners.borrow()[idx]
        }

        fn listener_count(&self) -> usize {
            self.listeners.borrow().len()
        }

        fn methods_out(&self) -> &[types::Method] {
            &self.methods_out
        }

        fn methods_in(&self) -> &[types::Method] {
            &self.methods_in
        }

        fn errd(&self) {
            self.errd_called.set(true);
        }

        fn send_message(&self, msg: &dyn message::Message) {
            self.last_sent_data
                .borrow_mut()
                .extend_from_slice(msg.data());
        }

        fn protocol_name(&self) -> &str {
            self.protocol_name
        }

        fn server(&self) -> bool {
            self.server
        }

        fn mark_destroyed(&self) {
            self.destroyed.set(true);
        }

        fn as_any(&self) -> &dyn any::Any {
            self
        }
    }

    #[test]
    fn call_rejects_invalid_method_index() {
        let obj = TestWireObject {
            server: true,
            id: cell::Cell::new(77),
            ..Default::default()
        };

        assert!(obj.call(0, &[]).is_ok_and(|res| res == 0));
        assert!(obj.last_error_msg.borrow().contains("invalid method"));
    }

    #[test]
    fn call_rejects_method_since_newer_than_object_version() {
        let obj = TestWireObject {
            server: true,
            id: cell::Cell::new(55),
            version: cell::Cell::new(1),
            methods_out: vec![types::Method {
                idx: 0,
                params: &[],
                returns_type: "",
                since: 3,
                destructor: false,
            }],
            ..Default::default()
        };

        assert!(obj.call(0, &[]).is_ok_and(|res| res == 0));
        assert!(obj.last_error_msg.borrow().contains("since"));
    }

    #[test]
    fn server_side_call_rejects_returns_type_methods() {
        let obj = TestWireObject {
            server: true,
            id: cell::Cell::new(5),
            methods_out: vec![types::Method {
                idx: 0,
                params: &[],
                returns_type: "child",
                since: 0,
                destructor: false,
            }],
            ..Default::default()
        };

        assert!(obj.call(0, &[]).is_ok_and(|res| res == 0));
        assert!(
            obj.last_error_msg
                .borrow()
                .contains("server cannot call returnsType methods")
        );
    }

    #[test]
    fn call_fails_for_unsupported_array_element_type() {
        let obj = TestWireObject {
            server: true,
            id: cell::Cell::new(12),
            methods_out: vec![types::Method {
                idx: 0,
                params: &[
                    types::MessageMagic::TypeArray as u8,
                    types::MessageMagic::TypeObjectId as u8,
                ],
                returns_type: "",
                since: 0,
                destructor: false,
            }],
            ..Default::default()
        };

        assert!(obj.call(0, &[]).is_ok_and(|r| r == 0));
        assert!(obj.errd_called.get());
    }

    #[test]
    fn call_marks_object_destroyed_when_destructor_method_is_called() {
        let obj = TestWireObject {
            server: true,
            id: cell::Cell::new(44),
            methods_out: vec![types::Method {
                idx: 0,
                params: &[],
                returns_type: "",
                since: 0,
                destructor: true,
            }],
            ..Default::default()
        };

        assert!(!obj.destroyed.get());
        assert!(obj.call(0, &[]).is_ok_and(|r| r == 0));
        assert!(obj.destroyed.get());
        let sent = obj.last_sent_data.borrow();
        assert!(!sent.is_empty());
        assert_eq!(sent[0], message::MessageType::GenericProtocolMessage as u8);
    }

    #[test]
    fn called_rejects_invalid_method_index() {
        let obj = TestWireObject {
            id: cell::Cell::new(91),
            ..Default::default()
        };

        let data = [types::MessageMagic::End as u8];
        let mut dispatch = ();
        let _ = obj.called(0, &data, &[], &mut dispatch);

        assert!(obj.last_error_msg.borrow().contains("invalid method"));
    }

    #[test]
    fn called_rejects_method_since_newer_than_object_version() {
        let obj = TestWireObject {
            id: cell::Cell::new(77),
            version: cell::Cell::new(1),
            methods_in: vec![types::Method {
                idx: 0,
                params: &[],
                returns_type: "",
                since: 2,
                destructor: false,
            }],
            ..Default::default()
        };
        object::Object::listen(&obj, 0, on_noop as *mut raw::c_void);

        let data = [types::MessageMagic::End as u8];
        let mut dispatch = ();
        let _ = obj.called(0, &data, &[], &mut dispatch);

        assert!(obj.last_error_msg.borrow().contains("since"));
    }

    #[test]
    fn called_rejects_type_mismatch_between_spec_and_wire() {
        let obj = TestWireObject {
            id: cell::Cell::new(73),
            methods_in: vec![types::Method {
                idx: 0,
                params: &[types::MessageMagic::TypeUint as u8],
                returns_type: "",
                since: 0,
                destructor: false,
            }],
            ..Default::default()
        };
        object::Object::listen(&obj, 0, on_noop as *mut raw::c_void);

        let data = [
            types::MessageMagic::TypeInt as u8,
            0,
            0,
            0,
            0,
            types::MessageMagic::End as u8,
        ];
        let mut dispatch = ();
        let _ = obj.called(0, &data, &[], &mut dispatch);

        assert!(obj.last_error_msg.borrow().contains("should be"));
    }

    #[test]
    fn called_rejects_array_wire_type_mismatch() {
        let obj = TestWireObject {
            id: cell::Cell::new(12),
            methods_in: vec![types::Method {
                idx: 0,
                params: &[
                    types::MessageMagic::TypeArray as u8,
                    types::MessageMagic::TypeUint as u8,
                ],
                returns_type: "",
                since: 0,
                destructor: false,
            }],
            ..Default::default()
        };
        object::Object::listen(&obj, 0, on_noop as *mut raw::c_void);

        let data = [
            types::MessageMagic::TypeArray as u8,
            types::MessageMagic::TypeInt as u8,
            0x00,
            types::MessageMagic::End as u8,
        ];
        let mut dispatch = ();
        let _ = obj.called(0, &data, &[], &mut dispatch);

        assert!(obj.last_error_msg.borrow().contains("should be"));
    }

    #[test]
    fn called_rejects_oversized_array_payload() {
        let obj = TestWireObject {
            id: cell::Cell::new(88),
            methods_in: vec![types::Method {
                idx: 0,
                params: &[
                    types::MessageMagic::TypeArray as u8,
                    types::MessageMagic::TypeUint as u8,
                ],
                returns_type: "",
                since: 0,
                destructor: false,
            }],
            ..Default::default()
        };
        object::Object::listen(&obj, 0, on_noop as *mut raw::c_void);

        // varint encoding of 10001: [0x91, 0x4E]
        let data = [
            types::MessageMagic::TypeArray as u8,
            types::MessageMagic::TypeUint as u8,
            0x91,
            0x4E,
            types::MessageMagic::End as u8,
        ];
        let mut dispatch = ();
        let _ = obj.called(0, &data, &[], &mut dispatch);

        assert!(obj.last_error_msg.borrow().contains("max array size"));
    }

    #[test]
    fn called_rejects_object_id_magic_type() {
        let obj = TestWireObject {
            id: cell::Cell::new(19),
            methods_in: vec![types::Method {
                idx: 0,
                params: &[types::MessageMagic::TypeObjectId as u8],
                returns_type: "",
                since: 0,
                destructor: false,
            }],
            ..Default::default()
        };
        object::Object::listen(&obj, 0, on_noop as *mut raw::c_void);

        let data = [types::MessageMagic::TypeObjectId as u8];
        let mut dispatch = ();
        let _ = obj.called(0, &data, &[], &mut dispatch);

        assert!(
            obj.last_error_msg
                .borrow()
                .contains("object type is not impl")
        );
    }

    #[test]
    fn called_marks_destroyed_for_destructor_without_listener() {
        let obj = TestWireObject {
            id: cell::Cell::new(0),
            methods_in: vec![types::Method {
                idx: 0,
                params: &[],
                returns_type: "",
                since: 0,
                destructor: true,
            }],
            ..Default::default()
        };

        let data = [types::MessageMagic::End as u8];
        let mut dispatch = ();
        let _ = obj.called(0, &data, &[], &mut dispatch);

        assert!(obj.destroyed.get());
    }

    #[test]
    fn called_marks_destroyed_for_destructor_with_listener() {
        let obj = TestWireObject {
            id: cell::Cell::new(0),
            methods_in: vec![types::Method {
                idx: 0,
                params: &[],
                returns_type: "",
                since: 0,
                destructor: true,
            }],
            ..Default::default()
        };

        let dispatch_data = crate::DispatchData {
            object: ptr::null::<TestWireObject>() as *const dyn object::Object,
        };
        object::Object::set_data(&obj, &raw const dispatch_data as *mut raw::c_void, None);
        object::Object::listen(&obj, 0, on_destructor as *mut raw::c_void);

        DESTRUCTOR_CALLS.with(|c| c.set(0));

        let data = [types::MessageMagic::End as u8];
        let mut dispatch = ();
        let _ = obj.called(0, &data, &[], &mut dispatch);

        assert_eq!(DESTRUCTOR_CALLS.with(|c| c.get()), 1);
        assert!(obj.destroyed.get());
    }
}
