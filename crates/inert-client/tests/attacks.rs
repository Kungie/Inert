//! End-to-end tests over real TCP + TLS 1.3, including the ten attack
//! scenarios of spec §15.1. A "real" server runs the SDK; an "evil" server
//! speaks the wire protocol by hand and misbehaves on purpose.

use ed25519_dalek::SigningKey;
use inert_client::{ActionOutcome, ClientError, Session, Site};
use inert_proto::*;
use inert_server::{App, Bundle, Data, Next, Reject, val};
use inert_transport::tls::{client_config_dev, generate_dev_certs, server_config};
use inert_transport::{Conn, Role};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;

const HOST: &str = "bank.example";

fn bank_json() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../examples/bank/app.inert.json")
}

fn key(seed: u8) -> SigningKey {
    SigningKey::from_bytes(&[seed; 32])
}

fn signed_bundle(k: &SigningKey) -> Bundle {
    inert_server::bundle::build(&bank_json(), k).unwrap()
}

struct Tls {
    client: Arc<inert_transport::rustls::ClientConfig>,
    acceptor: inert_transport::tokio_rustls::TlsAcceptor,
}

fn tls() -> Tls {
    let c = generate_dev_certs(HOST).unwrap();
    Tls {
        client: client_config_dev(&c.ca_der).unwrap(),
        acceptor: inert_transport::tokio_rustls::TlsAcceptor::from(server_config(vec![c.leaf_der, c.ca_der], c.leaf_key_pkcs8).unwrap()),
    }
}

struct Counter(Arc<AtomicUsize>);

fn bank_app(b: Bundle, transfers: Arc<AtomicUsize>) -> App<Counter> {
    let mut app = App::new(b, Counter(transfers));
    let accounts = || val::list([val::obj([("id", val::text("chk")), ("label", val::text("Checking"))])]);
    app.screen("overview", |_, _| {
        Ok(Data::new()
            .set("holder", val::text("Demo"))
            .set("total", val::dec(100, 2))
            .set("currency", val::text("EUR"))
            .set("accounts", val::list([val::obj([("id", val::text("chk")), ("label", val::text("Checking")), ("balance", val::dec(100, 2))])]))
            .set("transactions", val::list([])))
    });
    app.screen("transfer", move |_, _| Ok(Data::new().set("accounts", accounts()).set("limit", val::dec(1_000_000, 2))));
    app.screen("settings", |_, _| Ok(Data::new().set("notify", val::bool(true)).set("currency", val::text("eur"))));
    app.action("transfer", |c, a| {
        if a.text("to").is_empty() {
            return Err(Reject::on("to"));
        }
        c.0.fetch_add(1, Ordering::SeqCst);
        Ok(Next::screen("overview"))
    });
    app.action("save_settings", |_, _| Ok(Next::Stay));
    app
}

/// Starts the real SDK server. Returns the site description and a transfer counter.
async fn real_server() -> (Site, Arc<AtomicUsize>) {
    let k = key(1);
    let counter = Arc::new(AtomicUsize::new(0));
    let app = bank_app(signed_bundle(&k), counter.clone()).into_shared().unwrap();
    let t = tls();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let acceptor = t.acceptor.clone();
    tokio::spawn(async move {
        loop {
            let (tcp, _) = listener.accept().await.unwrap();
            let (app, acc) = (app.clone(), acceptor.clone());
            tokio::spawn(async move {
                if let Ok(c) = inert_transport::handshake(tcp, &acc).await {
                    let _ = inert_server::app::session(app, c).await;
                }
            });
        }
    });
    (Site { host: HOST.into(), addr, tls: t.client, pinned_key: k.verifying_key().to_bytes(), dev: true }, counter)
}

