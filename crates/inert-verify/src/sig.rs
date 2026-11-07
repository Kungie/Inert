//! Manifest signatures (Ed25519 over `SIGNING_CONTEXT || manifest_bytes`).

use crate::{Result, VerifyError, check_manifest};
use ed25519_dalek::{Signature, VerifyingKey};
use inert_proto::{ErrorCode, Manifest, SIGNING_CONTEXT};
use sha2::{Digest, Sha256};

/// Short, stable identifier for a public key: `ed25519:<first 8 bytes of sha256, hex>`.
pub fn key_id(public_key: &[u8; 32]) -> String {
    let h = Sha256::digest(public_key);
    format!("ed25519:{}", hex::encode(&h[..8]))
}

/// The exact bytes that are signed.
pub fn signing_message(manifest_bytes: &[u8]) -> Vec<u8> {
    let mut m = Vec::with_capacity(SIGNING_CONTEXT.len() + manifest_bytes.len());
    m.extend_from_slice(SIGNING_CONTEXT);
    m.extend_from_slice(manifest_bytes);
    m
}

pub fn verify_signature(manifest_bytes: &[u8], sig: &[u8; 64], public_key: &[u8; 32]) -> Result<()> {
    let bad = |m: &str| VerifyError::new(ErrorCode::BadSignature, "welcome.sig", m);
    let key = VerifyingKey::from_bytes(public_key).map_err(|_| bad("pinned key is not a valid Ed25519 key"))?;
    let sig = Signature::from_bytes(sig);
    key.verify_strict(&signing_message(manifest_bytes), &sig).map_err(|_| bad("signature does not verify"))
}

/// Everything a client checks on `welcome` (spec §5.4 step 3, §7.1, §8):
/// key id, signature against the pinned key, manifest decoding, that the
/// manifest belongs to the host that was dialled, and all manifest rules.
pub fn verify_welcome(manifest_bytes: &[u8], sig: &[u8; 64], key_id_claimed: &str, pinned_key: &[u8; 32], host: &str) -> Result<Manifest> {
    if key_id_claimed != key_id(pinned_key) {
        return Err(VerifyError::new(ErrorCode::BadSignature, "welcome.key_id", "manifest is signed with a key that is not the pinned key"));
    }
    verify_signature(manifest_bytes, sig, pinned_key)?;
    let manifest = Manifest::from_bytes(manifest_bytes).map_err(|e| VerifyError::new(ErrorCode::ManifestInvalid, "manifest", e.to_string()))?;
    // Bytes must already be canonical: what was signed is what everyone decodes.
    if manifest.to_bytes() != manifest_bytes {
        return Err(VerifyError::new(ErrorCode::ManifestInvalid, "manifest", "manifest bytes are not in canonical form"));
    }
    if manifest.app != host {
        return Err(VerifyError::new(ErrorCode::ManifestInvalid, "manifest.app", format!("manifest is for {:?}, not {host:?}", manifest.app)));
    }
    check_manifest(&manifest)?;
    Ok(manifest)
}
