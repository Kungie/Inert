//! Servo as the engine for the unsecured web.
//!
//! Servo renders offscreen into a `SoftwareRenderingContext`; each new frame
//! is read back and drawn into the browser's content area as a texture.
//! Mouse, wheel and keyboard input is forwarded from egui. Shield blocks ads
//! and trackers by intercepting Servo's resource loads with Brave's adblock
//! engine. `inert://` links are handed to the secured engine.

use crate::app::Event;
use eframe::egui::{self, Color32, ColorImage, Id, Rect, Sense, TextureHandle, TextureOptions, Ui};
use servo::{
    DeviceIntRect, DeviceIntSize, DevicePoint, EventLoopWaker, InputEvent, Key, KeyState, KeyboardEvent, LoadStatus, MouseButton, MouseButtonAction, MouseButtonEvent, MouseMoveEvent, NamedKey,
    RenderingContext, Servo, ServoBuilder, SoftwareRenderingContext, WebView, WebViewBuilder, WebViewDelegate, WheelDelta, WheelEvent, WheelMode,
};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::mpsc::Sender;

#[derive(Clone)]
struct Waker(egui::Context);

impl EventLoopWaker for Waker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(self.clone())
    }
    fn wake(&self) {
        self.0.request_repaint();
    }
}

/// Shared ad blocker for Servo tabs.
#[derive(Default)]
pub struct Blocker {
    pub engine: RefCell<Option<adblock::Engine>>,
    pub enabled: Cell<bool>,
    pub blocked: Cell<u64>,
}

pub struct Host {
    pub servo: Servo,
    ctx: Rc<SoftwareRenderingContext>,
    size: Cell<(u32, u32)>,
    texture: RefCell<Option<TextureHandle>>,
    painted_for: Cell<u64>,
    /// Frames left to repaint after a resize (static pages send no new frame).
    settle: Cell<u8>,
    pub blocker: Rc<Blocker>,
}

impl Host {
    pub fn new(ectx: &egui::Context, profile_dir: std::path::PathBuf) -> Result<Host, String> {
        let ctx = Rc::new(SoftwareRenderingContext::new(dpi::PhysicalSize::new(1024, 768)).map_err(|e| format!("{e:?}"))?);
        ctx.make_current().map_err(|e| format!("{e:?}"))?;
        let mut prefs = servo::Preferences::default();
        prefs.network_http_proxy_uri = String::new();
        prefs.network_https_proxy_uri = String::new();
        prefs.user_agent = crate::web::USER_AGENT.replace("Version/18.6 Safari/605.1.15", "Servo/0.7 InertBrowser/0.1");
        // Cookies, local storage and cache live in the browser profile, so
        // sign-ins on Servo tabs survive restarts (demo profiles are throwaway).
        let dir = profile_dir.join("servo");
        let _ = std::fs::create_dir_all(&dir);
        let opts = servo::Opts { config_dir: Some(dir), ..Default::default() };
        let servo = ServoBuilder::default().opts(opts).preferences(prefs).event_loop_waker(Box::new(Waker(ectx.clone()))).build();
        Ok(Host { servo, ctx, size: Cell::new((1024, 768)), texture: RefCell::new(None), painted_for: Cell::new(0), settle: Cell::new(0), blocker: Rc::new(Blocker::default()) })
    }

    pub fn spin(&self) {
        self.servo.spin_event_loop();
    }
}

pub struct View {
    pub view: WebView,
    frame: Rc<Cell<bool>>,
    cursor: Rc<Cell<servo::Cursor>>,
    pub can_back: Rc<Cell<bool>>,
    pub can_fwd: Rc<Cell<bool>>,
}

struct Delegate {
    tab: u64,
    tx: Sender<Event>,
    ectx: egui::Context,
    frame: Rc<Cell<bool>>,
    cursor: Rc<Cell<servo::Cursor>>,
    can_back: Rc<Cell<bool>>,
    can_fwd: Rc<Cell<bool>>,
    blocker: Rc<Blocker>,
}

impl Delegate {
    fn send(&self, e: Event) {
        let _ = self.tx.send(e);
        self.ectx.request_repaint();
    }
}

