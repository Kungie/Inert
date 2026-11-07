//! Servo as the engine for the unsecured web, in its own process.
//!
//! The browser never links Servo. It spawns `inert-servo-host` (next to its
//! own executable), sends JSON-line commands to its stdin and reads events
//! from its stdout. The host renders offscreen and writes frames of the
//! active page into a shared memory-mapped file; the browser copies the
//! newest frame into a texture and draws it in the content area. Mouse,
//! wheel and keyboard input is forwarded. Shield (Brave's adblock engine)
//! runs inside the host. `inert://` links are handed back to the secured
//! engine. If the host dies, its tabs are dropped and it is restarted.

use crate::app::Event;
use eframe::egui::{self, Color32, ColorImage, Id, Rect, Sense, TextureHandle, TextureOptions, Ui};
use inert_servo_ipc::{Cmd, Ev, Input, SLOT_BYTES, SLOTS};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::rc::Rc;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

struct Frame {
    view: u64,
    image: ColorImage,
}

#[derive(Default)]
struct Shared {
    frame: Option<Frame>,
    cursor: HashMap<u64, String>,
    history: HashMap<u64, (bool, bool)>,
    /// Pending `read_text` requests: req → tab.
    /// Consent checks: req → tab.
    consents: HashMap<u64, u64>,
    reads: HashMap<u64, u64>,
    blocked: u64,
    shield_ready: bool,
    dead: bool,
}

struct Inner {
    stdin: RefCell<ChildStdin>,
    shared: Arc<Mutex<Shared>>,
    next_req: Cell<u64>,
    child: RefCell<Child>,
}

impl Inner {
    fn send(&self, c: &Cmd) {
        let mut s = self.stdin.borrow_mut();
        if let Ok(line) = serde_json::to_string(c) {
            let _ = s.write_all(line.as_bytes()).and_then(|_| s.write_all(b"\n")).and_then(|_| s.flush());
        }
    }
}

pub struct Host {
    inner: Rc<Inner>,
    size: Cell<(u32, u32)>,
    texture: RefCell<Option<TextureHandle>>,
    shown: Cell<u64>,
    adblock: Cell<Option<bool>>,
    pub pid: u32,
}

fn helper_path() -> std::path::PathBuf {
    if let Ok(p) = std::env::var("INERT_SERVO_HOST") {
        return p.into();
    }
    let exe = std::env::current_exe().unwrap_or_default();
    exe.with_file_name(if cfg!(windows) { "inert-servo-host.exe" } else { "inert-servo-host" })
}

