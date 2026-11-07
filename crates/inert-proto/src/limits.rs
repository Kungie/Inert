//! Absolute limits from spec §12. A manifest may set lower limits, never higher.

/// Max frame for server → client messages (`welcome` and `asset` are the large ones).
pub const FRAME_MAX_SERVER_TO_CLIENT: usize = 4 * 1024 * 1024;
/// Max frame for every other message (all client → server messages, and
/// server → client messages other than `welcome` / `asset`).
pub const FRAME_MAX_SMALL: usize = 256 * 1024;
/// Max CBOR nesting depth.
pub const CBOR_MAX_DEPTH: usize = 32;
/// Max template tree depth.
pub const TEMPLATE_MAX_DEPTH: usize = 16;
pub const MAX_SCREENS: usize = 256;
pub const MAX_ACTIONS: usize = 256;
/// Absolute cap on any `string { max }`, in characters.
pub const STRING_MAX: u64 = 4096;
/// Absolute cap on any `list { max }`.
pub const LIST_MAX: u64 = 1000;
pub const IMAGE_MAX_BYTES: u64 = 2 * 1024 * 1024;
pub const IMAGE_MAX_SIDE: u64 = 4096;
pub const IMAGE_MAX_PIXELS: u64 = 16_000_000;
pub const HANDSHAKE_TIMEOUT_SECS: u64 = 10;
pub const READ_TIMEOUT_SECS: u64 = 30;
pub const IDLE_TIMEOUT_SECS: u64 = 300;
pub const REMEMBERED_ACTION_IDS: usize = 1024;
/// Max decimal scale (digits after the point).
pub const DECIMAL_MAX_SCALE: u32 = 18;
/// Max enum options per field.
pub const ENUM_MAX_OPTIONS: usize = 256;
/// Max fields per action / params per screen / slots per screen.
pub const MAX_FIELDS: usize = 64;
/// Max nodes in one screen template.
pub const MAX_NODES_PER_SCREEN: usize = 1024;
