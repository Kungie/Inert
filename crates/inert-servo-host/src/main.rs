//! `inert-servo-host`: Servo in its own process for the Inert browser.
//!
//! Usage (spawned by the browser): `inert-servo-host --profile <dir> --shm <file>`
//!
//! - stdin: JSON-line `Cmd`s, stdout: JSON-line `Ev`s (see `inert-servo-ipc`).
//! - Frames of the active view go into a two-slot memory-mapped file.
//! - Shield: subresource loads are checked with Brave's adblock engine.
//! - `inert://` navigations are refused and handed back to the browser.
//! - Exits when stdin closes (the browser went away).

use inert_servo_ipc::{Cmd, Ev, Input, SLOT_BYTES, SLOTS};
use servo::{
    DeviceIntRect, DeviceIntSize, DevicePoint, EventLoopWaker, InputEvent, Key, KeyState, KeyboardEvent, LoadStatus, MouseButton, MouseButtonAction, MouseButtonEvent, MouseMoveEvent, NamedKey,
    RenderingContext, ServoBuilder, SoftwareRenderingContext, WebView, WebViewBuilder, WebViewDelegate, WheelDelta, WheelEvent, WheelMode,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

type Wake = Arc<(Mutex<bool>, Condvar)>;

#[derive(Clone)]
struct Waker(Wake);

impl EventLoopWaker for Waker {
    fn clone_box(&self) -> Box<dyn EventLoopWaker> {
        Box::new(self.clone())
    }
    fn wake(&self) {
        let (m, c) = &*self.0;
        *m.lock().unwrap() = true;
        c.notify_one();
    }
}

#[derive(Default)]
struct Blocker {
    engine: RefCell<Option<adblock::Engine>>,
    enabled: Cell<bool>,
    blocked: Cell<u64>,
    /// Sites where the user turned blocking off.
    allow: RefCell<Vec<String>>,
}

struct Delegate {
    id: u64,
    out: Sender<Ev>,
    frame: Rc<Cell<bool>>,
    blocker: Rc<Blocker>,
    /// The context menu the page is waiting on, until the browser answers.
    menu: Rc<RefCell<Option<servo::ContextMenu>>>,
    /// Host of the page this view shows (for per-site blocking).
    page: RefCell<String>,
}

const MENU_ACTIONS: [(&str, servo::ContextMenuAction); 11] = [
    ("GoBack", servo::ContextMenuAction::GoBack),
    ("GoForward", servo::ContextMenuAction::GoForward),
    ("Reload", servo::ContextMenuAction::Reload),
    ("CopyLink", servo::ContextMenuAction::CopyLink),
    ("OpenLinkInNewWebView", servo::ContextMenuAction::OpenLinkInNewWebView),
    ("CopyImageLink", servo::ContextMenuAction::CopyImageLink),
    ("OpenImageInNewView", servo::ContextMenuAction::OpenImageInNewView),
    ("Cut", servo::ContextMenuAction::Cut),
    ("Copy", servo::ContextMenuAction::Copy),
    ("Paste", servo::ContextMenuAction::Paste),
    ("SelectAll", servo::ContextMenuAction::SelectAll),
];

fn cursor_name(c: servo::Cursor) -> &'static str {
    match c {
        servo::Cursor::Pointer => "pointer",
        servo::Cursor::Text => "text",
        servo::Cursor::Wait | servo::Cursor::Progress => "wait",
        servo::Cursor::Crosshair => "crosshair",
        servo::Cursor::NotAllowed => "not-allowed",
        _ => "default",
    }
}

