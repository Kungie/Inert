//! The server runtime (spec §10.2): handler registry, coverage check,
//! per-connection session loop with validation on both edges.

use crate::bundle::Bundle;
use crate::json::{AssetFile, hash_bytes, image_info};
use crate::val::{Data, Params};
use anyhow::{Result, anyhow, bail};
use inert_proto::limits::{IMAGE_MAX_BYTES, IMAGE_MAX_PIXELS, IMAGE_MAX_SIDE, REMEMBERED_ACTION_IDS};
use inert_proto::{ErrorCode, Fields, Hash, Message, PROTOCOL_VERSION, ResultError, ScreenMsg};
use inert_transport::{ServerConn, TransportError};
use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex, RwLock};

/// A business-rule refusal from a handler (e.g. insufficient funds).
#[derive(Debug, Clone)]
pub struct Reject {
    pub field: Option<String>,
}

impl Reject {
    pub fn on(field: &str) -> Self {
        Self { field: Some(field.to_owned()) }
    }
    pub fn general() -> Self {
        Self { field: None }
    }
}

/// What to show after an action.
pub enum Next {
    /// Open another (or the same) screen with these params.
    Screen(String, Fields),
    /// Re-render the screen the action came from with its current params.
    Stay,
}

impl Next {
    pub fn screen(name: &str) -> Self {
        Next::Screen(name.to_owned(), Fields::new())
    }
}

type ScreenFn<S> = Box<dyn Fn(&mut S, Params<'_>) -> std::result::Result<Data, Reject> + Send + Sync>;
type ActionFn<S> = Box<dyn Fn(&mut S, Params<'_>) -> std::result::Result<Next, Reject> + Send + Sync>;

pub struct App<S> {
    bundle: Arc<RwLock<Arc<Bundle>>>,
    screens: BTreeMap<String, ScreenFn<S>>,
    actions: BTreeMap<String, ActionFn<S>>,
    dynamic_assets: Arc<RwLock<HashMap<Hash, AssetFile>>>,
    state: Arc<Mutex<S>>,
}

impl<S: Send + 'static> App<S> {
    pub fn new(bundle: Bundle, state: S) -> Self {
        Self {
            bundle: Arc::new(RwLock::new(Arc::new(bundle))),
            screens: BTreeMap::new(),
            actions: BTreeMap::new(),
            dynamic_assets: Arc::default(),
            state: Arc::new(Mutex::new(state)),
        }
    }

    pub fn screen(&mut self, name: &str, f: impl Fn(&mut S, Params<'_>) -> std::result::Result<Data, Reject> + Send + Sync + 'static) -> &mut Self {
        self.screens.insert(name.to_owned(), Box::new(f));
        self
    }

    pub fn action(&mut self, name: &str, f: impl Fn(&mut S, Params<'_>) -> std::result::Result<Next, Reject> + Send + Sync + 'static) -> &mut Self {
        self.actions.insert(name.to_owned(), Box::new(f));
        self
    }

    /// Registers a dynamic image (not in the manifest) and returns its hash.
    pub fn register_image(&self, bytes: Vec<u8>) -> Result<Hash> {
        let (mime, w, h) = image_info(&bytes)?;
        if bytes.len() as u64 > IMAGE_MAX_BYTES || w > IMAGE_MAX_SIDE || h > IMAGE_MAX_SIDE || w * h > IMAGE_MAX_PIXELS {
            bail!("image exceeds protocol limits");
        }
        let hash = hash_bytes(&bytes);
        self.dynamic_assets.write().unwrap().insert(hash, AssetFile { hash, mime: mime.into(), bytes });
        Ok(hash)
    }

    /// Direct access to the shared state (setup, tests).
    pub fn state(&self) -> std::sync::MutexGuard<'_, S> {
        lock(&self.state)
    }

    pub fn bundle(&self) -> Arc<Bundle> {
        self.bundle.read().unwrap().clone()
    }

    /// Checks that handlers cover exactly the manifest's screens and actions.
    pub fn check_coverage(&self, b: &Bundle) -> Result<()> {
        let ms: BTreeSet<_> = b.manifest.screens.keys().collect();
        let hs: BTreeSet<_> = self.screens.keys().collect();
        let ma: BTreeSet<_> = b.manifest.actions.keys().collect();
        let ha: BTreeSet<_> = self.actions.keys().collect();
        let mut problems = Vec::new();
        for s in ms.difference(&hs) {
            problems.push(format!("screen {s:?} has no handler"));
        }
        for s in hs.difference(&ms) {
            problems.push(format!("handler for unknown screen {s:?}"));
        }
        for a in ma.difference(&ha) {
            problems.push(format!("action {a:?} has no handler"));
        }
        for a in ha.difference(&ma) {
            problems.push(format!("handler for unknown action {a:?}"));
        }
        if problems.is_empty() { Ok(()) } else { Err(anyhow!("handler coverage: {}", problems.join("; "))) }
    }

    /// Swaps in a new bundle (dev hot reload). Fails if handlers don't cover it.
    pub fn replace_bundle(&self, b: Bundle) -> Result<()> {
        self.check_coverage(&b)?;
        *self.bundle.write().unwrap() = Arc::new(b);
        Ok(())
    }

    pub fn into_shared(self) -> Result<Arc<Self>> {
        self.check_coverage(&self.bundle())?;
        Ok(Arc::new(self))
    }
}

/// Runs the accept loop forever.
pub async fn serve<S: Send + 'static>(app: Arc<App<S>>, addr: &str, tls: Arc<inert_transport::rustls::ServerConfig>) -> Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    let acceptor = inert_transport::tokio_rustls::TlsAcceptor::from(tls);
    tracing::info!("serving {} on {addr}", app.bundle().manifest.app);
    loop {
        let (tcp, peer) = listener.accept().await?;
        let app = app.clone();
        let acceptor = acceptor.clone();
        tokio::spawn(async move {
            match inert_transport::handshake(tcp, &acceptor).await {
                Ok(conn) => {
                    if let Err(e) = session(app, conn).await {
                        tracing::debug!("{peer}: session ended: {e}");
                    }
                }
                Err(e) => tracing::debug!("{peer}: handshake failed: {e}"),
            }
        });
    }
}

struct SessionState {
    bundle: Arc<Bundle>,
    current: Option<(String, Fields)>,
    last_data: HashMap<String, Fields>,
    seen: VecDeque<([u8; 16], Message)>,
}

/// Serves one connection. Public so tests can drive it over any stream.
pub async fn session<S: Send + 'static>(app: Arc<App<S>>, mut conn: ServerConn) -> Result<()> {
    run_session(app, &mut conn).await
}

