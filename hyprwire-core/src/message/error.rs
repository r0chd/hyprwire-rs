extern crate alloc;

use alloc::fmt;
use core::error;

/// Errors produced by wire-level message parsing and protocol dispatch.
#[derive(Debug)]
pub enum Error {
    /// The input buffer ended before the message was complete.
    UnexpectedEof,
    /// The message-type byte did not map to a known [`super::MessageType`].
    InvalidMessageType,
    /// A field-type tag in the message payload was not recognised.
    InvalidFieldType,
    /// A variable-length integer was encoded incorrectly.
    InvalidVarInt,
    /// The message structure was syntactically invalid.
    MalformedMessage,

    /// A `HandshakeBegin` message advertised more than 256 protocol versions.
    TooManyVersions,
    /// A `HandshakeProtocols` message listed more than 2048 protocols.
    TooManyProtocols,
    /// An array field exceeded the maximum allowed element count.
    ArrayTooLong,

    /// The version field in a `BindProtocol` message was out of range.
    InvalidVersion,
    /// A message that is valid on the wire arrived on the wrong side of the
    /// connection.
    InvalidMessage,
    /// Client and server could not find a common protocol version during
    /// the handshake.
    VersionNegotiationFailed,

    /// No spec was registered for this object's interface.
    NoSpec,
    /// The method index in a `GenericProtocolMessage` is out of range for the
    /// bound interface.
    InvalidMethod,
    /// A method argument did not match the expected type or was out of range.
    InvalidParameter,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnexpectedEof => write!(f, "unexpected end of message data"),
            Self::InvalidMessageType => write!(f, "invalid message type"),
            Self::InvalidFieldType => write!(f, "invalid field type"),
            Self::InvalidVarInt => write!(f, "invalid variable-length integer"),
            Self::MalformedMessage => write!(f, "malformed message"),
            Self::TooManyVersions => write!(f, "handshake advertised more than 256 versions"),
            Self::TooManyProtocols => write!(f, "handshake listed more than 2048 protocols"),
            Self::ArrayTooLong => write!(f, "array field exceeds maximum length"),
            Self::InvalidVersion => write!(f, "version number is out of range"),
            Self::InvalidMessage => {
                write!(f, "message type is not valid in this connection role")
            }
            Self::VersionNegotiationFailed => write!(f, "no common protocol version found"),
            Self::NoSpec => write!(f, "no spec registered for object"),
            Self::InvalidMethod => write!(f, "method index out of range for interface"),
            Self::InvalidParameter => write!(f, "method argument type or value is invalid"),
        }
    }
}

impl error::Error for Error {}
