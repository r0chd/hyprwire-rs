#[repr(u8)]
#[derive(Copy, Clone)]
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

    /// [magic : 1B][type : 1B][n_els : VLQ]{ [data...] }
    TypeArray = 0x21,

    /// [magic : 1B][id : UINT][name_len : VLQ][object name ...]
    TypeObject = 0x22,

    /// Special types
    /// FD has size 0. It's passed via control.
    TypeFd = 0x40,
}
