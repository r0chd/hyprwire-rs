use crate::message::MessageError;
use libffi::low;
use std::{cell, fmt, hash, rc, sync::Arc};

/// A user-facing handle to a protocol object. Wraps the internal
/// `Rc<RefCell<dyn Object>>` so callers never deal with those types directly.
pub struct Object(rc::Rc<cell::RefCell<dyn super::object::Object>>);

impl Object {
    pub fn from_raw(inner: rc::Rc<cell::RefCell<dyn super::object::Object>>) -> Self {
        Self(inner)
    }

    #[must_use]
    pub fn into_inner(self) -> rc::Rc<cell::RefCell<dyn super::object::Object>> {
        self.0
    }

    #[must_use]
    pub fn inner(&self) -> &rc::Rc<cell::RefCell<dyn super::object::Object>> {
        &self.0
    }
}

impl fmt::Debug for Object {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Object")
            .field(&rc::Rc::as_ptr(&self.0))
            .finish()
    }
}

impl Clone for Object {
    fn clone(&self) -> Self {
        Self(rc::Rc::clone(&self.0))
    }
}

impl PartialEq for Object {
    fn eq(&self, other: &Self) -> bool {
        rc::Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for Object {}

impl hash::Hash for Object {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        rc::Rc::as_ptr(&self.0).hash(state);
    }
}

#[repr(u8)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum MessageMagic {
    /// Signifies an end of a message
    End = 0x0,

    /// Primitive type identifiers
    TypeUint = 0x10,
    TypeInt = 0x11,
    TypeF32 = 0x12,
    TypeSeq = 0x13,
    TypeObjectId = 0x14,

    /// Variable length types
    /// [magic : 1B][len : VLQ][data : len B]
    TypeVarchar = 0x20,

    /// [magic : 1B][type : 1B][`n_els` : VLQ]{ [data...] }
    TypeArray = 0x21,

    /// [magic : 1B][id : UINT][`name_len` : VLQ][object name ...]
    TypeObject = 0x22,

    /// Special types
    /// FD has size 0. It's passed via control.
    TypeFd = 0x40,
}

impl MessageMagic {
    pub(crate) fn to_ffi_type(self) -> *mut low::ffi_type {
        match self {
            Self::TypeUint | Self::TypeObject | Self::TypeSeq | Self::TypeObjectId => {
                &raw mut low::types::uint32
            }
            Self::TypeInt | Self::TypeFd => &raw mut low::types::sint32,
            Self::TypeVarchar | Self::TypeArray => &raw mut low::types::pointer,
            Self::TypeF32 => &raw mut low::types::float,
            Self::End => &raw mut low::types::void,
        }
    }
}

impl TryFrom<u8> for MessageMagic {
    type Error = MessageError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(Self::End),
            0x10 => Ok(Self::TypeUint),
            0x11 => Ok(Self::TypeInt),
            0x12 => Ok(Self::TypeF32),
            0x13 => Ok(Self::TypeSeq),
            0x14 => Ok(Self::TypeObjectId),
            0x20 => Ok(Self::TypeVarchar),
            0x21 => Ok(Self::TypeArray),
            0x22 => Ok(Self::TypeObject),
            0x40 => Ok(Self::TypeFd),
            _ => Err(MessageError::MalformedMessage),
        }
    }
}

pub enum CallArg<'a> {
    Uint(u32),
    Int(i32),
    F32(f32),
    Object(u32),
    Varchar(&'a [u8]),
    Fd(i32),
    UintArray(&'a [u32]),
    IntArray(&'a [i32]),
    F32Array(&'a [f32]),
    ObjectArray(&'a [u32]),
    FdArray(&'a [i32]),
    VarcharArray(&'a [&'a [u8]]),
}

pub struct Method {
    pub idx: u32,
    pub params: &'static [u8],
    pub returns_type: &'static str,
    pub since: u32,
    pub destructor: bool,
}

pub trait ProtocolObjectSpec: Send + Sync {
    fn object_name(&self) -> &str;

    fn c2s(&self) -> &[Method];

    fn s2c(&self) -> &[Method];
}

pub trait ProtocolSpec {
    fn spec_name(&self) -> &str;

    fn spec_ver(&self) -> u32;

    fn objects(&self) -> &[Arc<dyn ProtocolObjectSpec>];
}
