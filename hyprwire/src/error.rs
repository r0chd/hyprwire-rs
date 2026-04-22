use hyprwire_core::message;
use std::{error, fmt, io};

/// Errors returned by the public Hyprwire client and server APIs.
#[derive(Debug)]
pub enum Error {
    /// The connection was closed by the peer or an unrecoverable protocol error
    /// occurred that has left the connection in an unusable state.
    ConnectionClosed,

    /// The initial handshake did not complete within the allowed timeout.
    HandshakeTimeout,

    /// Client and server could not agree on a compatible protocol version.
    VersionNegotiationFailed,

    /// The requested bind version exceeds the maximum version supported by the
    /// spec.
    VersionOutOfRange {
        /// The version that was requested.
        requested: u32,
        /// The highest version the spec supports.
        max: u32,
    },

    /// A wire-level protocol violation was detected.
    ProtocolViolation(message::Error),

    /// An OS-level I/O error.
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ConnectionClosed => write!(f, "connection closed"),
            Self::HandshakeTimeout => write!(f, "handshake timed out"),
            Self::VersionNegotiationFailed => {
                write!(f, "version negotiation failed: no common protocol version")
            }
            Self::VersionOutOfRange { requested, max } => write!(
                f,
                "requested version {requested} exceeds spec maximum {max}"
            ),
            Self::ProtocolViolation(e) => write!(f, "protocol violation: {e}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl error::Error for Error {
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            Self::ProtocolViolation(e) => Some(e),
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<message::Error> for Error {
    fn from(e: message::Error) -> Self {
        match e {
            message::Error::VersionNegotiationFailed => Self::VersionNegotiationFailed,
            other => Self::ProtocolViolation(other),
        }
    }
}