impl WebViewDelegate for Delegate {
    fn notify_new_frame_ready(&self, _w: WebView) {
        self.frame.set(true);
    }
    fn notify_load_status_changed(&self, w: WebView, s: LoadStatus) {
        let url = w.url().map(|u| u.to_string()).unwrap_or_default();
        match s {
            LoadStatus::Started => {
                let _ = self.out.send(Ev::Load { view: self.id, started: true, url });
            }
            LoadStatus::Complete => {
                let _ = self.out.send(Ev::Load { view: self.id, started: false, url });
            }
            _ => {}
        }
    }
    fn notify_url_changed(&self, _w: WebView, url: url::Url) {
        *self.page.borrow_mut() = url.host_str().unwrap_or_default().to_ascii_lowercase();
        let _ = self.out.send(Ev::Url { view: self.id, url: url.to_string() });
    }
    fn notify_page_title_changed(&self, _w: WebView, title: Option<String>) {
        if let Some(title) = title {
            let _ = self.out.send(Ev::Title { view: self.id, title });
        }
    }
    fn notify_cursor_changed(&self, _w: WebView, c: servo::Cursor) {
        let _ = self.out.send(Ev::Cursor { view: self.id, cursor: cursor_name(c).into() });
    }
    fn notify_history_changed(&self, _w: WebView, entries: Vec<url::Url>, current: usize) {
        let _ = self.out.send(Ev::History { view: self.id, back: current > 0, fwd: current + 1 < entries.len() });
    }
    fn request_navigation(&self, _w: WebView, nav: servo::NavigationRequest) {
        if nav.url.scheme() == "inert" {
            let url = nav.url.to_string();
            nav.deny();
            let _ = self.out.send(Ev::Navigate { view: self.id, url });
        } else {
            nav.allow();
        }
    }
    fn request_create_new(&self, _p: WebView, _r: servo::CreateNewWebViewRequest) {}
    fn show_embedder_control(&self, _w: WebView, control: servo::EmbedderControl) {
        // Context menus are drawn by the browser; it answers with MenuPick.
        if let servo::EmbedderControl::ContextMenu(m) = control {
            let actions = m
                .items()
                .iter()
                .filter_map(|i| match i {
                    servo::ContextMenuItem::Item { action, enabled: true, .. } => MENU_ACTIONS.iter().find(|(_, a)| a == action).map(|(n, _)| (*n).to_owned()),
                    _ => None,
                })
                .collect();
            let info = m.element_info();
            let flags = info.flags;
            let _ = self.out.send(Ev::Menu {
                view: self.id,
                actions,
                link: info.link_url.as_ref().map(|u| u.to_string()),
                image: info.image_url.as_ref().map(|u| u.to_string()),
                selection: flags.contains(servo::ContextMenuElementInformationFlags::Selection),
                editable: flags.contains(servo::ContextMenuElementInformationFlags::EditableText),
            });
            *self.menu.borrow_mut() = Some(m);
        }
    }
    fn hide_embedder_control(&self, _w: WebView, _id: servo::EmbedderControlId) {
        self.menu.borrow_mut().take();
    }
    fn load_web_resource(&self, _w: WebView, load: servo::WebResourceLoad) {
        if !self.blocker.enabled.get() {
            return;
        }
        // The user turned blocking off for this page's site.
        {
            let page = self.page.borrow();
            if !page.is_empty() && self.blocker.allow.borrow().iter().any(|s| *page == *s || page.ends_with(&format!(".{s}"))) {
                return;
            }
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
            let _ = self.out.send(Ev::Blocked { n: self.blocker.blocked.get() });
            let resp = servo::WebResourceResponse::new(req.url.clone()).status_code(http::StatusCode::FORBIDDEN);
            load.intercept(resp).finish();
        }
    }
}

fn arg(args: &[String], k: &str) -> Option<String> {
    args.iter().position(|a| a == k).and_then(|i| args.get(i + 1)).cloned()
}

fn named(k: &str) -> Option<NamedKey> {
    Some(match k {
        "Enter" => NamedKey::Enter,
        "Backspace" => NamedKey::Backspace,
        "Delete" => NamedKey::Delete,
        "Tab" => NamedKey::Tab,
        "Escape" => NamedKey::Escape,
        "ArrowUp" => NamedKey::ArrowUp,
        "ArrowDown" => NamedKey::ArrowDown,
        "ArrowLeft" => NamedKey::ArrowLeft,
        "ArrowRight" => NamedKey::ArrowRight,
        "Home" => NamedKey::Home,
        "End" => NamedKey::End,
        "PageUp" => NamedKey::PageUp,
        "PageDown" => NamedKey::PageDown,
        _ => return None,
    })
}

