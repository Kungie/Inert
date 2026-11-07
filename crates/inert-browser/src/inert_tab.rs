//! `inert://` tabs: one protocol session per tab on the tokio runtime.
//! Everything the UI receives from here has already been verified by
//! `inert-client` (signature, pin, rules, screen/template conformance).

use crate::profile::Pin;
use eframe::egui;
use inert_client::{ActionOutcome, ClientError, Session, Site};
use inert_proto::*;
use inert_transport::trust::{DevTrust, default_dev_trust_dir, load_dir};
use std::collections::{BTreeMap, HashMap};
use std::sync::Arc;
use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

pub enum InertCmd {
    Open { screen: String, params: Fields },
    Action { screen: String, data: Fields, action: String, fields: Fields },
    Asset(Hash),
}

pub enum InertEvent {
    Connected { manifest: Arc<Manifest>, key_id: String },
    Screen(ScreenMsg),
    ActionDone(ActionOutcome),
    Asset { hash: Hash, mime: String, bytes: Vec<u8> },
    Failed(Failure),
}

#[derive(Clone, Debug)]
pub enum Failure {
    /// No trust record (no DNS discovery in the vertical slice).
    NoTrust,
    KeyChanged { pinned: String, offered: String },
    Rollback { got: u64, seen: u64 },
    Protocol { code: Option<ErrorCode>, detail: String },
    Network(String),
}

impl Failure {
    pub fn from_client(e: ClientError) -> Self {
        match e {
            ClientError::KeyChanged { pinned, offered } => Failure::KeyChanged { pinned, offered },
            ClientError::Rollback { got, seen } => Failure::Rollback { got, seen },
            ClientError::Fatal { code, detail } => Failure::Protocol { code: Some(code), detail },
            ClientError::Transport(t) => match t.code() {
                Some(code) => Failure::Protocol { code: Some(code), detail: t.to_string() },
                None => Failure::Network(t.to_string()),
            },
        }
    }
}

#[derive(Clone, Debug)]
pub enum FormValue {
    Text(String),
    Choice(String),
    Bool(bool),
}

pub enum Img {
    Loading,
    Ready(egui::TextureHandle),
    Broken,
}

#[derive(PartialEq, Eq, Clone, Copy, Debug)]
pub enum Status {
    Connecting,
    Ready,
    Failed,
}

pub struct InertTab {
    pub host: String,
    pub cmd: Option<UnboundedSender<InertCmd>>,
    pub status: Status,
    pub failure: Option<Failure>,
    pub manifest: Option<Arc<Manifest>>,
    pub key_id: String,
    pub dev: bool,
    pub screen: Option<ScreenMsg>,
    pub params: Fields,
    pub pending_open: Option<(String, Fields)>,
    pub forms: HashMap<(String, String), FormValue>,
    pub field_errors: HashMap<String, String>,
    pub banner: Option<String>,
    pub images: HashMap<Hash, Img>,
    pub busy: bool,
    pub back: Vec<(String, Fields)>,
    pub fwd: Vec<(String, Fields)>,
    pub zoom: f32,
    pub confirm_href: Option<String>,
}

impl InertTab {
    pub fn new(host: &str, screen: Option<String>, params: Fields) -> Self {
        InertTab {
            host: host.to_owned(),
            cmd: None,
            status: Status::Connecting,
            failure: None,
            manifest: None,
            key_id: String::new(),
            dev: true,
            screen: None,
            params: Fields::new(),
            pending_open: screen.map(|s| (s, params)),
            forms: HashMap::new(),
            field_errors: HashMap::new(),
            banner: None,
            images: HashMap::new(),
            busy: false,
            back: Vec::new(),
            fwd: Vec::new(),
            zoom: 1.0,
            confirm_href: None,
        }
    }

    pub fn url(&self) -> String {
        match &self.screen {
            Some(s) if self.manifest.as_ref().is_some_and(|m| m.entry != s.screen || !self.params.is_empty()) => {
                let mut u = format!("inert://{}/{}", self.host, s.screen);
                if !self.params.is_empty() {
                    let q: Vec<String> = self.params.iter().map(|(k, v)| format!("{k}={}", value_to_query(v))).collect();
                    u.push('?');
                    u.push_str(&q.join("&"));
                }
                u
            }
            _ => format!("inert://{}", self.host),
        }
    }

