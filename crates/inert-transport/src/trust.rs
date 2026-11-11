//! Development trust files (spec §8, development mode).
//!
//! `inert dev` writes one file per site; `inert-browser` reads them. A file
//! carries where to dial the site, the dev CA that signs its TLS leaf, and the
//! manifest signing key to pin. Browsers mark such sites as `DEV`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DevTrust {
    /// Site domain, e.g. `bank.example`.
    pub host: String,
    /// Socket address to dial instead of DNS, e.g. `127.0.0.1:7443`.
    pub addr: String,
    /// Dev CA certificate, DER, base64.
    pub ca_der_b64: String,
    /// Ed25519 manifest public key, base64.
    pub manifest_key_b64: String,
}

impl DevTrust {
    pub fn ca_der(&self) -> Option<Vec<u8>> {
        b64(&self.ca_der_b64)
    }

    pub fn manifest_key(&self) -> Option<[u8; 32]> {
        b64(&self.manifest_key_b64)?.try_into().ok()
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let s = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        toml::from_str(&s).map_err(|e| e.to_string())
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, toml::to_string_pretty(self).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
    }
}

pub fn encode_b64(b: &[u8]) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(b)
}

fn b64(s: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s).ok()
}

/// Default directory for dev trust files:
/// `~/Library/Application Support/Inert/dev-trust` on macOS.
pub fn default_dev_trust_dir() -> PathBuf {
    if let Ok(p) = std::env::var("INERT_DEV_TRUST_DIR") {
        return PathBuf::from(p);
    }
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("Inert").join("dev-trust")
}

/// Loads every `*.toml` dev trust file in `dir`.
pub fn load_dir(dir: &Path) -> Vec<DevTrust> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut out: Vec<DevTrust> = rd
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .filter_map(|p| DevTrust::load(&p).ok())
        .collect();
    out.sort_by(|a, b| a.host.cmp(&b.host));
    out
}