impl Host {
    pub fn new(ectx: &egui::Context, profile_dir: std::path::PathBuf, tx: Sender<Event>) -> Result<Host, String> {
        let helper = helper_path();
        if !helper.exists() {
            return Err(format!("{} not found (build it with `cargo build -p inert-servo-host`)", helper.display()));
        }
        let shm = std::env::temp_dir().join(format!("inert-servo-{}.frames", std::process::id()));
        let mut child = Command::new(&helper)
            .arg("--profile")
            .arg(&profile_dir)
            .arg("--shm")
            .arg(&shm)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(if std::env::var_os("INERT_SERVO_LOG").is_some() { Stdio::inherit() } else { Stdio::null() })
            .spawn()
            .map_err(|e| format!("starting Servo: {e}"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let pid = child.id();
        let shared = Arc::new(Mutex::new(Shared::default()));
        {
            let shared = shared.clone();
            let ectx = ectx.clone();
            std::thread::Builder::new().name("servo-ipc".into()).spawn(move || reader(stdout, shared, tx, ectx)).map_err(|e| e.to_string())?;
        }
        Ok(Host { inner: Rc::new(Inner { stdin: RefCell::new(stdin), shared, next_req: Cell::new(1), child: RefCell::new(child) }), size: Cell::new((0, 0)), texture: RefCell::new(None), shown: Cell::new(0), adblock: Cell::new(None), pid })
    }

    /// True once the host process has exited (crash or kill).
    pub fn dead(&self) -> bool {
        self.inner.shared.lock().map(|s| s.dead).unwrap_or(true)
    }

    pub fn set_adblock(&self, on: bool) {
        if self.adblock.get() != Some(on) {
            self.adblock.set(Some(on));
            self.inner.send(&Cmd::Adblock { on });
        }
    }

    /// The filter lists are cached now: let the host build its engine.
    pub fn load_filters(&self) {
        self.inner.send(&Cmd::LoadFilters);
    }

    /// (requests blocked this session, engine ready)
    /// Asks for the cookies Servo would send to `url`; the answer arrives as
    /// `Event::ServoCookies` with the returned request id.
    pub fn cookies(&self, url: &str) -> u64 {
        let req = self.inner.next_req.get() + 1;
        self.inner.next_req.set(req);
        self.inner.send(&Cmd::Cookies { req, url: url.to_owned() });
        req
    }

    /// Deletes cookies and site storage for these sites; empty = everything.
    pub fn clear_cookies(&self, sites: Vec<String>) {
        self.inner.send(&Cmd::ClearCookies { sites });
    }

    pub fn set_cookies(&self, list: Vec<(String, inert_servo_ipc::CookieJson)>) {
        self.inner.send(&Cmd::SetCookies { list });
    }

    pub fn shield(&self) -> (u64, bool) {
        self.inner.shared.lock().map(|s| (s.blocked, s.shield_ready)).unwrap_or((0, false))
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.inner.send(&Cmd::Quit);
        let mut c = self.inner.child.borrow_mut();
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(500);
        while std::time::Instant::now() < deadline {
            if let Ok(Some(_)) = c.try_wait() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let _ = c.kill();
        let _ = c.wait();
    }
}

fn reader(stdout: std::process::ChildStdout, shared: Arc<Mutex<Shared>>, tx: Sender<Event>, ectx: egui::Context) {
    let mut map: Option<memmap2::Mmap> = None;
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        let Ok(ev) = serde_json::from_str::<Ev>(&line) else { continue };
        let send = |e: Event| {
            let _ = tx.send(e);
        };
        match ev {
            Ev::Ready { shm } => {
                if let Ok(f) = std::fs::File::open(&shm) {
                    // SAFETY: the host created this file for us and only writes
                    // a slot before announcing it; we copy it out right away.
                    // A torn frame is possible under load, never a bad read:
                    // the file length is fixed for the host's lifetime.
                    map = unsafe { memmap2::Mmap::map(&f) }.ok();
                }
            }
            Ev::Frame { view, slot, w, h, .. } => {
                let Some(m) = &map else { continue };
                let len = w as usize * h as usize * 4;
                let start = (slot as u64 % SLOTS * SLOT_BYTES) as usize;
                if len as u64 > SLOT_BYTES || start + len > m.len() {
                    continue;
                }
                // One conversion straight out of shared memory; no extra copy.
                let image = ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &m[start..start + len]);
                if let Ok(mut s) = shared.lock() {
                    s.frame = Some(Frame { view, image });
                }
            }
            Ev::Load { view, started, url } => send(Event::WebLoad { tab: view, started, url }),
            Ev::Url { view, url } => send(Event::ServoUrl { tab: view, url }),
            Ev::Title { view, title } => send(Event::WebTitle { tab: view, title }),
            Ev::History { view, back, fwd } => {
                if let Ok(mut s) = shared.lock() {
                    s.history.insert(view, (back, fwd));
                }
            }
            Ev::Cursor { view, cursor } => {
                if let Ok(mut s) = shared.lock() {
                    s.cursor.insert(view, cursor);
                }
            }
            Ev::Navigate { view, url } => send(Event::Navigate { tab: view, url }),
            Ev::Eval { req, text } => {
                let consent = shared.lock().ok().and_then(|mut s| s.consents.remove(&req));
                if let Some(tab) = consent {
                    send(Event::Consent { tab, what: text });
                    continue;
                }
                let tab = shared.lock().ok().and_then(|mut s| s.reads.remove(&req));
                if let Some(tab) = tab {
                    send(Event::PageText { tab, text });
                }
            }
            Ev::Blocked { n } => {
                if let Ok(mut s) = shared.lock() {
                    s.blocked = n;
                }
            }
            Ev::ShieldReady => {
                if let Ok(mut s) = shared.lock() {
                    s.shield_ready = true;
                }
            }
            Ev::Cookies { req, list } => send(Event::ServoCookies { req, list }),
            Ev::Log { msg } => tracing::debug!("servo-host: {msg}"),
        }
        ectx.request_repaint();
    }
    if let Ok(mut s) = shared.lock() {
        s.dead = true;
    }
    ectx.request_repaint();
}

/// One Servo web view (a tab) living in the host process.
pub struct View {
    id: u64,
    inner: Rc<Inner>,
}

impl View {
    pub fn load(&self, url: &str) {
        self.inner.send(&Cmd::Load { view: self.id, url: url.to_owned() });
    }
    pub fn back(&self) {
        self.inner.send(&Cmd::Back { view: self.id });
    }
    pub fn forward(&self) {
        self.inner.send(&Cmd::Forward { view: self.id });
    }
    pub fn reload(&self) {
        self.inner.send(&Cmd::Reload { view: self.id });
    }
    pub fn zoom(&self, z: f32) {
        self.inner.send(&Cmd::Zoom { view: self.id, z });
    }
    pub fn eval(&self, script: &str) {
        self.inner.send(&Cmd::Eval { view: self.id, req: 0, script: script.to_owned() });
    }
    fn history(&self) -> (bool, bool) {
        self.inner.shared.lock().ok().and_then(|s| s.history.get(&self.id).copied()).unwrap_or((false, false))
    }
    pub fn can_back(&self) -> bool {
        self.history().0
    }
    pub fn can_fwd(&self) -> bool {
        self.history().1
    }
    fn input(&self, ev: Input) {
        self.inner.send(&Cmd::Input { view: self.id, ev });
    }
}

impl Drop for View {
    fn drop(&mut self) {
        self.inner.send(&Cmd::Close { view: self.id });
    }
}

pub fn create(host: &Host, tab: u64, url: &str) -> Result<View, String> {
    url::Url::parse(url).map_err(|e| e.to_string())?;
    host.inner.send(&Cmd::Create { view: tab, url: url.to_owned() });
    Ok(View { id: tab, inner: host.inner.clone() })
}

/// Asks what the cookie-banner script did on this page (`Event::Consent`).
pub fn read_consent(v: &View, tab: u64) {
    let req = v.inner.next_req.get() + 1000;
    v.inner.next_req.set(req + 1);
    if let Ok(mut s) = v.inner.shared.lock() {
        s.consents.insert(req, tab);
    }
    v.inner.send(&Cmd::Eval { view: v.id, req, script: crate::cookies::CONSENT_QUERY.into() });
}

/// Reads the page's visible text for the assistant (untrusted).
pub fn read_text(v: &View, tab: u64) {
    let req = v.inner.next_req.get();
    v.inner.next_req.set(req + 1);
    if let Ok(mut s) = v.inner.shared.lock() {
        s.reads.insert(req, tab);
    }
    v.inner.send(&Cmd::Eval { view: v.id, req, script: "(document.title + '\\n' + (document.body ? document.body.innerText : '')).slice(0, 24000)".into() });
}

fn named(k: egui::Key) -> Option<&'static str> {
    use egui::Key as K;
    Some(match k {
        K::Enter => "Enter",
        K::Backspace => "Backspace",
        K::Delete => "Delete",
        K::Tab => "Tab",
        K::Escape => "Escape",
        K::ArrowUp => "ArrowUp",
        K::ArrowDown => "ArrowDown",
        K::ArrowLeft => "ArrowLeft",
        K::ArrowRight => "ArrowRight",
        K::Home => "Home",
        K::End => "End",
        K::PageUp => "PageUp",
        K::PageDown => "PageDown",
        _ => return None,
    })
}

