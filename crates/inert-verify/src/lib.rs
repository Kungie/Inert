//! # inert-verify
//!
//! Every verification rule of the Inert protocol lives here (spec §7) and is
//! shared by the server and the browser. Pure functions only: no I/O, no
//! async, no `unsafe`.

#![forbid(unsafe_code)]

pub mod contrast;
mod manifest_rules;
mod runtime;
pub mod sig;
mod values;

pub use manifest_rules::check_manifest;
pub use runtime::{check_action, check_open, check_screen};
pub use sig::{key_id, verify_welcome};
pub use values::{check_field_value, check_slot_value};

use inert_proto::ErrorCode;

/// A rule violation. `path` points at the offending part (`screens.overview.root.children[2]`).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{code}: {path}: {msg}")]
pub struct VerifyError {
    pub code: ErrorCode,
    pub path: String,
    pub msg: String,
    /// For action/open errors: the field the error belongs to, if any.
    pub field: Option<String>,
}

impl VerifyError {
    pub fn new(code: ErrorCode, path: impl Into<String>, msg: impl Into<String>) -> Self {
        Self { code, path: path.into(), msg: msg.into(), field: None }
    }

    pub fn field(code: ErrorCode, field: impl Into<String>, msg: impl Into<String>) -> Self {
        let field = field.into();
        Self { code, path: format!("fields.{field}"), msg: msg.into(), field: Some(field) }
    }
}

pub type Result<T> = std::result::Result<T, VerifyError>;