async fn run_session<S: Send + 'static, T>(app: Arc<App<S>>, conn: &mut inert_transport::Conn<T>) -> Result<()>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    // hello → welcome
    match conn.recv(false).await {
        Ok(Message::Hello { v }) if v == PROTOCOL_VERSION => {}
        Ok(Message::Hello { .. }) => {
            conn.fatal(ErrorCode::UnsupportedVersion).await;
            bail!("unsupported version");
        }
        Ok(_) => {
            conn.fatal(ErrorCode::UnknownMessage).await;
            bail!("expected hello");
        }
        Err(e) => return fail(conn, e).await,
    }
    let bundle = app.bundle();
    conn.send(&Message::Welcome { v: PROTOCOL_VERSION, manifest: bundle.manifest_bytes.clone(), sig: bundle.sig, key_id: bundle.key_id.clone() }).await?;
    let mut st = SessionState { bundle, current: None, last_data: HashMap::new(), seen: VecDeque::new() };

    loop {
        let msg = match conn.recv(true).await {
            Ok(m) => m,
            Err(TransportError::Closed) => return Ok(()),
            Err(e) => return fail(conn, e).await,
        };
        match msg {
            Message::Open { screen, params } => {
                if let Err(e) = inert_verify::check_open(&st.bundle.manifest, &screen, &params) {
                    conn.fatal(ErrorCode::ScreenInvalid).await;
                    bail!("bad open: {e}");
                }
                match render(&app, &mut st, &screen, params) {
                    Ok(s) => conn.send(&Message::Screen(s)).await?,
                    Err(code) => {
                        conn.fatal(code).await;
                        bail!("render failed");
                    }
                }
            }
            Message::Action { id, screen, action, fields } => {
                if let Some((_, prev)) = st.seen.iter().find(|(i, _)| *i == id) {
                    // Duplicate (e.g. resent after a reconnect race): replay, don't re-run.
                    let prev = prev.clone();
                    conn.send(&prev).await?;
                    continue;
                }
                let reply = handle_action(&app, &mut st, id, &screen, &action, &fields);
                st.seen.push_back((id, reply.clone()));
                if st.seen.len() > REMEMBERED_ACTION_IDS {
                    st.seen.pop_front();
                }
                conn.send(&reply).await?;
            }
            Message::GetAsset { hash } => {
                let a = st.bundle.assets.get(&hash).cloned().or_else(|| app.dynamic_assets.read().unwrap().get(&hash).cloned());
                match a {
                    Some(a) => conn.send(&Message::Asset { hash, mime: a.mime, bytes: a.bytes }).await?,
                    None => {
                        conn.fatal(ErrorCode::AssetInvalid).await;
                        bail!("unknown asset");
                    }
                }
            }
            other => {
                conn.fatal(ErrorCode::UnknownMessage).await;
                bail!("unexpected {}", other.type_name());
            }
        }
    }
}