fn key(state: KeyState, k: Key, mods: servo::Modifiers) -> InputEvent {
    let mut e = KeyboardEvent::from_state_and_key(state, k);
    e.event.modifiers = mods;
    InputEvent::Keyboard(e)
}

fn to_servo(ev: Input) -> Vec<InputEvent> {
    let p = |x: f32, y: f32| -> servo::WebViewPoint { DevicePoint::new(x, y).into() };
    match ev {
        Input::Move { x, y } => vec![InputEvent::MouseMove(MouseMoveEvent::new(p(x, y)))],
        Input::Button { x, y, button, down } => {
            let b = match button {
                0 => MouseButton::Primary,
                1 => MouseButton::Auxiliary,
                2 => MouseButton::Secondary,
                3 => MouseButton::Back,
                _ => MouseButton::Forward,
            };
            vec![InputEvent::MouseButton(MouseButtonEvent::new(if down { MouseButtonAction::Down } else { MouseButtonAction::Up }, b, p(x, y)))]
        }
        Input::Wheel { x, y, dx, dy, mode } => {
            let mode = match mode {
                1 => WheelMode::DeltaLine,
                2 => WheelMode::DeltaPage,
                _ => WheelMode::DeltaPixel,
            };
            vec![InputEvent::Wheel(WheelEvent::new(WheelDelta { x: dx, y: dy, z: 0.0, mode }, p(x, y)))]
        }
        Input::Key { key: k, down, shift, ctrl, alt, meta } => {
            let Some(nk) = named(&k) else { return vec![] };
            let mut m = servo::Modifiers::empty();
            if shift {
                m |= servo::Modifiers::SHIFT;
            }
            if ctrl {
                m |= servo::Modifiers::CONTROL;
            }
            if alt {
                m |= servo::Modifiers::ALT;
            }
            if meta {
                m |= servo::Modifiers::META;
            }
            vec![key(if down { KeyState::Down } else { KeyState::Up }, Key::Named(nk), m)]
        }
        Input::Text { s } => s
            .chars()
            .flat_map(|c| {
                let k = Key::Character(c.to_string());
                [key(KeyState::Down, k.clone(), servo::Modifiers::empty()), key(KeyState::Up, k, servo::Modifiers::empty())]
            })
            .collect(),
        Input::Edit { action } => match action.as_str() {
            "copy" => vec![InputEvent::EditingAction(servo::ClipboardAction::Copy)],
            "cut" => vec![InputEvent::EditingAction(servo::ClipboardAction::Cut)],
            "paste" => vec![InputEvent::EditingAction(servo::ClipboardAction::Paste)],
            "select_all" => {
                let k = Key::Character("a".into());
                vec![key(KeyState::Down, k.clone(), servo::Modifiers::META), key(KeyState::Up, k, servo::Modifiers::META)]
            }
            _ => vec![],
        },
        Input::Focus => vec![],
    }
}

fn build_blocker() -> Receiver<adblock::Engine> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let dir = dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("Inert").join("adblock");
        let mut set = adblock::lists::FilterSet::new(false);
        let mut any = false;
        for name in ["easylist", "easyprivacy", "ublock-filters", "ublock-unbreak", "ublock-quick-fixes", "ublock-badware", "ublock-privacy"] {
            if let Ok(t) = std::fs::read_to_string(dir.join(format!("{name}.txt"))) {
                set.add_filter_list(t, adblock::lists::ParseOptions::default());
                any = true;
            }
        }
        if any {
            let _ = tx.send(adblock::Engine::new_with_filter_set(set));
        }
    });
    rx
}

struct Entry {
    view: WebView,
    frame: Rc<Cell<bool>>,
    menu: Rc<RefCell<Option<servo::ContextMenu>>>,
}

