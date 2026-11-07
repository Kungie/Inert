//! # inert-client
//!
//! The client half of the protocol, shared by the browser and the headless
//! test client. Every rule is enforced here, fail-closed: a session never
//! hands out a screen, a manifest or an asset that did not verify.

#![forbid(unsafe_code)]

use inert_proto::limits::{IMAGE_MAX_BYTES, IMAGE_MAX_PIXELS, IMAGE_MAX_SIDE};
use inert_proto::*;
use inert_transport::{ClientConn, TransportError};
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Where and how to reach a site, and which manifest key to trust.
#[derive(Clone)]
pub struct Site {
    pub host: String,
    pub addr: String,
    pub tls: Arc<inert_transport::rustls::ClientConfig>,
    pub pinned_key: [u8; 32],
    pub dev: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("the site's manifest key changed (pinned {pinned}, offered {offered})")]
    KeyChanged { pinned: String, offered: String },
    #[error("manifest version {got} is older than the last seen version {seen} (rollback)")]
    Rollback { got: u64, seen: u64 },
    #[error("{code}: {detail}")]
    Fatal { code: ErrorCode, detail: String },
    #[error("transport: {0}")]
    Transport(#[from] TransportError),
}

impl ClientError {
    pub fn code(&self) -> Option<ErrorCode> {
        match self {
            ClientError::Fatal { code, .. } => Some(*code),
            ClientError::Transport(t) => t.code(),
            ClientError::KeyChanged { .. } => Some(ErrorCode::BadSignature),
            ClientError::Rollback { .. } => Some(ErrorCode::ManifestInvalid),
        }
    }
}

fn fatal(code: ErrorCode, detail: impl Into<String>) -> ClientError {
    ClientError::Fatal { code, detail: detail.into() }
}

/// Outcome of an action that reached the server.
#[derive(Debug, Clone, PartialEq)]
pub enum ActionOutcome {
    Ok(Option<ScreenMsg>),
    Error { code: ErrorCode, field: Option<String> },
}

pub struct Session {
    conn: ClientConn,
    pub manifest: Arc<Manifest>,
    pub host: String,
    pub key_id: String,
}

pub fn new_action_id() -> [u8; 16] {
    use rand::RngCore;
    let mut id = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut id);
    id
}

impl Session {
    /// Dials, completes TLS, exchanges hello/welcome and verifies the
    /// manifest (key pin, signature, canonical bytes, host, all rules,
    /// rollback). `seen_version` is the highest version seen before.
    pub async fn open(site: &Site, seen_version: Option<u64>) -> Result<Session, ClientError> {
        let mut conn = inert_transport::connect(&site.addr, &site.host, site.tls.clone()).await?;
        conn.send(&Message::Hello { v: PROTOCOL_VERSION }).await?;
        let (manifest_bytes, sig, key_id) = match conn.recv(false).await? {
            Message::Welcome { v, manifest, sig, key_id } if v == PROTOCOL_VERSION => (manifest, sig, key_id),
            Message::Welcome { .. } => return Err(fatal(ErrorCode::UnsupportedVersion, "server speaks another version")),
            Message::Fatal { code } => return Err(fatal(code, "server refused the connection")),
            other => {
                conn.fatal(ErrorCode::UnknownMessage).await;
                return Err(fatal(ErrorCode::UnknownMessage, format!("expected welcome, got {}", other.type_name())));
            }
        };
        let pinned = inert_verify::key_id(&site.pinned_key);
        if key_id != pinned {
            conn.fatal(ErrorCode::BadSignature).await;
            return Err(ClientError::KeyChanged { pinned, offered: key_id });
        }
        let manifest = match inert_verify::verify_welcome(&manifest_bytes, &sig, &key_id, &site.pinned_key, &site.host) {
            Ok(m) => m,
            Err(e) => {
                conn.fatal(e.code).await;
                return Err(fatal(e.code, e.to_string()));
            }
        };
        if let Some(seen) = seen_version {
            if manifest.version < seen {
                conn.fatal(ErrorCode::ManifestInvalid).await;
                return Err(ClientError::Rollback { got: manifest.version, seen });
            }
        }
        Ok(Session { conn, manifest: Arc::new(manifest), host: site.host.clone(), key_id })
    }

    fn check_screen(&self, s: &ScreenMsg) -> Result<(), ClientError> {
        inert_verify::check_screen(&self.manifest, s).map_err(|e| fatal(e.code, e.to_string()))
    }

