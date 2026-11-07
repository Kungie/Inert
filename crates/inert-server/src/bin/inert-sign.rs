//! `inert-sign keygen --out <file>`
//! `inert-sign build <app.inert.json> --key <file> --out <manifest.signed.cbor>`
//! `inert-sign check <app.inert.json>`   (rules only, nothing is signed)

use anyhow::{Result, bail};
use inert_server::bundle;
use std::path::PathBuf;

fn arg(args: &[String], flag: &str) -> Option<PathBuf> {
    args.iter().position(|a| a == flag).and_then(|i| args.get(i + 1)).map(PathBuf::from)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("keygen") => {
            let out = arg(&args, "--out").unwrap_or_else(|| PathBuf::from("signing.key"));
            if out.exists() {
                bail!("{} already exists; refusing to overwrite a signing key", out.display());
            }
            let k = bundle::keygen();
            bundle::save_key(&k, &out)?;
            println!("wrote {} (keep it out of version control)", out.display());
            println!("public key id: {}", inert_verify::key_id(&k.verifying_key().to_bytes()));
        }
        Some("build") => {
            let Some(json) = args.get(1).map(PathBuf::from) else { bail!("usage: inert-sign build <app.inert.json> --key <file> --out <file>") };
            let key = bundle::load_key(&arg(&args, "--key").unwrap_or_else(|| PathBuf::from("signing.key")))?;
            let out = arg(&args, "--out").unwrap_or_else(|| PathBuf::from("manifest.signed.cbor"));
            let b = bundle::build(&json, &key)?;
            b.write(&out)?;
            println!("signed {} v{} → {} ({} bytes, {} assets, key {})", b.manifest.app, b.manifest.version, out.display(), b.manifest_bytes.len(), b.assets.len(), b.key_id);
        }
        Some("check") => {
            let Some(json) = args.get(1).map(PathBuf::from) else { bail!("usage: inert-sign check <app.inert.json>") };
            let (m, _) = inert_server::json::load_source(&json)?;
            inert_verify::check_manifest(&m)?;
            println!("{}: ok ({} screens, {} actions)", m.app, m.screens.len(), m.actions.len());
        }
        _ => {
            eprintln!("inert-sign keygen --out <file>\ninert-sign build <app.inert.json> --key <file> --out <manifest.signed.cbor>\ninert-sign check <app.inert.json>");
            std::process::exit(2);
        }
    }
    Ok(())
}