fn main() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let args: Vec<String> = std::env::args().collect();
    let profile = PathBuf::from(arg(&args, "--profile").unwrap_or_else(|| std::env::temp_dir().to_string_lossy().into_owned()));
    let shm_path = PathBuf::from(arg(&args, "--shm").unwrap_or_else(|| std::env::temp_dir().join("inert-servo-frames").to_string_lossy().into_owned()));

    // Output: one writer thread owns stdout.
    let (out, out_rx) = channel::<Ev>();
    std::thread::spawn(move || {
        let stdout = std::io::stdout();
        for ev in out_rx {
            let mut lock = stdout.lock();
            if serde_json::to_writer(&mut lock, &ev).is_err() || lock.write_all(b"\n").is_err() || lock.flush().is_err() {
                std::process::exit(0);
            }
        }
    });

    // Input: commands from stdin wake the main loop.
    let wake: Wake = Arc::new((Mutex::new(false), Condvar::new()));
    let (cmd_tx, cmd_rx) = channel::<Option<Cmd>>();
    {
        let wake = wake.clone();
        std::thread::spawn(move || {
            let stdin = std::io::stdin();
            for line in stdin.lock().lines() {
                let Ok(line) = line else { break };
                if let Ok(c) = serde_json::from_str::<Cmd>(&line) {
                    let _ = cmd_tx.send(Some(c));
                    Waker(wake.clone()).wake();
                }
            }
            let _ = cmd_tx.send(None);
            Waker(wake).wake();
        });
    }

    // Shared frame memory.
    let file = std::fs::OpenOptions::new().read(true).write(true).create(true).truncate(true).open(&shm_path).expect("frame file");
    file.set_len(SLOT_BYTES * SLOTS).expect("size frame file");
    // SAFETY: the file is private to this browser session; only the browser
    // maps it (read-only) and it only reads a slot after the Frame event.
    let mut shm = unsafe { memmap2::MmapMut::map_mut(&file) }.expect("map frame file");

    let ctx = Rc::new(SoftwareRenderingContext::new(dpi::PhysicalSize::new(1024, 768)).expect("software rendering context"));
    ctx.make_current().expect("make current");
    let servo_dir = profile.join("servo");
    let _ = std::fs::create_dir_all(&servo_dir);
    let mut prefs = servo::Preferences::default();
    prefs.network_http_proxy_uri = String::new();
    prefs.network_https_proxy_uri = String::new();
    prefs.user_agent = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Servo/0.7 InertBrowser/0.1".into();
    let servo = ServoBuilder::default()
        .opts(servo::Opts { config_dir: Some(servo_dir), ..Default::default() })
        .preferences(prefs)
        .event_loop_waker(Box::new(Waker(wake.clone())))
        .build();

    let blocker = Rc::new(Blocker::default());
    let mut blocker_rx = Some(build_blocker());
    let mut views: HashMap<u64, Entry> = HashMap::new();
    let mut active: Option<u64> = None;
    let mut size = (1024u32, 768u32);
    let mut scale = 1.0f32;
    let mut settle = 0u32;
    let mut seq = 0u64;
    let _ = out.send(Ev::Ready { shm: shm_path.to_string_lossy().into_owned() });

    'main: loop {
        {
            let (m, c) = &*wake;
            let mut flag = m.lock().unwrap();
            if !*flag {
                let wait = if settle > 0 { Duration::from_millis(16) } else { Duration::from_millis(250) };
                flag = c.wait_timeout(flag, wait).unwrap().0;
            }
            *flag = false;
        }
        servo.spin_event_loop();
        if let Some(rx) = &blocker_rx {
            match rx.try_recv() {
                Ok(e) => {
                    *blocker.engine.borrow_mut() = Some(e);
                    blocker_rx = None;
                    let _ = out.send(Ev::ShieldReady);
                }
                // No cached lists yet; LoadFilters retries later.
                Err(std::sync::mpsc::TryRecvError::Disconnected) => blocker_rx = None,
                Err(_) => {}
            }
        }
        while let Ok(c) = cmd_rx.try_recv() {
            let Some(c) = c else { break 'main };
            match c {
                Cmd::Create { view, url } => {
                    let Ok(u) = url::Url::parse(&url) else { continue };
                    let frame = Rc::new(Cell::new(true));
                    let menu = Rc::new(RefCell::new(None));
                    let d = Rc::new(Delegate { id: view, out: out.clone(), frame: frame.clone(), blocker: blocker.clone(), menu: menu.clone(), page: RefCell::new(u.host_str().unwrap_or_default().to_ascii_lowercase()) });
                    let wv = WebViewBuilder::new(&servo, ctx.clone()).url(u).delegate(d).hidpi_scale_factor(euclid::Scale::new(scale)).build();
                    // Pages see prefers-color-scheme: dark, like the rest of Inert.
                    wv.notify_theme_change(servo::Theme::Dark);
                    views.insert(view, Entry { view: wv, frame, menu });
                }
                Cmd::Load { view, url } => {
                    if let (Some(e), Ok(u)) = (views.get(&view), url::Url::parse(&url)) {
                        e.view.load(u);
                    }
                }
                Cmd::Show { view } => {
                    if active != Some(view) {
                        if let Some(old) = active.and_then(|a| views.get(&a)) {
                            old.view.hide();
                        }
                        if let Some(e) = views.get(&view) {
                            e.view.show();
                            e.view.set_hidpi_scale_factor(euclid::Scale::new(scale));
                            e.view.resize(dpi::PhysicalSize::new(size.0, size.1));
                            e.view.set_focused(true);
                        }
                        active = Some(view);
                        settle = 30;
                    }
                }
                Cmd::Close { view } => {
                    views.remove(&view);
                    if active == Some(view) {
                        active = None;
                    }
                }
                Cmd::Resize { w, h, scale: s } => {
                    size = (w.clamp(1, 3840), h.clamp(1, 2160));
                    scale = s.clamp(0.5, 4.0);
                    if let Some(e) = active.and_then(|a| views.get(&a)) {
                        e.view.set_hidpi_scale_factor(euclid::Scale::new(scale));
                        // Only through the WebView: Servo resizes context and viewport together.
                        e.view.resize(dpi::PhysicalSize::new(size.0, size.1));
                    }
                    settle = 30;
                }
                Cmd::Back { view } => {
                    if let Some(e) = views.get(&view) {
                        e.view.go_back(1);
                    }
                }
                Cmd::Forward { view } => {
                    if let Some(e) = views.get(&view) {
                        e.view.go_forward(1);
                    }
                }
                Cmd::Reload { view } => {
                    if let Some(e) = views.get(&view) {
                        e.view.reload();
                    }
                }
                Cmd::Zoom { view, z } => {
                    if let Some(e) = views.get(&view) {
                        e.view.set_page_zoom(z);
                    }
                }
                Cmd::Eval { view, req, script } => {
                    if let Some(e) = views.get(&view) {
                        let out = out.clone();
                        e.view.evaluate_javascript(script, move |r| {
                            let text = match r {
                                Ok(servo::JSValue::String(s)) => s,
                                _ => String::new(),
                            };
                            let _ = out.send(Ev::Eval { req, text });
                        });
                    }
                }
                Cmd::Input { view, ev } => {
                    if let Some(e) = views.get(&view) {
                        if matches!(ev, Input::Focus) {
                            e.view.set_focused(true);
                        }
                        for ie in to_servo(ev) {
                            e.view.notify_input_event(ie);
                        }
                    }
                }
                Cmd::Adblock { on } => blocker.enabled.set(on),
                Cmd::AdblockAllow { sites } => *blocker.allow.borrow_mut() = sites,
                Cmd::LoadFilters => {
                    if blocker.engine.borrow().is_none() && blocker_rx.is_none() {
                        blocker_rx = Some(build_blocker());
                    }
                }
                Cmd::Cookies { req, url } => {
                    let list = url::Url::parse(&url)
                        .map(|u| servo.site_data_manager().cookies_for_url(u, servo::CookieSource::HTTP))
                        .unwrap_or_default()
                        .iter()
                        .map(|c| inert_servo_ipc::CookieJson {
                            name: c.name().to_owned(),
                            value: String::new(),
                            domain: c.domain().unwrap_or("").to_owned(),
                            path: c.path().unwrap_or("/").to_owned(),
                            expires: c.expires_datetime().map(|t| t.unix_timestamp()),
                            secure: c.secure().unwrap_or(false),
                            http_only: c.http_only().unwrap_or(false),
                            same_site: c.same_site().map(|s| format!("{s:?}")),
                        })
                        .collect();
                    let _ = out.send(Ev::Cookies { req, list });
                }
                Cmd::ClearCookies { sites } => {
                    let all = servo::StorageType::Cookies | servo::StorageType::Local | servo::StorageType::Session;
                    if sites.is_empty() {
                        servo.site_data_manager().clear_cookies(None);
                        let names: Vec<String> = servo.site_data_manager().site_data(all).iter().map(|d| d.name()).collect();
                        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
                        servo.site_data_manager().clear_site_data(&refs, all);
                    } else {
                        let refs: Vec<&str> = sites.iter().map(String::as_str).collect();
                        servo.site_data_manager().clear_site_data(&refs, all);
                    }
                }
                Cmd::SetCookies { list } => {
                    for (u, c) in list {
                        let Ok(u) = url::Url::parse(&u) else { continue };
                        // Chrome marks a domain cookie with a leading dot. A host-only
                        // cookie must stay without a Domain attribute: with one,
                        // `__Host-` cookies are refused and sites that pair cookies
                        // (GitHub's session and its same-site twin) see half a login.
                        let mut b = cookie::Cookie::build((c.name, c.value)).path(c.path).secure(c.secure).http_only(c.http_only);
                        if c.domain.starts_with('.') {
                            b = b.domain(c.domain);
                        }
                        if let Some(t) = c.expires.and_then(|t| cookie::time::OffsetDateTime::from_unix_timestamp(t).ok()) {
                            b = b.expires(t);
                        }
                        b = match c.same_site.as_deref() {
                            Some("Strict") => b.same_site(cookie::SameSite::Strict),
                            Some("None") => b.same_site(cookie::SameSite::None),
                            Some("Lax") => b.same_site(cookie::SameSite::Lax),
                            _ => b,
                        };
                        servo.site_data_manager().set_cookie_for_url(u, b.build().into_owned(), None);
                    }
                }
                Cmd::MenuPick { view, action } => {
                    if let Some(m) = views.get(&view).and_then(|e| e.menu.borrow_mut().take()) {
                        match action.and_then(|a| MENU_ACTIONS.iter().find(|(n, _)| *n == a).map(|(_, x)| *x)) {
                            Some(a) => m.select(a),
                            None => m.dismiss(),
                        }
                    }
                }
                Cmd::Quit => break 'main,
            }
        }
        servo.spin_event_loop();

        // Paint the active view into the next slot.
        let Some(e) = active.and_then(|a| views.get(&a)) else { continue };
        let dirty = e.frame.replace(false);
        if !dirty && settle == 0 {
            continue;
        }
        settle = settle.saturating_sub(1);
        let _ = ctx.make_current();
        e.view.paint();
        let (w, h) = (size.0.min(3840), size.1.min(2160));
        if let Some(img) = ctx.read_to_image(DeviceIntRect::from_size(DeviceIntSize::new(w as i32, h as i32))) {
            let (iw, ih) = (img.width(), img.height());
            let bytes = img.as_raw();
            let slot = (seq % SLOTS) as usize;
            let start = slot * SLOT_BYTES as usize;
            if bytes.len() <= SLOT_BYTES as usize {
                shm[start..start + bytes.len()].copy_from_slice(bytes);
                seq += 1;
                let _ = out.send(Ev::Frame { view: active.unwrap_or(0), slot: slot as u8, w: iw, h: ih, seq });
            }
        }
    }
    drop(views);
    servo.spin_event_loop();
    drop(servo);
    let _ = std::fs::remove_file(&shm_path);
}