    async fn recv_reply(&mut self) -> Result<Message, ClientError> {
        let m = self.conn.recv(false).await?;
        if let Message::Fatal { code } = m {
            return Err(fatal(code, "server closed the session"));
        }
        Ok(m)
    }

    /// Opens a screen. Params are checked locally first.
    pub async fn open_screen(&mut self, screen: &str, params: Fields) -> Result<ScreenMsg, ClientError> {
        inert_verify::check_open(&self.manifest, screen, &params).map_err(|e| fatal(ErrorCode::FieldInvalid, e.to_string()))?;
        self.conn.send(&Message::Open { screen: screen.to_owned(), params }).await?;
        match self.recv_reply().await? {
            Message::Screen(s) if s.screen == screen => {
                if let Err(e) = self.check_screen(&s) {
                    self.conn.fatal(ErrorCode::ScreenInvalid).await;
                    return Err(e);
                }
                Ok(s)
            }
            other => {
                self.conn.fatal(ErrorCode::UnknownMessage).await;
                Err(fatal(ErrorCode::UnknownMessage, format!("expected screen {screen:?}, got {}", other.type_name())))
            }
        }
    }

    /// Validates locally (instant field errors), then sends. Local
    /// validation failures are returned as `ActionOutcome::Error` without
    /// touching the network.
    pub async fn action(&mut self, screen: &str, screen_data: &Fields, action: &str, fields: Fields) -> Result<ActionOutcome, ClientError> {
        if let Err(e) = inert_verify::check_action(&self.manifest, screen, Some(screen_data), action, &fields) {
            return Ok(ActionOutcome::Error { code: e.code, field: e.field });
        }
        self.action_unchecked(new_action_id(), screen, action, fields).await
    }

    /// Sends an action without local checks (tests use this to play a
    /// misbehaving client).
    pub async fn action_unchecked(&mut self, id: [u8; 16], screen: &str, action: &str, fields: Fields) -> Result<ActionOutcome, ClientError> {
        self.conn.send(&Message::Action { id, screen: screen.to_owned(), action: action.to_owned(), fields }).await?;
        match self.recv_reply().await? {
            Message::Result { id: rid, ok, error, next } if rid == id => {
                if let Some(n) = &next {
                    if let Err(e) = self.check_screen(n) {
                        self.conn.fatal(ErrorCode::ScreenInvalid).await;
                        return Err(e);
                    }
                }
                if ok {
                    Ok(ActionOutcome::Ok(next))
                } else {
                    let e = error.expect("decoder guarantees error when !ok");
                    Ok(ActionOutcome::Error { code: e.code, field: e.field })
                }
            }
            other => {
                self.conn.fatal(ErrorCode::UnknownMessage).await;
                Err(fatal(ErrorCode::UnknownMessage, format!("expected result, got {}", other.type_name())))
            }
        }
    }

    /// Fetches an asset and checks its hash and declared limits. Pixel
    /// limits are re-checked by the decoder.
    pub async fn asset(&mut self, hash: [u8; 32]) -> Result<(String, Vec<u8>), ClientError> {
        self.conn.send(&Message::GetAsset { hash }).await?;
        match self.recv_reply().await? {
            Message::Asset { hash: h, mime, bytes } if h == hash => {
                let ok_hash: [u8; 32] = Sha256::digest(&bytes).into();
                let bad = ok_hash != hash || (mime != "image/png" && mime != "image/webp") || bytes.len() as u64 > IMAGE_MAX_BYTES;
                let meta_bad = self.manifest.assets.get(&hash).is_some_and(|m| m.mime != mime || m.bytes != bytes.len() as u64);
                if bad || meta_bad {
                    self.conn.fatal(ErrorCode::AssetInvalid).await;
                    return Err(fatal(ErrorCode::AssetInvalid, "asset does not match its hash or limits"));
                }
                Ok((mime, bytes))
            }
            other => {
                self.conn.fatal(ErrorCode::UnknownMessage).await;
                Err(fatal(ErrorCode::UnknownMessage, format!("expected asset, got {}", other.type_name())))
            }
        }
    }
}

/// Pixel limits for decoders (spec §12).
pub fn image_dims_ok(w: u64, h: u64) -> bool {
    w > 0 && h > 0 && w <= IMAGE_MAX_SIDE && h <= IMAGE_MAX_SIDE && w * h <= IMAGE_MAX_PIXELS
}