/// Draws the active Servo view into `rect` and forwards input to it.
/// `overlay` is true while browser popovers cover the page.
pub fn show(ui: &mut Ui, host: &Host, tab: u64, v: &View, overlay: bool) {
    let rect = ui.max_rect();
    let ppp = ui.ctx().pixels_per_point();
    let want = ((rect.width() * ppp).round().max(1.0) as u32, (rect.height() * ppp).round().max(1.0) as u32);
    if host.shown.get() != tab {
        host.shown.set(tab);
        host.inner.send(&Cmd::Show { view: tab });
        host.size.set((0, 0));
    }
    if host.size.get() != want {
        host.size.set(want);
        host.inner.send(&Cmd::Resize { w: want.0, h: want.1 });
    }
    let frame = host.inner.shared.lock().ok().and_then(|mut s| if s.frame.as_ref().is_some_and(|f| f.view == tab) { s.frame.take() } else { None });
    if let Some(f) = frame {
        let ci = f.image;
        let mut t = host.texture.borrow_mut();
        match t.as_mut() {
            Some(tex) => tex.set(ci, TextureOptions::LINEAR),
            None => *t = Some(ui.ctx().load_texture("servo-surface", ci, TextureOptions::LINEAR)),
        }
    }
    let p = ui.painter();
    p.rect_filled(rect, 0.0, Color32::WHITE);
    if let Some(t) = host.texture.borrow().as_ref() {
        // Draw at the frame's own size so a stale frame during resize isn't stretched.
        let [tw, th] = t.size();
        let r = Rect::from_min_size(rect.min, egui::vec2(tw as f32 / ppp, th as f32 / ppp)).intersect(rect);
        let uv = Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(r.width() * ppp / tw as f32, r.height() * ppp / th as f32));
        p.image(t.id(), r, uv, Color32::WHITE);
    }
    if overlay {
        return;
    }

    let id = Id::unique(("servo-surface", tab));
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    if resp.is_pointer_button_down_on() || resp.clicked() {
        if !ui.memory(|m| m.has_focus(id)) {
            v.input(Input::Focus);
        }
        resp.request_focus();
    }
    // egui's own focus (not the OS window's): keys go where egui routes them.
    let focused = ui.memory(|m| m.has_focus(id));
    if resp.hovered() {
        let c = host.inner.shared.lock().ok().and_then(|s| s.cursor.get(&tab).cloned()).unwrap_or_default();
        ui.ctx().set_cursor_icon(match c.as_str() {
            "pointer" => egui::CursorIcon::PointingHand,
            "text" => egui::CursorIcon::Text,
            "wait" => egui::CursorIcon::Wait,
            "crosshair" => egui::CursorIcon::Crosshair,
            "not-allowed" => egui::CursorIcon::NotAllowed,
            _ => egui::CursorIcon::Default,
        });
    }
    let at = |p: egui::Pos2| ((p.x - rect.left()) * ppp, (p.y - rect.top()) * ppp);
    let events = ui.input(|i| i.events.clone());
    for ev in events {
        match ev {
            egui::Event::PointerMoved(pos) if rect.contains(pos) => {
                let (x, y) = at(pos);
                v.input(Input::Move { x, y });
            }
            egui::Event::PointerButton { pos, button, pressed, .. } if rect.contains(pos) => {
                let button = match button {
                    egui::PointerButton::Primary => 0,
                    egui::PointerButton::Middle => 1,
                    egui::PointerButton::Secondary => 2,
                    egui::PointerButton::Extra1 => 3,
                    egui::PointerButton::Extra2 => 4,
                };
                let (x, y) = at(pos);
                v.input(Input::Button { x, y, button, down: pressed });
            }
            egui::Event::MouseWheel { unit, delta, .. } if resp.hovered() => {
                let (mode, scale) = match unit {
                    egui::MouseWheelUnit::Point => (0, ppp as f64),
                    egui::MouseWheelUnit::Line => (1, 1.0),
                    egui::MouseWheelUnit::Page => (2, 1.0),
                };
                let (x, y) = at(ui.ctx().pointer_latest_pos().unwrap_or(rect.center()));
                // DOM convention: positive deltaY scrolls down.
                v.input(Input::Wheel { x, y, dx: -(delta.x as f64) * scale, dy: -(delta.y as f64) * scale, mode });
            }
            egui::Event::Text(s) | egui::Event::Paste(s) if focused => v.input(Input::Text { s }),
            egui::Event::Key { key, pressed, modifiers: m, .. } if focused => {
                if let Some(k) = named(key) {
                    v.input(Input::Key { key: k.into(), down: pressed, shift: m.shift, ctrl: m.ctrl, alt: m.alt, meta: m.mac_cmd });
                }
            }
            _ => {}
        }
    }
    // Keep Tab and arrows inside the page while it has focus.
    if focused {
        ui.memory_mut(|mem| mem.set_focus_lock_filter(id, egui::EventFilter { tab: true, horizontal_arrows: true, vertical_arrows: true, escape: false }));
    }
}