    pub fn title(&self) -> String {
        match (&self.manifest, &self.screen) {
            (Some(m), Some(s)) => {
                let t = m.screens.get(&s.screen).map(|d| d.title.clone()).unwrap_or_default();
                if s.screen == m.entry { m.name.clone() } else { format!("{t} · {}", m.name) }
            }
            (Some(m), None) => m.name.clone(),
            _ => self.host.clone(),
        }
    }

    pub fn send(&self, c: InertCmd) {
        if let Some(tx) = &self.cmd {
            let _ = tx.send(c);
        }
    }

    pub fn open(&mut self, screen: &str, params: Fields, push_history: bool) {
        if push_history {
            if let Some(s) = &self.screen {
                self.back.push((s.screen.clone(), self.params.clone()));
                self.fwd.clear();
            }
        }
        self.busy = true;
        self.field_errors.clear();
        self.banner = None;
        self.pending_open = Some((screen.to_owned(), params.clone()));
        self.send(InertCmd::Open { screen: screen.to_owned(), params });
    }

    pub fn go_back(&mut self) -> bool {
        if let Some((s, p)) = self.back.pop() {
            if let Some(cur) = &self.screen {
                self.fwd.push((cur.screen.clone(), self.params.clone()));
            }
            self.open(&s, p, false);
            true
        } else {
            false
        }
    }

    pub fn go_forward(&mut self) -> bool {
        if let Some((s, p)) = self.fwd.pop() {
            if let Some(cur) = &self.screen {
                self.back.push((cur.screen.clone(), self.params.clone()));
            }
            self.open(&s, p, false);
            true
        } else {
            false
        }
    }

    /// Initial form values for a freshly received screen (from `init`).
    pub fn reset_forms(&mut self) {
        self.forms.clear();
        let (Some(m), Some(s)) = (&self.manifest, &self.screen) else { return };
        let Some(def) = m.screens.get(&s.screen) else { return };
        let data = &s.data;
        let resolve = |a: &Option<ArgSrc>| -> Option<Value> {
            match a {
                Some(ArgSrc::Lit(v)) => Some(v.clone()),
                Some(ArgSrc::Bind(p)) => data.get(p).cloned(),
                None => None,
            }
        };
        let mut out = HashMap::new();
        def.root.walk(&mut |n, _, _| match n {
            Node::Input { action, field, init, .. } => {
                let t = match resolve(init) {
                    Some(Value::Text(s)) => s,
                    Some(v @ (Value::Uint(_) | Value::Nint(_))) => v.as_i64().map(|n| n.to_string()).unwrap_or_default(),
                    Some(v @ Value::Array(_)) => Decimal::from_value(&v).map(|d| d.to_string()).unwrap_or_default(),
                    _ => String::new(),
                };
                out.insert((action.clone(), field.clone()), FormValue::Text(t));
            }
            Node::Select { action, field, init, .. } => {
                let ft = m.actions.get(action).and_then(|a| a.fields.get(field));
                let first = match ft {
                    Some(FieldType::Enum { options }) => options.first().map(|o| o.value.clone()),
                    Some(FieldType::Choice { from }) => data.get(from).and_then(Value::as_array).and_then(|a| a.first()).and_then(|i| i.get("id")).and_then(Value::as_text).map(str::to_owned),
                    _ => None,
                };
                let v = resolve(init).and_then(|v| v.as_text().map(str::to_owned)).or(first).unwrap_or_default();
                out.insert((action.clone(), field.clone()), FormValue::Choice(v));
            }
            Node::Toggle { action, field, init, .. } => {
                let v = resolve(init).and_then(|v| v.as_bool()).unwrap_or(false);
                out.insert((action.clone(), field.clone()), FormValue::Bool(v));
            }
            _ => {}
        });
        self.forms = out;
    }

