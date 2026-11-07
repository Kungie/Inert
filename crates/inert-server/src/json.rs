//! `app.inert.json` → [`Manifest`].
//!
//! The JSON source mirrors the CBOR shape (spec §6) with three conveniences:
//! decimals are written as strings (`"0.01"`), static assets are named and
//! point at files (`"assets": {"logo": "assets/logo.png"}`, then
//! `"src": {"asset": "logo"}`), and decimal literals in `args` are written as
//! `{"lit": {"decimal": "1.00"}}`. Everything else is decoded by the same
//! strict decoder that reads the wire format, so unknown fields fail.

use anyhow::{Context, Result, anyhow, bail};
use inert_proto::{AssetMeta, Decimal, Hash, Manifest, Value};
use serde_json::Value as J;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::Path;

/// A static asset resolved from the JSON source.
#[derive(Debug, Clone)]
pub struct AssetFile {
    pub hash: Hash,
    pub mime: String,
    pub bytes: Vec<u8>,
}

/// Image dimensions without decoding pixels (PNG IHDR / WebP VP8X|VP8|VP8L).
pub fn image_info(bytes: &[u8]) -> Result<(&'static str, u64, u64)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") && bytes.len() >= 24 {
        let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap()) as u64;
        let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap()) as u64;
        return Ok(("image/png", w, h));
    }
    if bytes.len() >= 30 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        let chunk = &bytes[12..16];
        let d = &bytes[20..];
        if chunk == b"VP8X" {
            let w = 1 + u32::from_le_bytes([d[4], d[5], d[6], 0]) as u64;
            let h = 1 + u32::from_le_bytes([d[7], d[8], d[9], 0]) as u64;
            return Ok(("image/webp", w, h));
        }
        if chunk == b"VP8 " {
            let w = (u16::from_le_bytes([d[6], d[7]]) & 0x3fff) as u64;
            let h = (u16::from_le_bytes([d[8], d[9]]) & 0x3fff) as u64;
            return Ok(("image/webp", w, h));
        }
        if chunk == b"VP8L" {
            let b = &d[1..5];
            let w = 1 + ((b[0] as u64) | (((b[1] & 0x3f) as u64) << 8));
            let h = 1 + (((b[1] >> 6) as u64) | ((b[2] as u64) << 2) | (((b[3] & 0x0f) as u64) << 10));
            return Ok(("image/webp", w, h));
        }
    }
    bail!("not a PNG or WebP image")
}

pub fn hash_bytes(b: &[u8]) -> Hash {
    Sha256::digest(b).into()
}

/// Converts JSON to an Inert value. `ctx` lets the caller rewrite special nodes.
fn to_value(j: &J, assets: &BTreeMap<String, Hash>, path: &str) -> Result<Value> {
    Ok(match j {
        J::Null => Value::Null,
        J::Bool(b) => Value::Bool(*b),
        J::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::int(i)
            } else if let Some(u) = n.as_u64() {
                Value::Uint(u)
            } else {
                bail!("{path}: floats are not allowed; write decimals as strings")
            }
        }
        J::String(s) => Value::text(s.clone()),
        J::Array(a) => Value::Array(a.iter().enumerate().map(|(i, x)| to_value(x, assets, &format!("{path}[{i}]"))).collect::<Result<_>>()?),
        J::Object(o) => {
            let is_decimal_type = o.get("type").and_then(J::as_str) == Some("decimal");
            let mut out = Vec::with_capacity(o.len());
            for (k, v) in o {
                let p = format!("{path}.{k}");
                let val = if is_decimal_type && (k == "min" || k == "max") {
                    let s = v.as_str().ok_or_else(|| anyhow!("{p}: decimal bounds are strings like \"0.01\""))?;
                    Decimal::parse(s).ok_or_else(|| anyhow!("{p}: invalid decimal {s:?}"))?.to_value()
                } else if k == "asset" && v.is_string() {
                    let name = v.as_str().unwrap();
                    let h = assets.get(name).ok_or_else(|| anyhow!("{p}: unknown asset {name:?}"))?;
                    Value::Bytes(h.to_vec())
                } else if k == "lit" && v.get("decimal").is_some() && v.as_object().is_some_and(|m| m.len() == 1) {
                    let s = v["decimal"].as_str().ok_or_else(|| anyhow!("{p}: decimal literal must be a string"))?;
                    Decimal::parse(s).ok_or_else(|| anyhow!("{p}: invalid decimal {s:?}"))?.to_value()
                } else {
                    to_value(v, assets, &p)?
                };
                out.push((Value::text(k.clone()), val));
            }
            Value::Map(out)
        }
    })
}

/// Reads `app.inert.json` and the asset files it names.
pub fn load_source(json_path: &Path) -> Result<(Manifest, Vec<AssetFile>)> {
    let dir = json_path.parent().unwrap_or(Path::new("."));
    let text = std::fs::read_to_string(json_path).with_context(|| format!("reading {}", json_path.display()))?;
    let mut j: J = serde_json::from_str(&text).with_context(|| format!("parsing {}", json_path.display()))?;
    let obj = j.as_object_mut().ok_or_else(|| anyhow!("top level must be an object"))?;

    let mut names = BTreeMap::new();
    let mut files = Vec::new();
    let mut metas = BTreeMap::new();
    if let Some(a) = obj.remove("assets") {
        let a = a.as_object().ok_or_else(|| anyhow!("assets must be an object of name → file"))?.clone();
        for (name, file) in a {
            let rel = file.as_str().ok_or_else(|| anyhow!("assets.{name} must be a file path"))?;
            let bytes = std::fs::read(dir.join(rel)).with_context(|| format!("reading asset {rel}"))?;
            let (mime, w, h) = image_info(&bytes).with_context(|| format!("asset {rel}"))?;
            let hash = hash_bytes(&bytes);
            names.insert(name.clone(), hash);
            metas.insert(hash, AssetMeta { mime: mime.into(), bytes: bytes.len() as u64, width: w, height: h });
            files.push(AssetFile { hash, mime: mime.into(), bytes });
        }
    }
    let v = to_value(&j, &names, "manifest")?;
    // Insert the resolved assets map, then decode strictly.
    let mut entries = match v {
        Value::Map(m) => m,
        _ => unreachable!(),
    };
    entries.push((
        Value::text("assets"),
        Value::Map(
            metas
                .iter()
                .map(|(h, m)| {
                    (
                        Value::Bytes(h.to_vec()),
                        inert_proto::cbor::map([
                            ("mime", Value::text(m.mime.clone())),
                            ("bytes", Value::Uint(m.bytes)),
                            ("width", Value::Uint(m.width)),
                            ("height", Value::Uint(m.height)),
                        ]),
                    )
                })
                .collect(),
        ),
    ));
    // Round-trip through the canonical encoding so map order matches the wire.
    let canonical = inert_proto::cbor::decode(&inert_proto::cbor::encode(&Value::Map(entries)))?;
    let m = Manifest::from_value(&canonical).map_err(|e| anyhow!("{}: {e}", json_path.display()))?;
    Ok((m, files))
}
