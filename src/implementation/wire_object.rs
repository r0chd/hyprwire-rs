use super::types;
use crate::implementation::object;
use crate::message;
use crate::message::MessageError;
use libffi::low as ffi;
use std::os::raw;
use types::MessageMagic;

pub trait WireObject: object::Object {
    fn version(&self) -> u32;

    fn listeners(&self) -> &[*mut raw::c_void];

    fn methods_out(&self) -> &[types::Method];

    fn methods_in(&self) -> &[types::Method];

    fn errd(&mut self);

    fn send_message(&mut self, msg: &dyn message::Message);

    fn server(&self) -> bool;

    fn id(&self) -> u32;

    fn called(&mut self, id: u32, data: &[u8], fds: &[i32]) -> Result<(), MessageError> {
        let methods = self.methods_in();

        if methods.len() <= id as usize {
            let msg = format!("invalid method {} for object {}", id, self.id());
            log::debug!("core protocol error: {msg}");
            self.error(self.id(), &msg);
            return Err(MessageError::InvalidMethod);
        }

        if self.listeners().len() <= id as usize {
            return Ok(());
        }

        let method = &methods[id as usize];
        let mut params: Vec<u8> = Vec::new();

        if !method.returns_type.is_empty() {
            params.push(MessageMagic::TypeSeq as u8);
        }

        params.extend_from_slice(method.params.as_bytes());

        if method.since > self.version() {
            let msg = format!(
                "method {} since {} but has {}",
                id,
                method.since,
                self.version()
            );
            log::debug!("core protocol error: {}", msg);
            self.error(self.id(), &msg);
            return Err(MessageError::ProtocolVersionTooLow);
        }

        let mut ffi_types: Vec<*mut ffi::ffi_type> = Vec::new();

        let mut data_idx: usize = 0;
        let mut i: usize = 0;
        while i < params.len() {
            let param = MessageMagic::try_from(params[i])?;
            let wire_param = MessageMagic::try_from(data[data_idx])?;

            if param != wire_param {
                let msg = format!(
                    "method {} param idx {} should be {:?} but was {:?}",
                    id, i, param, wire_param
                );
                log::debug!("core protocol error: {msg}");
                self.error(self.id(), &msg);
                return Err(MessageError::InvalidParameter);
            }

            ffi_types.push(param.to_ffi_type());

            match param {
                MessageMagic::End => i += 1, // BUG if this happens or malformed message
                MessageMagic::TypeFd => data_idx += 1,
                MessageMagic::TypeUint
                | MessageMagic::TypeF32
                | MessageMagic::TypeInt
                | MessageMagic::TypeObject
                | MessageMagic::TypeSeq => data_idx += 5,
                MessageMagic::TypeVarchar => {
                    let (str_len, var_int_len) = message::parse_var_int(data, data_idx + 1);
                    data_idx += str_len + var_int_len + 1;
                }
                MessageMagic::TypeArray => {
                    i += 1;
                    let arr_type = MessageMagic::try_from(params[i])?;
                    let wire_type = MessageMagic::try_from(data[data_idx + 1])?;

                    if arr_type != wire_type {
                        // raise protocol error
                        let msg = format!(
                            "method {} param idx {} should be {:?} but was {:?}",
                            id, i, arr_type, wire_type
                        );
                        log::debug!("core protocol error: {msg}");
                        self.error(self.id(), &msg);
                        return Err(MessageError::IncorrectParamIdx);
                    }

                    let (arr_len, len_len) = message::parse_var_int(data, data_idx + 2);
                    let mut arr_message_len: usize = 2 + len_len;

                    ffi_types.push(MessageMagic::TypeUint.to_ffi_type());

                    match arr_type {
                        MessageMagic::TypeUint
                        | MessageMagic::TypeF32
                        | MessageMagic::TypeInt
                        | MessageMagic::TypeObject
                        | MessageMagic::TypeSeq => arr_message_len += 4 * arr_len,
                        MessageMagic::TypeVarchar => {
                            for _ in 0..arr_len {
                                if data_idx + arr_message_len > data.len() {
                                    let msg = "failed demarshaling array message";
                                    log::debug!("core protocol error: {msg}");
                                    self.error(self.id(), msg);
                                    return Err(MessageError::DemarshalingFailed);
                                }

                                let (str_len, str_len_len) =
                                    message::parse_var_int(data, data_idx + arr_message_len);
                                arr_message_len += str_len + str_len_len;
                            }
                        }
                        MessageMagic::TypeFd => {}
                        _ => {
                            let msg = "failed demarshaling array message";
                            log::debug!("core protocol error: {msg}");
                            self.error(self.id(), msg);
                            return Err(MessageError::DemarshalingFailed);
                        }
                    }

                    data_idx += arr_message_len;
                }
                MessageMagic::TypeObjectId => {
                    let msg = "object type is not implemented";
                    log::debug!("core protocol error: {msg}");
                    self.error(self.id(), msg);
                    return Err(MessageError::Unimplemented);
                }
            }

            i += 1;
        }

        let mut cif = ffi::ffi_cif::default();
        unsafe {
            if ffi::prep_cif(
                &mut cif,
                ffi::ffi_abi_FFI_DEFAULT_ABI,
                ffi_types.len(),
                &raw mut libffi::raw::ffi_type_void,
                ffi_types.as_mut_ptr(),
            )
            .is_err()
            {
                log::debug!("core protocol error: ffi failed");
                self.errd();
                return Ok(());
            }
        }

        let mut avalues: Vec<*mut raw::c_void> = Vec::with_capacity(ffi_types.len());
        let mut other_buffers: Vec<Vec<u8>> = Vec::new();
        let mut strings: Vec<Vec<u8>> = Vec::new();
        let mut fd_no: usize = 0;

        let mut i: usize = 0;
        while i < data.len() {
            let mut buf: Option<*mut raw::c_void> = None;
            let param = MessageMagic::try_from(data[i])?;

            match param {
                MessageMagic::End => break,
                MessageMagic::TypeUint | MessageMagic::TypeObject | MessageMagic::TypeSeq => {
                    let mut storage = vec![0u8; std::mem::size_of::<u32>()];
                    storage.copy_from_slice(&data[i + 1..i + 1 + std::mem::size_of::<u32>()]);
                    buf = Some(storage.as_mut_ptr() as *mut raw::c_void);
                    other_buffers.push(storage);
                    i += std::mem::size_of::<u32>();
                }
                MessageMagic::TypeF32 => {
                    let mut storage = vec![0u8; std::mem::size_of::<f32>()];
                    storage.copy_from_slice(&data[i + 1..i + 1 + std::mem::size_of::<f32>()]);
                    buf = Some(storage.as_mut_ptr() as *mut raw::c_void);
                    other_buffers.push(storage);
                    i += std::mem::size_of::<f32>();
                }
                MessageMagic::TypeInt => {
                    let mut storage = vec![0u8; std::mem::size_of::<i32>()];
                    storage.copy_from_slice(&data[i + 1..i + 1 + std::mem::size_of::<i32>()]);
                    buf = Some(storage.as_mut_ptr() as *mut raw::c_void);
                    other_buffers.push(storage);
                    i += std::mem::size_of::<i32>();
                }
                MessageMagic::TypeVarchar => {
                    let (str_len, len_len) = message::parse_var_int(data, i + 1);
                    let str_bytes = &data[i + 1 + len_len..i + 1 + len_len + str_len];

                    let mut owned_str = Vec::with_capacity(str_len + 1);
                    owned_str.extend_from_slice(str_bytes);
                    owned_str.push(0); // null terminator
                    strings.push(owned_str);

                    let str_ptr = strings.last().unwrap().as_ptr();
                    let mut slot = vec![0u8; std::mem::size_of::<*const u8>()];
                    slot.copy_from_slice(&(str_ptr as usize).to_ne_bytes());
                    buf = Some(slot.as_mut_ptr() as *mut raw::c_void);
                    other_buffers.push(slot);

                    i += str_len + len_len;
                }
                MessageMagic::TypeArray => {
                    let arr_type = MessageMagic::try_from(data[i + 1])?;
                    let (arr_len, len_len) = message::parse_var_int(data, i + 2);
                    let mut arr_message_len: usize = 2 + len_len;

                    match arr_type {
                        MessageMagic::TypeUint
                        | MessageMagic::TypeF32
                        | MessageMagic::TypeInt
                        | MessageMagic::TypeObject
                        | MessageMagic::TypeSeq => {
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
                            avalues.push(data_slot.as_mut_ptr() as *mut raw::c_void);
                            other_buffers.push(data_slot);

                            let mut size_slot = vec![0u8; std::mem::size_of::<u32>()];
                            size_slot.copy_from_slice(&(arr_len as u32).to_le_bytes());
                            avalues.push(size_slot.as_mut_ptr() as *mut raw::c_void);
                            other_buffers.push(size_slot);
                        }
                        MessageMagic::TypeVarchar => {
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
                            avalues.push(data_slot.as_mut_ptr() as *mut raw::c_void);
                            other_buffers.push(data_slot);

                            let mut size_slot = vec![0u8; std::mem::size_of::<u32>()];
                            size_slot.copy_from_slice(&(arr_len as u32).to_le_bytes());
                            avalues.push(size_slot.as_mut_ptr() as *mut raw::c_void);
                            other_buffers.push(size_slot);
                        }
                        MessageMagic::TypeFd => {
                            let alloc_len = if arr_len == 0 { 1 } else { arr_len };
                            let elem_size = std::mem::size_of::<i32>();
                            let mut data_buf = vec![0u8; alloc_len * elem_size];

                            for j in 0..arr_len {
                                if fd_no >= fds.len() {
                                    let msg = "failed demarshaling array message";
                                    log::debug!("core protocol error: {msg}");
                                    self.error(self.id(), msg);
                                    return Err(MessageError::DemarshalingFailed);
                                }
                                data_buf[j * elem_size..(j + 1) * elem_size]
                                    .copy_from_slice(&fds[fd_no].to_le_bytes());
                                fd_no += 1;
                            }

                            let data_ptr = data_buf.as_mut_ptr();
                            other_buffers.push(data_buf);

                            let mut data_slot = vec![0u8; std::mem::size_of::<*mut u8>()];
                            data_slot.copy_from_slice(&(data_ptr as usize).to_ne_bytes());
                            avalues.push(data_slot.as_mut_ptr() as *mut raw::c_void);
                            other_buffers.push(data_slot);

                            let mut size_slot = vec![0u8; std::mem::size_of::<u32>()];
                            size_slot.copy_from_slice(&(arr_len as u32).to_le_bytes());
                            avalues.push(size_slot.as_mut_ptr() as *mut raw::c_void);
                            other_buffers.push(size_slot);
                        }
                        _ => {
                            let msg = "failed demarshaling array message";
                            log::debug!("core protocol error: {msg}");
                            self.error(self.id(), msg);
                            return Err(MessageError::DemarshalingFailed);
                        }
                    }

                    i += arr_message_len - 1; // loop does += 1
                }
                MessageMagic::TypeObjectId => {
                    let msg = "object type is not implemented";
                    log::debug!("core protocol error: {msg}");
                    self.error(self.id(), msg);
                    return Err(MessageError::Unimplemented);
                }
                MessageMagic::TypeFd => {
                    if fd_no >= fds.len() {
                        let msg = "failed demarshaling fd";
                        log::debug!("core protocol error: {msg}");
                        self.error(self.id(), msg);
                        return Err(MessageError::DemarshalingFailed);
                    }
                    let mut storage = vec![0u8; std::mem::size_of::<i32>()];
                    storage.copy_from_slice(&fds[fd_no].to_le_bytes());
                    fd_no += 1;
                    buf = Some(storage.as_mut_ptr() as *mut raw::c_void);
                    other_buffers.push(storage);
                }
            }

            if let Some(b) = buf {
                avalues.push(b);
            }

            i += 1;
        }

        let listener = self.listeners()[id as usize];
        unsafe {
            ffi::call::<()>(
                &mut cif,
                libffi::high::CodePtr::from_ptr(listener),
                avalues.as_mut_ptr(),
            )
        };

        Ok(())
    }
}
