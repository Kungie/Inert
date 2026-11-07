//! Signing keys and signed bundles (spec §10.1).
//!
//! A bundle is `manifest.signed.cbor` — `{manifest: bytes, sig: bytes(64),
//! key_id: text, public_key: bytes(32)}` — plus `assets/<sha256 hex>` files.
//! Servers load bundles; they never see the signing key.

use crate::json::{AssetFile, load_source};
use anyhow::{Context, Result, anyhow, bail};
use ed25519_dalek::{Signer, SigningKey};
use inert_proto::cbor::{self, Value, map};
use inert_proto::{Hash, Manifest};
use std::collections::BTreeMap;
use std::path::Path;

const KEY_PREFIX: &str = "inert-ed25519-secret:";

pub fn keygen() -> SigningKey {
    SigningKey::generate(&mut rand::rngs::OsRng)
}

pub fn save_key(key: &SigningKey, path: &Path) -> Result<()> {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(path, format!("{KEY_PREFIX}{}\n", hex::encode(key.to_bytes())))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub fn load_key(path: &Path) -> Result<SigningKey> {
    let s = std::fs::read_to_string(path).with_context(|| format!("reading key {}", path.display()))?;
    let h = s.trim().strip_prefix(KEY_PREFIX).ok_or_else(|| anyhow!("{} is not an Inert signing key", path.display()))?;
    let b: [u8; 32] = hex::decode(h)?.try_into().map_err(|_| anyhow!("bad key length"))?;
    Ok(SigningKey::from_bytes(&b))
}

/// A signed manifest ready to serve.
#[derive(Clone)]
pub struct Bundle {
    pub manifest: Manifest,
    pub manifest_bytes: Vec<u8>,
    pub sig: [u8; 64],
    pub key_id: String,
    pub public_key: [u8; 32],
    pub assets: BTreeMap<Hash, AssetFile>,
}

/// `inert-sign build`: rules → canonical CBOR → signature.
pub fn build(json_path: &Path, key: &SigningKey) -> Result<Bundle> {
    let (manifest, files) = load_source(json_path)?;
    inert_verify::check_manifest(&manifest).map_err(|e| anyhow!("manifest rule failed — nothing was signed\n  {e}"))?;
    let manifest_bytes = manifest.to_bytes();
    let sig = key.sign(&inert_verify::sig::signing_message(&manifest_bytes)).to_bytes();
    let public_key = key.verifying_key().to_bytes();
    let key_id = inert_verify::key_id(&public_key);
    // Self-check: what we produce must verify like a browser would.
    inert_verify::verify_welcome(&manifest_bytes, &sig, &key_id, &public_key, &manifest.app).map_err(|e| anyhow!("self-check failed: {e}"))?;
    Ok(Bundle { manifest, manifest_bytes, sig, key_id, public_key, assets: files.into_iter().map(|a| (a.hash, a)).collect() })
}

impl Bundle {
    pub fn write(&self, out: &Path) -> Result<()> {
        let v = map([
            ("manifest", Value::Bytes(self.manifest_bytes.clone())),
            ("sig", Value::Bytes(self.sig.to_vec())),
            ("key_id", Value::text(self.key_id.clone())),
            ("public_key", Value::Bytes(self.public_key.to_vec())),
        ]);
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p)?;
        }
        std::fs::write(out, cbor::encode(&v))?;
        let adir = out.parent().unwrap_or(Path::new(".")).join("assets");
        for a in self.assets.values() {
            std::fs::create_dir_all(&adir)?;
            std::fs::write(adir.join(hex::encode(a.hash)), &a.bytes)?;
        }
        Ok(())
    }

    /// Loads a bundle written by [`Bundle::write`] and re-checks everything.
    pub fn load(path: &Path) -> Result<Self> {
        let raw = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
        let v = cbor::decode(&raw)?;
        let mut o = inert_proto::obj::Obj::new(&v, "bundle")?;
        let manifest_bytes = o.bytes("manifest")?;
        let sig = o.fixed::<64>("sig")?;
        let key_id = o.text("key_id")?;
        let public_key = o.fixed::<32>("public_key")?;
        o.finish()?;
        let manifest = Manifest::from_bytes(&manifest_bytes)?;
        inert_verify::verify_welcome(&manifest_bytes, &sig, &key_id, &public_key, &manifest.app)?;
        let adir = path.parent().unwrap_or(Path::new(".")).join("assets");
        let mut assets = BTreeMap::new();
        for (h, meta) in &manifest.assets {
            let bytes = std::fs::read(adir.join(hex::encode(h))).with_context(|| "missing asset file")?;
            if crate::json::hash_bytes(&bytes) != *h {
                bail!("asset {} does not match its hash", hex::encode(h));
            }
            assets.insert(*h, AssetFile { hash: *h, mime: meta.mime.clone(), bytes });
        }
        Ok(Bundle { manifest, manifest_bytes, sig, key_id, public_key, assets })
    }
}