async fn fail<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin>(conn: &mut inert_transport::Conn<T>, e: TransportError) -> Result<()> {
    if let Some(code) = e.code() {
        conn.fatal(code).await;
    }
    Err(e.into())
}

fn lock<S>(m: &Mutex<S>) -> std::sync::MutexGuard<'_, S> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

/// Calls the screen handler and checks its output against the template.
fn render<S: Send + 'static>(app: &App<S>, st: &mut SessionState, screen: &str, params: Fields) -> std::result::Result<ScreenMsg, ErrorCode> {
    let f = app.screens.get(screen).ok_or(ErrorCode::Internal)?;
    let out = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let mut s = lock(&app.state);
        f(&mut s, Params(&params))
    }));
    let data = match out {
        Ok(Ok(d)) => d.into_fields(),
        Ok(Err(_)) | Err(_) => {
            tracing::error!("screen handler {screen:?} failed");
            return Err(ErrorCode::Internal);
        }
    };
    let msg = ScreenMsg { screen: screen.to_owned(), data };
    if let Err(e) = inert_verify::check_screen(&st.bundle.manifest, &msg) {
        // Server-side self check: never send a screen the browser would reject.
        tracing::error!("screen handler {screen:?} produced invalid data: {e}");
        return Err(ErrorCode::Internal);
    }
    st.last_data.insert(screen.to_owned(), msg.data.clone());
    st.current = Some((screen.to_owned(), params));
    Ok(msg)
}

fn result_err(id: [u8; 16], code: ErrorCode, field: Option<String>) -> Message {
    Message::Result { id, ok: false, error: Some(ResultError { code, field }), next: None }
}

fn handle_action<S: Send + 'static>(app: &App<S>, st: &mut SessionState, id: [u8; 16], screen: &str, action: &str, fields: &Fields) -> Message {
    if let Err(e) = inert_verify::check_action(&st.bundle.manifest, screen, st.last_data.get(screen), action, fields) {
        return result_err(id, e.code, e.field);
    }
    let Some(f) = app.actions.get(action) else { return result_err(id, ErrorCode::ActionUnknown, None) };
    let out = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let mut s = lock(&app.state);
        f(&mut s, Params(fields))
    }));
    let next = match out {
        Ok(Ok(n)) => n,
        Ok(Err(r)) => return result_err(id, ErrorCode::Rejected, r.field),
        Err(_) => {
            tracing::error!("action handler {action:?} panicked");
            return result_err(id, ErrorCode::Internal, None);
        }
    };
    let (name, params) = match next {
        Next::Screen(n, p) => (n, p),
        Next::Stay => st.current.clone().filter(|(n, _)| n == screen).unwrap_or((screen.to_owned(), Fields::new())),
    };
    if inert_verify::check_open(&st.bundle.manifest, &name, &params).is_err() {
        tracing::error!("action {action:?} navigated to an invalid screen {name:?}");
        return result_err(id, ErrorCode::Internal, None);
    }
    match render(app, st, &name, params) {
        Ok(s) => Message::Result { id, ok: true, error: None, next: Some(s) },
        Err(code) => result_err(id, code, None),
    }
}

/// Drives a session over an arbitrary stream (used by tests).
pub async fn session_over<S: Send + 'static, T>(app: Arc<App<S>>, mut conn: inert_transport::Conn<T>) -> Result<()>
where
    T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    run_session(app, &mut conn).await
}