    /// Plain-text rendering of the signed screen for the AI panel and agents.
    pub fn semantic_text(&self) -> String {
        let (Some(m), Some(s)) = (&self.manifest, &self.screen) else { return String::new() };
        let Some(def) = m.screens.get(&s.screen) else { return String::new() };
        let mut out = format!("Site: inert://{} ({}), screen: {} — {}\n", m.app, m.name, s.screen, def.title);
        fn txt(v: &Value) -> String {
            match v {
                Value::Text(s) => s.clone(),
                Value::Uint(_) | Value::Nint(_) => v.as_i64().map(|n| n.to_string()).unwrap_or_default(),
                Value::Array(_) => Decimal::from_value(v).map(|d| d.to_string()).unwrap_or_default(),
                Value::Bool(b) => b.to_string(),
                _ => String::new(),
            }
        }
        fn go(n: &Node, data: &Fields, item: Option<&Value>, out: &mut String, m: &Manifest) {
            let get = |p: &str| -> String {
                if let Some(rest) = p.strip_prefix("item.") { item.and_then(|i| i.get(rest)).map(txt).unwrap_or_default() } else { data.get(p).map(txt).unwrap_or_default() }
            };
            match n {
                Node::Stack { children, dir, .. } => {
                    if *dir == Dir::H {
                        let mut parts = Vec::new();
                        for c in children {
                            let mut s = String::new();
                            go(c, data, item, &mut s, m);
                            let s = s.trim().replace('\n', " ");
                            if !s.is_empty() {
                                parts.push(s);
                            }
                        }
                        out.push_str(&parts.join(" | "));
                        out.push('\n');
                    } else {
                        for c in children {
                            go(c, data, item, out, m);
                        }
                    }
                }
                Node::Text { text, .. } => {
                    let t = match text {
                        TextSrc::Lit(s) => s.clone(),
                        TextSrc::Bind(p) => get(p),
                    };
                    out.push_str(&t);
                    out.push('\n');
                }
                Node::List { bind, item: tpl } => {
                    if let Some(items) = data.get(bind).and_then(Value::as_array) {
                        for it in items {
                            out.push_str("- ");
                            let mut s = String::new();
                            go(tpl, data, Some(it), &mut s, m);
                            out.push_str(s.trim());
                            out.push('\n');
                        }
                    }
                }
                Node::Button { label, action, .. } => out.push_str(&format!("[button: {label} → action {action}]\n")),
                Node::Input { label, field, .. } => out.push_str(&format!("[input: {label} ({field})]\n")),
                Node::Select { label, field, .. } => out.push_str(&format!("[select: {label} ({field})]\n")),
                Node::Toggle { label, field, .. } => out.push_str(&format!("[toggle: {label} ({field})]\n")),
                Node::Link { label, .. } => out.push_str(&format!("[link: {label}]\n")),
                Node::Image { alt, .. } => out.push_str(&format!("[image: {alt}]\n")),
                Node::Divider => out.push_str("---\n"),
            }
        }
        go(&def.root, &s.data, None, &mut out, m);
        out
    }
}

