//! `inert dev` (spec §10.3): one call that makes a site runnable locally.
//!
//! - creates a dev signing key and a dev CA + TLS leaf (once, in `.inert-dev/`)
//! - signs `app.inert.json`
//! - writes the site's dev trust file for the browser
//! - re-signs and hot-swaps the manifest whenever the JSON changes

use crate::App;
use crate::bundle::{self, Bundle};
use anyhow::{Context, Result};
use inert_transport::tls::{generate_dev_certs, server_config};
use inert_transport::trust::{DevTrust, default_dev_trust_dir, encode_b64};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

pub struct DevOptions {
    /// Directory holding `app.inert.json` (and its assets).
    pub dir: PathBuf,
    /// Address to listen on, e.g. `127.0.0.1:7443`.
    pub addr: String,
}

struct DevFiles {
    key: ed25519_dalek::SigningKey,
    leaf_der: Vec<u8>,
    leaf_key: Vec<u8>,
    ca_der: Vec<u8>,
}

fn ensure_dev_files(dir: &Path, host: &str) -> Result<DevFiles> {
    let d = dir.join(".inert-dev");
    std::fs::create_dir_all(&d)?;
    let key_path = d.join("signing.key");
    let key = if key_path.exists() {
        bundle::load_key(&key_path)?
    } else {
        let k = bundle::keygen();
        bundle::save_key(&k, &key_path)?;
        k
    };
    let (ca, leaf, leaf_key) = (d.join(format!("{host}.ca.der")), d.join(format!("{host}.leaf.der")), d.join(format!("{host}.leaf.key")));
    if !(ca.exists() && leaf.exists() && leaf_key.exists()) {
        let c = generate_dev_certs(host).map_err(anyhow::Error::msg)?;
        std::fs::write(&ca, &c.ca_der)?;
        std::fs::write(&leaf, &c.leaf_der)?;
        std::fs::write(&leaf_key, &c.leaf_key_pkcs8)?;
    }
    Ok(DevFiles { key, leaf_der: std::fs::read(leaf)?, leaf_key: std::fs::read(leaf_key)?, ca_der: std::fs::read(ca)? })
}

/// Signs, writes the trust file, serves, and hot-reloads. Never returns on success.
pub async fn run<S: Send + 'static>(opts: DevOptions, make_app: impl FnOnce(Bundle) -> Result<App<S>>) -> Result<()> {
    let _ = tracing_subscriber::fmt().with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into())).try_init();
    let json = opts.dir.join("app.inert.json");
    // First pass only to learn the host for the certs.
    let (src, _) = crate::json::load_source(&json)?;
    let host = src.app.clone();
    let files = ensure_dev_files(&opts.dir, &host)?;
    let bundle = bundle::build(&json, &files.key).context("signing app.inert.json")?;

    let trust = DevTrust { host: host.clone(), addr: opts.addr.clone(), ca_der_b64: encode_b64(&files.ca_der), manifest_key_b64: encode_b64(&bundle.public_key) };
    let trust_path = default_dev_trust_dir().join(format!("{host}.toml"));
    trust.save(&trust_path).map_err(anyhow::Error::msg)?;
    tracing::info!("dev trust file: {}", trust_path.display());
    tracing::info!("open inert://{host} in the Inert browser");

    let app = make_app(bundle)?.into_shared()?;
    let tls = server_config(vec![files.leaf_der.clone(), files.ca_der.clone()], files.leaf_key.clone()).map_err(anyhow::Error::msg)?;

    // Hot reload: poll the JSON (and its directory) for changes.
    {
        let app = app.clone();
        let key = files.key.clone();
        let json = json.clone();
        std::thread::spawn(move || {
            let mtime = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);
            let mut last = mtime(&json);
            loop {
                std::thread::sleep(Duration::from_millis(800));
                let now = mtime(&json);
                if now == last {
                    continue;
                }
                last = now;
                match bundle::build(&json, &key).and_then(|b| app.replace_bundle(b)) {
                    Ok(()) => tracing::info!("re-signed and reloaded {}", json.display()),
                    Err(e) => tracing::error!("reload rejected, keeping the previous manifest: {e:#}"),
                }
            }
        });
    }
    crate::app::serve(app, &opts.addr, tls).await
}

/// Builds a multi-thread runtime and runs [`run`]. Handy for example `main`s:
/// `inert_server::dev::main(env!("CARGO_MANIFEST_DIR"), "127.0.0.1:7443", build)`.
/// The first CLI argument, if any, overrides the listen address.
pub fn main<S: Send + 'static>(app_dir: &str, default_addr: &str, make_app: impl FnOnce(Bundle) -> Result<App<S>>) -> Result<()> {
    let addr = std::env::args().nth(1).unwrap_or_else(|| default_addr.to_owned());
    let dir = std::env::var("INERT_APP_DIR").map(PathBuf::from).unwrap_or_else(|_| PathBuf::from(app_dir));
    tokio::runtime::Builder::new_multi_thread().enable_all().build()?.block_on(run(DevOptions { dir, addr }, make_app))
}

/// Keeps `Arc` in the public API surface tidy.
pub type SharedApp<S> = Arc<App<S>>;