impl WebViewDelegate for Delegate {
    fn notify_new_frame_ready(&self, _w: WebView) {
        self.frame.set(true);
        self.ectx.request_repaint();
    }

    fn notify_load_status_changed(&self, w: WebView, status: LoadStatus) {
        let url = w.url().map(|u| u.to_string()).unwrap_or_default();
        match status {
            LoadStatus::Started => self.send(Event::WebLoad { tab: self.tab, started: true, url }),
            LoadStatus::Complete => self.send(Event::WebLoad { tab: self.tab, started: false, url }),
            _ => {}
        }
    }

    fn notify_url_changed(&self, _w: WebView, url: url::Url) {
        self.send(Event::ServoUrl { tab: self.tab, url: url.to_string() });
    }

    fn notify_page_title_changed(&self, _w: WebView, title: Option<String>) {
        if let Some(title) = title {
            self.send(Event::WebTitle { tab: self.tab, title });
        }
    }

    fn notify_cursor_changed(&self, _w: WebView, c: servo::Cursor) {
        self.cursor.set(c);
    }

    fn notify_history_changed(&self, _w: WebView, entries: Vec<url::Url>, current: usize) {
        self.can_back.set(current > 0);
        self.can_fwd.set(current + 1 < entries.len());
        self.ectx.request_repaint();
    }

    fn request_navigation(&self, _w: WebView, nav: servo::NavigationRequest) {
        if nav.url.scheme() == "inert" {
            let url = nav.url.to_string();
            nav.deny();
            self.send(Event::Navigate { tab: self.tab, url });
        } else {
            nav.allow();
        }
    }

    fn request_create_new(&self, _parent: WebView, _req: servo::CreateNewWebViewRequest) {
        // Pop-ups are not opened as separate windows; links with
        // target=_blank still navigate through request_navigation.
    }

    fn load_web_resource(&self, _w: WebView, load: servo::WebResourceLoad) {
        if !self.blocker.enabled.get() {
            return;
        }
        let req = load.request();
        if req.is_for_main_frame {
            return;
        }
        let url = req.url.to_string();
        let source = req.referrer_url.as_ref().map(|u| u.to_string()).unwrap_or_default();
        let kind = match format!("{:?}", req.destination).to_ascii_lowercase().as_str() {
            "script" => "script",
            "image" => "image",
            "style" => "stylesheet",
            "font" => "font",
            "audio" | "video" | "track" => "media",
            "iframe" | "frame" => "subdocument",
            "document" => "document",
            "" | "none" => "xmlhttprequest",
            _ => "other",
        };
        let blocked = self.blocker.engine.borrow().as_ref().and_then(|e| adblock::request::Request::new(&url, &source, kind, "GET").ok().map(|r| e.check_network_request(&r).should_block())).unwrap_or(false);
        if blocked {
            self.blocker.blocked.set(self.blocker.blocked.get() + 1);
            let resp = servo::WebResourceResponse::new(req.url.clone()).status_code(http::StatusCode::FORBIDDEN);
            load.intercept(resp).finish();
        }
    }
}

pub fn create(host: &Host, tab: u64, url: &str, tx: &Sender<Event>, ectx: &egui::Context) -> Result<View, String> {
    let u = url::Url::parse(url).map_err(|e| e.to_string())?;
    let frame = Rc::new(Cell::new(true));
    let cursor = Rc::new(Cell::new(servo::Cursor::Default));
    let can_back = Rc::new(Cell::new(false));
    let can_fwd = Rc::new(Cell::new(false));
    let d = Rc::new(Delegate { tab, tx: tx.clone(), ectx: ectx.clone(), frame: frame.clone(), cursor: cursor.clone(), can_back: can_back.clone(), can_fwd: can_fwd.clone(), blocker: host.blocker.clone() });
    let view = WebViewBuilder::new(&host.servo, host.ctx.clone()).url(u).delegate(d).build();
    Ok(View { view, frame, cursor, can_back, can_fwd })
}

/// Reads the page's visible text for the assistant (untrusted).
pub fn read_text(v: &View, tab: u64, tx: &Sender<Event>, ectx: &egui::Context) {
    let tx = tx.clone();
    let ectx = ectx.clone();
    v.view.evaluate_javascript("(document.title + '\\n' + (document.body ? document.body.innerText : '')).slice(0, 24000)", move |r| {
        let text = match r {
            Ok(servo::JSValue::String(s)) => s,
            _ => String::new(),
        };
        let _ = tx.send(Event::PageText { tab, text });
        ectx.request_repaint();
    });
}

