//! # inert-proto
//!
//! Message and App Manifest types for the Inert protocol, a strict
//! deterministic CBOR codec and the protocol's absolute limits.
//!
//! This crate has no I/O, no async runtime and no `unsafe`. It is the part of
//! the protocol every implementation must agree on byte for byte.

#![forbid(unsafe_code)]

pub mod cbor;
pub mod decimal;
pub mod error;
pub mod limits;
pub mod manifest;
pub mod msg;
pub mod obj;

pub use cbor::Value;
pub use decimal::Decimal;
pub use error::{DecodeError, ErrorCode};
pub use manifest::*;
pub use msg::{Fields, Message, PROTOCOL_VERSION, ResultError, ScreenMsg};

/// Domain separation prefix for manifest signatures.
pub const SIGNING_CONTEXT: &[u8] = b"inert-manifest-v1\0";