fn value_to_query(v: &Value) -> String {
    let s = match v {
        Value::Text(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        other => other.as_i64().map(|n| n.to_string()).unwrap_or_default(),
    };
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

/// Parses `inert://host[/screen][?k=v…]`.
pub fn parse_url(u: &str) -> Option<(String, Option<String>, Vec<(String, String)>)> {
    let rest = u.strip_prefix("inert://")?;
    let (hp, query) = rest.split_once('?').map(|(a, b)| (a, Some(b))).unwrap_or((rest, None));
    let (host, path) = hp.split_once('/').map(|(a, b)| (a, Some(b))).unwrap_or((hp, None));
    let host = host.trim().to_ascii_lowercase();
    if host.is_empty() {
        return None;
    }
    let screen = path.map(|p| p.trim_matches('/').to_owned()).filter(|p| !p.is_empty());
    let params = query.map(|q| url::form_urlencoded::parse(q.as_bytes()).into_owned().collect()).unwrap_or_default();
    Some((host, screen, params))
}

/// Converts query strings into typed params per the screen definition.
pub fn typed_params(m: &Manifest, screen: &str, raw: &[(String, String)]) -> Fields {
    let mut out = Fields::new();
    let Some(def) = m.screens.get(screen) else { return out };
    for (k, v) in raw {
        let Some(t) = def.params.get(k) else { continue };
        let val = match t {
            FieldType::Int { .. } => v.parse::<i64>().ok().map(Value::int),
            FieldType::Bool => v.parse::<bool>().ok().map(Value::Bool),
            FieldType::Decimal { .. } => Decimal::parse(v).map(|d| d.to_value()),
            _ => Some(Value::text(v.clone())),
        };
        if let Some(val) = val {
            out.insert(k.clone(), val);
        }
    }
    out
}

/// All sites with a dev trust file (shown on the new tab page).
pub fn known_sites() -> Vec<DevTrust> {
    load_dir(&default_dev_trust_dir())
}

/// Resolves a host to a dialable [`Site`], applying the pin. Returns a new pin
/// to store when this is the first visit.
pub fn resolve(host: &str, pins: &BTreeMap<String, Pin>) -> Result<(Site, Option<Pin>), Failure> {
    let trust = known_sites().into_iter().find(|t| t.host == host).ok_or(Failure::NoTrust)?;
    let ca = trust.ca_der().ok_or(Failure::NoTrust)?;
    let tls = inert_transport::tls::client_config_dev(&ca).map_err(Failure::Network)?;
    let file_key = trust.manifest_key().ok_or(Failure::NoTrust)?;
    let (pinned_key, new_pin) = match pins.get(host) {
        Some(p) => {
            use base64::Engine;
            let k: [u8; 32] = base64::engine::general_purpose::STANDARD.decode(&p.key_b64).ok().and_then(|b| b.try_into().ok()).ok_or(Failure::NoTrust)?;
            (k, None)
        }
        None => (file_key, Some(Pin { key_b64: trust.manifest_key_b64.clone(), pinned_at: crate::profile::now(), max_version: 0 })),
    };
    Ok((Site { host: host.to_owned(), addr: trust.addr.clone(), tls, pinned_key, dev: true }, new_pin))
}

pub type EventTx = std::sync::mpsc::Sender<crate::app::Event>;

/// Starts the session task for a tab.
pub fn spawn(rt: &tokio::runtime::Handle, tab: u64, site: Site, seen_version: Option<u64>, tx: EventTx, ctx: egui::Context) -> UnboundedSender<InertCmd> {
    let (ctx_tx, mut rx) = unbounded_channel::<InertCmd>();
    rt.spawn(async move {
        let emit = |e: InertEvent| {
            let _ = tx.send(crate::app::Event::Inert(tab, e));
            ctx.request_repaint();
        };
        let mut s = match Session::open(&site, seen_version).await {
            Ok(s) => s,
            Err(e) => return emit(InertEvent::Failed(Failure::from_client(e))),
        };
        emit(InertEvent::Connected { manifest: s.manifest.clone(), key_id: s.key_id.clone() });
        while let Some(cmd) = rx.recv().await {
            let r = match cmd {
                InertCmd::Open { screen, params } => s.open_screen(&screen, params).await.map(InertEvent::Screen),
                InertCmd::Action { screen, data, action, fields } => s.action(&screen, &data, &action, fields).await.map(InertEvent::ActionDone),
                InertCmd::Asset(h) => s.asset(h).await.map(|(mime, bytes)| InertEvent::Asset { hash: h, mime, bytes }),
            };
            match r {
                Ok(ev) => emit(ev),
                Err(e) => return emit(InertEvent::Failed(Failure::from_client(e))),
            }
        }
    });
    ctx_tx
}

/// Decodes a verified asset with memory-safe decoders and pixel limits.
pub fn decode_image(mime: &str, bytes: &[u8]) -> Option<egui::ColorImage> {
    match mime {
        "image/png" => {
            let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
            dec.set_transformations(png::Transformations::normalize_to_color8());
            let mut r = dec.read_info().ok()?;
            let info = r.info();
            if !inert_client::image_dims_ok(info.width as u64, info.height as u64) {
                return None;
            }
            let mut buf = vec![0; r.output_buffer_size()];
            let fi = r.next_frame(&mut buf).ok()?;
            let (w, h) = (fi.width as usize, fi.height as usize);
            let data = &buf[..fi.buffer_size()];
            let rgba: Vec<u8> = match fi.color_type {
                png::ColorType::Rgba => data.to_vec(),
                png::ColorType::Rgb => data.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
                png::ColorType::Grayscale => data.iter().flat_map(|&g| [g, g, g, 255]).collect(),
                png::ColorType::GrayscaleAlpha => data.chunks_exact(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
                _ => return None,
            };
            Some(egui::ColorImage::from_rgba_unmultiplied([w, h], &rgba))
        }
        "image/webp" => {
            let mut d = image_webp::WebPDecoder::new(std::io::Cursor::new(bytes)).ok()?;
            let (w, h) = d.dimensions();
            if !inert_client::image_dims_ok(w as u64, h as u64) {
                return None;
            }
            let alpha = d.has_alpha();
            let mut buf = vec![0; d.output_buffer_size()?];
            d.read_image(&mut buf).ok()?;
            let rgba: Vec<u8> = if alpha { buf } else { buf.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect() };
            Some(egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba))
        }
        _ => None,
    }
}