fn point(rect: Rect, p: egui::Pos2, ppp: f32) -> servo::WebViewPoint {
    DevicePoint::new((p.x - rect.left()) * ppp, (p.y - rect.top()) * ppp).into()
}

fn named(k: egui::Key) -> Option<NamedKey> {
    use egui::Key as K;
    Some(match k {
        K::Enter => NamedKey::Enter,
        K::Backspace => NamedKey::Backspace,
        K::Delete => NamedKey::Delete,
        K::Tab => NamedKey::Tab,
        K::Escape => NamedKey::Escape,
        K::ArrowUp => NamedKey::ArrowUp,
        K::ArrowDown => NamedKey::ArrowDown,
        K::ArrowLeft => NamedKey::ArrowLeft,
        K::ArrowRight => NamedKey::ArrowRight,
        K::Home => NamedKey::Home,
        K::End => NamedKey::End,
        K::PageUp => NamedKey::PageUp,
        K::PageDown => NamedKey::PageDown,
        _ => return None,
    })
}

fn modifiers(m: egui::Modifiers) -> servo::Modifiers {
    let mut out = servo::Modifiers::empty();
    if m.shift {
        out |= servo::Modifiers::SHIFT;
    }
    if m.ctrl {
        out |= servo::Modifiers::CONTROL;
    }
    if m.alt {
        out |= servo::Modifiers::ALT;
    }
    if m.mac_cmd {
        out |= servo::Modifiers::META;
    }
    out
}

fn key_event(state: KeyState, key: Key, m: egui::Modifiers) -> InputEvent {
    let mut e = KeyboardEvent::from_state_and_key(state, key);
    e.event.modifiers = modifiers(m);
    InputEvent::Keyboard(e)
}