/// An evil server: sends `welcome`, then answers every request with `reply`.
async fn evil_server(welcome: Message, reply: impl Fn(&Message) -> Message + Send + Sync + 'static) -> (String, Arc<inert_transport::rustls::ClientConfig>) {
    let t = tls();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let reply = Arc::new(reply);
    tokio::spawn(async move {
        loop {
            let (tcp, _) = listener.accept().await.unwrap();
            let (acc, welcome, reply) = (t.acceptor.clone(), welcome.clone(), reply.clone());
            tokio::spawn(async move {
                let Ok(mut c) = inert_transport::handshake(tcp, &acc).await else { return };
                let _ = c.recv(false).await; // hello
                let _ = c.send(&welcome).await;
                while let Ok(m) = c.recv(false).await {
                    if c.send(&reply(&m)).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    (addr, t.client)
}

fn welcome_for(b: &Bundle) -> Message {
    Message::Welcome { v: PROTOCOL_VERSION, manifest: b.manifest_bytes.clone(), sig: b.sig, key_id: b.key_id.clone() }
}

fn transfer_screen_with(extra: Option<(&str, Value)>, item_extra: bool) -> Message {
    let mut item = vec![("id", Value::text("chk")), ("label", Value::text("Checking"))];
    if item_extra {
        item.push(("pin", Value::text("1234")));
    }
    let mut data = Fields::new();
    data.insert("accounts".into(), Value::Array(vec![cbor::map(item)]));
    data.insert("limit".into(), Decimal::new(100, 2).to_value());
    if let Some((k, v)) = extra {
        data.insert(k.into(), v);
    }
    Message::Screen(ScreenMsg { screen: "transfer".into(), data })
}

fn site(addr: String, tls: Arc<inert_transport::rustls::ClientConfig>, k: &SigningKey) -> Site {
    Site { host: HOST.into(), addr, tls, pinned_key: k.verifying_key().to_bytes(), dev: true }
}

// ------------------------------------------------------------------ happy path

#[tokio::test]
async fn happy_path_open_and_transfer() {
    let (site, counter) = real_server().await;
    let mut s = Session::open(&site, None).await.unwrap();
    assert_eq!(s.manifest.app, HOST);
    let ov = s.open_screen("overview", Fields::new()).await.unwrap();
    assert!(ov.data.contains_key("total"));
    let tr = s.open_screen("transfer", Fields::new()).await.unwrap();
    let mut f = Fields::new();
    f.insert("from".into(), Value::text("chk"));
    f.insert("to".into(), Value::text("DE89370400440532013000"));
    f.insert("amount".into(), Decimal::new(500, 2).to_value());
    f.insert("note".into(), Value::text(""));
    match s.action("transfer", &tr.data, "transfer", f).await.unwrap() {
        ActionOutcome::Ok(Some(next)) => assert_eq!(next.screen, "overview"),
        other => panic!("{other:?}"),
    }
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn business_rejection_points_at_field() {
    let (site, _) = real_server().await;
    let mut s = Session::open(&site, None).await.unwrap();
    let tr = s.open_screen("transfer", Fields::new()).await.unwrap();
    let mut f = Fields::new();
    f.insert("from".into(), Value::text("chk"));
    f.insert("to".into(), Value::text(""));
    f.insert("amount".into(), Decimal::new(500, 2).to_value());
    f.insert("note".into(), Value::text(""));
    assert_eq!(s.action("transfer", &tr.data, "transfer", f).await.unwrap(), ActionOutcome::Error { code: ErrorCode::Rejected, field: Some("to".into()) });
}

// ------------------------------------------------------------------ §15.1 attacks

/// 1. The server sends a password field the template does not have.
#[tokio::test]
async fn attack_1_injected_password_field_is_refused() {
    let k = key(1);
    let b = signed_bundle(&k);
    let (addr, tls) = evil_server(welcome_for(&b), |_| transfer_screen_with(Some(("password", Value::text(""))), false)).await;
    let mut s = Session::open(&site(addr, tls, &k), None).await.unwrap();
    let e = s.open_screen("transfer", Fields::new()).await.unwrap_err();
    assert_eq!(e.code(), Some(ErrorCode::ScreenInvalid), "{e}");
}

/// 2. An extra key inside list data.
#[tokio::test]
async fn attack_2_extra_key_in_screen_data_is_refused() {
    let k = key(1);
    let b = signed_bundle(&k);
    let (addr, tls) = evil_server(welcome_for(&b), |_| transfer_screen_with(None, true)).await;
    let mut s = Session::open(&site(addr, tls, &k), None).await.unwrap();
    assert_eq!(s.open_screen("transfer", Fields::new()).await.unwrap_err().code(), Some(ErrorCode::ScreenInvalid));
}

/// 3. One byte of the manifest changed.
#[tokio::test]
async fn attack_3_tampered_manifest_fails_signature() {
    let k = key(1);
    let b = signed_bundle(&k);
    let mut bytes = b.manifest_bytes.clone();
    let i = bytes.len() / 3;
    bytes[i] ^= 0x01;
    let w = Message::Welcome { v: PROTOCOL_VERSION, manifest: bytes, sig: b.sig, key_id: b.key_id.clone() };
    let (addr, tls) = evil_server(w, |_| Message::Fatal { code: ErrorCode::Internal }).await;
    let e = Session::open(&site(addr, tls, &k), None).await.err().unwrap();
    assert_eq!(e.code(), Some(ErrorCode::BadSignature), "{e}");
}

/// 4. The server presents a manifest signed with a different key.
#[tokio::test]
async fn attack_4_different_key_is_a_trust_warning() {
    let pinned = key(1);
    let attacker = key(2);
    let b = signed_bundle(&attacker);
    let (addr, tls) = evil_server(welcome_for(&b), |_| Message::Fatal { code: ErrorCode::Internal }).await;
    let e = Session::open(&site(addr, tls, &pinned), None).await.err().unwrap();
    assert!(matches!(e, ClientError::KeyChanged { .. }), "{e}");
}

/// 5. An older manifest version is replayed.
#[tokio::test]
async fn attack_5_rollback_is_refused() {
    let (site, _) = real_server().await;
    let e = Session::open(&site, Some(2)).await.err().unwrap();
    assert!(matches!(e, ClientError::Rollback { got: 1, seen: 2 }), "{e}");
}

/// 6. Low-contrast ("invisible") text never gets signed.
#[test]
fn attack_6_low_contrast_fails_the_build() {
    let src = std::fs::read_to_string(bank_json()).unwrap();
    let evil = src.replace("\"muted\": \"#9a9a9a\"", "\"muted\": \"#0a0a0a\"");
    assert_ne!(src, evil);
    let dir = std::env::temp_dir().join(format!("inert-contrast-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::copy(bank_json().parent().unwrap().join("assets/logo.png"), dir.join("assets/logo.png")).unwrap();
    std::fs::write(dir.join("app.inert.json"), evil).unwrap();
    let e = inert_server::bundle::build(&dir.join("app.inert.json"), &key(1)).err().unwrap();
    assert!(format!("{e:#}").contains("contrast"), "{e:#}");
    let _ = std::fs::remove_dir_all(dir);
}

/// 7. An asset whose bytes do not match the requested hash.
#[tokio::test]
async fn attack_7_asset_hash_mismatch_is_refused() {
    let k = key(1);
    let b = signed_bundle(&k);
    let (hash, meta) = b.manifest.assets.iter().next().map(|(h, m)| (*h, m.clone())).unwrap();
    let (addr, tls) = evil_server(welcome_for(&b), move |m| match m {
        Message::GetAsset { hash } => Message::Asset { hash: *hash, mime: meta.mime.clone(), bytes: vec![0x89; meta.bytes as usize] },
        _ => Message::Fatal { code: ErrorCode::Internal },
    })
    .await;
    let mut s = Session::open(&site(addr, tls, &k), None).await.unwrap();
    assert_eq!(s.asset(hash).await.unwrap_err().code(), Some(ErrorCode::AssetInvalid));
}

/// 8. The same action id twice runs the handler once.
#[tokio::test]
async fn attack_8_duplicate_action_id_runs_once() {
    let (site, counter) = real_server().await;
    let mut s = Session::open(&site, None).await.unwrap();
    s.open_screen("transfer", Fields::new()).await.unwrap();
    let mut f = Fields::new();
    f.insert("from".into(), Value::text("chk"));
    f.insert("to".into(), Value::text("DE89370400440532013000"));
    f.insert("amount".into(), Decimal::new(500, 2).to_value());
    f.insert("note".into(), Value::text(""));
    let id = [42u8; 16];
    let a = s.action_unchecked(id, "transfer", "transfer", f.clone()).await.unwrap();
    s.open_screen("transfer", Fields::new()).await.unwrap();
    let b = s.action_unchecked(id, "transfer", "transfer", f).await.unwrap();
    assert!(matches!(a, ActionOutcome::Ok(_)));
    assert_eq!(a, b, "the replayed result is the original one");
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

/// 9. A choice the screen never offered (someone else's account).
#[tokio::test]
async fn attack_9_unoffered_choice_is_refused_by_the_server() {
    let (site, counter) = real_server().await;
    let mut s = Session::open(&site, None).await.unwrap();
    s.open_screen("transfer", Fields::new()).await.unwrap();
    let mut f = Fields::new();
    f.insert("from".into(), Value::text("victim-account"));
    f.insert("to".into(), Value::text("DE89370400440532013000"));
    f.insert("amount".into(), Decimal::new(500, 2).to_value());
    f.insert("note".into(), Value::text(""));
    let r = s.action_unchecked([9; 16], "transfer", "transfer", f).await.unwrap();
    assert_eq!(r, ActionOutcome::Error { code: ErrorCode::FieldInvalid, field: Some("from".into()) });
    assert_eq!(counter.load(Ordering::SeqCst), 0);
}

/// 10. An oversized frame header: rejected before the body is read.
#[tokio::test]
async fn attack_10_oversized_frame_is_refused_unread() {
    let (site, _) = real_server().await;
    let tcp = tokio::net::TcpStream::connect(&site.addr).await.unwrap();
    let name = inert_transport::rustls::pki_types::ServerName::try_from(HOST.to_owned()).unwrap();
    let mut tls = inert_transport::tokio_rustls::TlsConnector::from(site.tls.clone()).connect(name, tcp).await.unwrap();
    tls.write_all(&(64u32 * 1024 * 1024).to_be_bytes()).await.unwrap();
    tls.flush().await.unwrap();
    let mut c = Conn::new(tls, Role::Client);
    assert_eq!(c.recv(false).await.unwrap(), Message::Fatal { code: ErrorCode::FrameTooLarge });
}

/// Extra: a correctly signed manifest for another host is refused.
#[tokio::test]
async fn manifest_for_another_host_is_refused() {
    let k = key(1);
    let b = signed_bundle(&k);
    let (addr, tls) = evil_server(welcome_for(&b), |_| Message::Fatal { code: ErrorCode::Internal }).await;
    let st = site(addr, tls, &k);
    // TLS already fails for a wrong host; check the manifest-level rule directly.
    let e =inert_verify::verify_welcome(&b.manifest_bytes, &b.sig, &b.key_id, &k.verifying_key().to_bytes(), "shop.example").unwrap_err();
    assert_eq!(e.code, ErrorCode::ManifestInvalid);
    assert!(Session::open(&st, None).await.is_ok());
}
