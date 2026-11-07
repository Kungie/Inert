//! Messages between the Inert browser and `inert-servo-host`.
//!
//! Transport: one JSON object per line. Commands go to the host's stdin,
//! events come back on its stdout. Rendered frames are written to a
//! memory-mapped file with two slots (double buffering); a `Frame` event says
//! which slot holds the newest frame. The browser process never links Servo.

#![forbid(unsafe_code)]

use serde::{Deserialize, Serialize};

/// Bytes per frame slot: enough for 3840×2160 RGBA.
pub const SLOT_BYTES: u64 = 3840 * 2160 * 4;
pub const SLOTS: u64 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Cmd {
    Create { view: u64, url: String },
    Load { view: u64, url: String },
    /// Makes `view` the one that is painted (the active tab).
    Show { view: u64 },
    Close { view: u64 },
    Resize { w: u32, h: u32 },
    Back { view: u64 },
    Forward { view: u64 },
    Reload { view: u64 },
    Zoom { view: u64, z: f32 },
    /// Runs a script; the string result comes back as `Ev::Eval { req }`.
    Eval { view: u64, req: u64, script: String },
    Input { view: u64, ev: Input },
    Adblock { on: bool },
    /// The filter lists were (re)cached: rebuild the blocking engine.
    LoadFilters,
    Quit,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "k", rename_all = "snake_case")]
pub enum Input {
    Move { x: f32, y: f32 },
    /// button: 0 primary, 1 middle, 2 secondary, 3 back, 4 forward.
    Button { x: f32, y: f32, button: u8, down: bool },
    /// mode: 0 pixel, 1 line, 2 page. DOM convention: positive dy scrolls down.
    Wheel { x: f32, y: f32, dx: f64, dy: f64, mode: u8 },
    /// Named keys only ("Enter", "Backspace", "ArrowLeft", …).
    Key { key: String, down: bool, shift: bool, ctrl: bool, alt: bool, meta: bool },
    Text { s: String },
    Focus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum Ev {
    Ready { shm: String },
    Frame { view: u64, slot: u8, w: u32, h: u32, seq: u64 },
    Load { view: u64, started: bool, url: String },
    Url { view: u64, url: String },
    Title { view: u64, title: String },
    History { view: u64, back: bool, fwd: bool },
    Cursor { view: u64, cursor: String },
    /// An inert:// navigation the host refused; the browser opens it securely.
    Navigate { view: u64, url: String },
    Eval { req: u64, text: String },
    Blocked { n: u64 },
    /// The blocking engine is built and in use.
    ShieldReady,
    Log { msg: String },
}