/// Draws the active Servo view into `rect` and forwards input to it.
/// `overlay` is true while browser popovers cover the page.
pub fn show(ui: &mut Ui, host: &Host, tab: u64, v: &View, overlay: bool) {
    let rect = ui.max_rect();
    let ppp = ui.ctx().pixels_per_point();
    let want = ((rect.width() * ppp).round().max(1.0) as u32, (rect.height() * ppp).round().max(1.0) as u32);
    if host.size.get() != want {
        host.size.set(want);
        // Only through the WebView: Servo resizes the context and the layout
        // viewport together, and skips the viewport if the context already matches.
        v.view.resize(dpi::PhysicalSize::new(want.0, want.1));
        host.settle.set(30);
    }
    if v.view.size().width.round() as u32 != want.0 || v.view.size().height.round() as u32 != want.1 {
        v.view.resize(dpi::PhysicalSize::new(want.0, want.1));
        host.settle.set(30);
    }
    if host.painted_for.get() != tab {
        host.painted_for.set(tab);
        host.settle.set(30);
    }
    let settling = host.settle.get() > 0;
    if settling {
        host.settle.set(host.settle.get() - 1);
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
    }
    if v.frame.replace(false) || settling {
        let _ = host.ctx.make_current();
        v.view.paint();
        if let Some(img) = host.ctx.read_to_image(DeviceIntRect::from_size(DeviceIntSize::new(want.0 as i32, want.1 as i32))) {
            let ci = ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
            let mut t = host.texture.borrow_mut();
            match t.as_mut() {
                Some(tex) => tex.set(ci, TextureOptions::LINEAR),
                None => *t = Some(ui.ctx().load_texture("servo-surface", ci, TextureOptions::LINEAR)),
            }
        }
    }
    let p = ui.painter();
    p.rect_filled(rect, 0.0, Color32::WHITE);
    if let Some(t) = host.texture.borrow().as_ref() {
        p.image(t.id(), rect, Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
    }
    if overlay {
        return;
    }

    let id = Id::unique(("servo-surface", tab));
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    if resp.is_pointer_button_down_on() || resp.clicked() {
        resp.request_focus();
        v.view.set_focused(true);
    }
    // egui's own focus (not the OS window's): keys go where egui routes them.
    let focused = ui.memory(|m| m.has_focus(id));
    if resp.hovered() {
        ui.ctx().set_cursor_icon(match v.cursor.get() {
            servo::Cursor::Pointer => egui::CursorIcon::PointingHand,
            servo::Cursor::Text => egui::CursorIcon::Text,
            servo::Cursor::Wait | servo::Cursor::Progress => egui::CursorIcon::Wait,
            servo::Cursor::Crosshair => egui::CursorIcon::Crosshair,
            servo::Cursor::NotAllowed => egui::CursorIcon::NotAllowed,
            _ => egui::CursorIcon::Default,
        });
    }
    let events = ui.input(|i| i.events.clone());
    for ev in events {
        match ev {
            egui::Event::PointerMoved(pos) if rect.contains(pos) => {
                v.view.notify_input_event(InputEvent::MouseMove(MouseMoveEvent::new(point(rect, pos, ppp))));
            }
            egui::Event::PointerButton { pos, button, pressed, .. } if rect.contains(pos) => {
                let b = match button {
                    egui::PointerButton::Primary => MouseButton::Primary,
                    egui::PointerButton::Secondary => MouseButton::Secondary,
                    egui::PointerButton::Middle => MouseButton::Auxiliary,
                    egui::PointerButton::Extra1 => MouseButton::Back,
                    egui::PointerButton::Extra2 => MouseButton::Forward,
                };
                let a = if pressed { MouseButtonAction::Down } else { MouseButtonAction::Up };
                v.view.notify_input_event(InputEvent::MouseButton(MouseButtonEvent::new(a, b, point(rect, pos, ppp))));
            }
            egui::Event::MouseWheel { unit, delta, .. } if resp.hovered() => {
                let (mode, scale) = match unit {
                    egui::MouseWheelUnit::Point => (WheelMode::DeltaPixel, ppp as f64),
                    egui::MouseWheelUnit::Line => (WheelMode::DeltaLine, 1.0),
                    egui::MouseWheelUnit::Page => (WheelMode::DeltaPage, 1.0),
                };
                let at = ui.ctx().pointer_latest_pos().unwrap_or(rect.center());
                // DOM convention: positive deltaY scrolls down.
                let d = WheelDelta { x: -(delta.x as f64) * scale, y: -(delta.y as f64) * scale, z: 0.0, mode };
                v.view.notify_input_event(InputEvent::Wheel(WheelEvent::new(d, point(rect, at, ppp))));
            }
            egui::Event::Text(t) if focused => {
                for ch in t.chars() {
                    let k = Key::Character(ch.to_string());
                    v.view.notify_input_event(key_event(KeyState::Down, k.clone(), egui::Modifiers::NONE));
                    v.view.notify_input_event(key_event(KeyState::Up, k, egui::Modifiers::NONE));
                }
            }
            egui::Event::Paste(t) if focused => {
                for ch in t.chars() {
                    let k = Key::Character(ch.to_string());
                    v.view.notify_input_event(key_event(KeyState::Down, k.clone(), egui::Modifiers::NONE));
                    v.view.notify_input_event(key_event(KeyState::Up, k, egui::Modifiers::NONE));
                }
            }
            egui::Event::Key { key, pressed, modifiers: m, .. } if focused => {
                if let Some(nk) = named(key) {
                    v.view.notify_input_event(key_event(if pressed { KeyState::Down } else { KeyState::Up }, Key::Named(nk), m));
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

/// Builds the ad blocking engine from the cached filter lists (background thread).
pub fn build_blocker(tx: Sender<Event>, ectx: egui::Context) {
    std::thread::spawn(move || {
        let dir = dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("Inert").join("adblock");
        let mut set = adblock::lists::FilterSet::new(false);
        let mut any = false;
        for name in ["easylist", "easyprivacy"] {
            if let Ok(t) = std::fs::read_to_string(dir.join(format!("{name}.txt"))) {
                set.add_filter_list(t, adblock::lists::ParseOptions::default());
                any = true;
            }
        }
        if any {
            let engine = adblock::Engine::new_with_filter_set(set);
            let _ = tx.send(Event::ServoBlocker(Box::new(engine)));
            ectx.request_repaint();
        }
    });
}
