//! Error codes from spec §13.2 and the typed decoding error.

use crate::cbor::CborError;

/// Wire error codes (spec §13.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum ErrorCode {
    UnsupportedVersion = 1,
    FrameTooLarge = 2,
    MalformedCbor = 3,
    UnknownMessage = 4,
    BadSignature = 5,
    ManifestInvalid = 6,
    ScreenInvalid = 7,
    AssetInvalid = 8,
    Timeout = 9,
    FieldInvalid = 100,
    ActionUnknown = 101,
    Rejected = 102,
    Internal = 103,
}

impl ErrorCode {
    pub fn from_u64(n: u64) -> Option<Self> {
        use ErrorCode::*;
        Some(match n {
            1 => UnsupportedVersion,
            2 => FrameTooLarge,
            3 => MalformedCbor,
            4 => UnknownMessage,
            5 => BadSignature,
            6 => ManifestInvalid,
            7 => ScreenInvalid,
            8 => AssetInvalid,
            9 => Timeout,
            100 => FieldInvalid,
            101 => ActionUnknown,
            102 => Rejected,
            103 => Internal,
            _ => return None,
        })
    }

    pub fn as_u64(self) -> u64 {
        self as u16 as u64
    }

    /// Fatal codes close the connection (spec §13.1).
    pub fn is_fatal(self) -> bool {
        (self as u16) < 100
    }

    pub fn name(self) -> &'static str {
        use ErrorCode::*;
        match self {
            UnsupportedVersion => "unsupported_version",
            FrameTooLarge => "frame_too_large",
            MalformedCbor => "malformed_cbor",
            UnknownMessage => "unknown_message",
            BadSignature => "bad_signature",
            ManifestInvalid => "manifest_invalid",
            ScreenInvalid => "screen_invalid",
            AssetInvalid => "asset_invalid",
            Timeout => "timeout",
            FieldInvalid => "field_invalid",
            ActionUnknown => "action_unknown",
            Rejected => "rejected",
            Internal => "internal",
        }
    }
}

impl std::fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} ({})", self.name(), *self as u16)
    }
}

/// Error while turning bytes or a CBOR value into a typed message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("cbor: {0}")]
    Cbor(#[from] CborError),
    #[error("{path}: expected {expected}")]
    Type { path: String, expected: &'static str },
    #[error("{path}: missing field")]
    Missing { path: String },
    #[error("{path}: unknown field {field:?}")]
    Unknown { path: String, field: String },
    #[error("{path}: {msg}")]
    Invalid { path: String, msg: String },
    #[error("unknown message type {0:?}")]
    UnknownMessage(String),
}

impl DecodeError {
    /// The wire error code this decoding failure maps to.
    pub fn code(&self) -> ErrorCode {
        match self {
            DecodeError::UnknownMessage(_) => ErrorCode::UnknownMessage,
            _ => ErrorCode::MalformedCbor,
        }
    }
}
