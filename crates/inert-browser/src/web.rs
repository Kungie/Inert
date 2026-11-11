//! Normal web tabs ("unsecured"): native system web views (WKWebView on
//! macOS) embedded under the browser chrome. Web content runs in WebKit's own
//! sandboxed content processes and never sees the Inert core, pins or AI keys.

use crate::app::Event;
use eframe::egui;
use std::sync::mpsc::Sender;
use std::time::Instant;

/// Safari-like UA so mail/chat web apps treat the view as a supported browser.
pub const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.6 Safari/605.1.15";

pub struct WebTab {
    pub view: Option<wry::WebView>,
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub last_active: Instant,
    pub zoom: f64,
    pub visible: bool,
    pub bounds: Option<egui::Rect>,
    pub has_adblock: bool,
    /// Our own view of the page history, so back/forward know when to fall
    /// back to the tab's history.
    pub hist: Vec<String>,
    pub pos: usize,
    pub pending: Option<i32>,
    /// Chosen when the tab is first shown (see `Browser::sync_webviews`).
    pub engine: Option<crate::profile::Engine>,
    pub sv: Option<crate::servo_web::View>,
}

impl WebTab {
    pub fn new(url: &str, title: &str) -> Self {
        WebTab { view: None, url: url.to_owned(), title: title.to_owned(), loading: false, last_active: Instant::now(), zoom: 1.0, visible: false, bounds: None, has_adblock: false, hist: Vec::new(), pos: 0, pending: None, engine: None, sv: None }
    }

    pub fn track(&mut self, url: &str) {
        if let Some(d) = self.pending.take() {
            self.pos = (self.pos as i32 + d).clamp(0, self.hist.len().saturating_sub(1) as i32) as usize;
            if let Some(h) = self.hist.get_mut(self.pos) {
                *h = url.to_owned();
            }
            return;
        }
        if self.hist.get(self.pos).map(String::as_str) != Some(url) {
            if !self.hist.is_empty() {
                self.hist.truncate(self.pos + 1);
            }
            self.hist.push(url.to_owned());
            self.pos = self.hist.len() - 1;
        }
    }

    pub fn discard(&mut self) {
        self.view = None;
        self.sv = None;
        self.visible = false;
        self.bounds = None;
        self.has_adblock = false;
    }
}

fn wrect(r: egui::Rect) -> wry::Rect {
    wry::Rect {
        position: wry::dpi::LogicalPosition::new(r.left() as f64, r.top() as f64).into(),
        size: wry::dpi::LogicalSize::new(r.width().max(1.0) as f64, r.height().max(1.0) as f64).into(),
    }
}

pub fn downloads_dir() -> std::path::PathBuf {
    dirs::download_dir().unwrap_or_else(std::env::temp_dir)
}

/// Creates the web view for a tab as a child of the browser window.
pub fn create(frame: &eframe::Frame, tab: u64, url: &str, rect: egui::Rect, tx: &Sender<Event>, ctx: &egui::Context) -> Result<wry::WebView, String> {
    let (t1, t2, t3, t4, t5) = (tx.clone(), tx.clone(), tx.clone(), tx.clone(), tx.clone());
    let (c1, c2, c3, c4, c5) = (ctx.clone(), ctx.clone(), ctx.clone(), ctx.clone(), ctx.clone());
    wry::WebViewBuilder::new()
        .with_url(url)
        .with_bounds(wrect(rect))
        .with_user_agent(USER_AGENT)
        .with_devtools(true)
        .with_background_color((0, 0, 0, 255))
        .with_back_forward_navigation_gestures(true)
        .with_accept_first_mouse(true)
        .with_on_page_load_handler(move |ev, url| {
            let _ = t1.send(Event::WebLoad { tab, started: matches!(ev, wry::PageLoadEvent::Started), url });
            c1.request_repaint();
        })
        .with_document_title_changed_handler(move |title| {
            let _ = t2.send(Event::WebTitle { tab, title });
            c2.request_repaint();
        })
        .with_new_window_req_handler(move |url, _| {
            let _ = t3.send(Event::OpenTab { url, background: false });
            c3.request_repaint();
            wry::NewWindowResponse::Deny
        })
        .with_navigation_handler(move |url| {
            // inert:// links from the web are handed to the secured engine.
            if url.starts_with("inert://") {
                let _ = t4.send(Event::Navigate { tab, url });
                c4.request_repaint();
                return false;
            }
            true
        })
        .with_download_started_handler(move |url, path| {
            let name = url.rsplit('/').next().filter(|n| !n.is_empty() && !n.contains('?')).unwrap_or("download").to_owned();
            let dest = downloads_dir().join(&name);
            *path = dest.clone();
            let _ = t5.send(Event::Download { url, path: dest });
            c5.request_repaint();
            true
        })
        .build_as_child(frame)
        .map_err(|e| e.to_string())
}

pub fn place(t: &mut WebTab, rect: egui::Rect) {
    if let Some(v) = &t.view {
        if t.bounds != Some(rect) {
            let _ = v.set_bounds(wrect(rect));
            t.bounds = Some(rect);
        }
    }
}

pub fn set_visible(t: &mut WebTab, on: bool) {
    if let Some(v) = &t.view {
        if t.visible != on {
            let _ = v.set_visible(on);
            t.visible = on;
            if on {
                let _ = v.focus();
            }
        }
    }
}

pub fn js(t: &WebTab, script: &str) {
    if let Some(v) = &t.view {
        let _ = v.evaluate_script(script);
    }
    if let Some(s) = &t.sv {
        s.eval(script);
    }
}

/// Reads the page's visible text (for the AI panel). Treated as untrusted.
pub fn read_text(t: &WebTab, tab: u64, tx: &Sender<Event>, ctx: &egui::Context) {
    if let Some(s) = &t.sv {
        crate::servo_web::read_text(s, tab);
        return;
    }
    if let Some(v) = &t.view {
        let tx = tx.clone();
        let ctx = ctx.clone();
        let _ = v.evaluate_script_with_callback("(document.title + '\\n' + (document.body ? document.body.innerText : '')).slice(0, 24000)", move |s| {
            let text = serde_json::from_str::<String>(&s).unwrap_or(s);
            let _ = tx.send(Event::PageText { tab, text });
            ctx.request_repaint();
        });
    }
}

/// macOS window dressing that egui and wry don't cover.
#[cfg(target_os = "macos")]
pub mod mac {
    use objc2::encode::{Encode, Encoding};
    use objc2::msg_send;
    use objc2::runtime::AnyObject;

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Pt {
        x: f64,
        y: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Sz {
        w: f64,
        h: f64,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct Rc {
        o: Pt,
        s: Sz,
    }
    unsafe impl Encode for Pt {
        const ENCODING: Encoding = Encoding::Struct("CGPoint", &[f64::ENCODING, f64::ENCODING]);
    }
    unsafe impl Encode for Sz {
        const ENCODING: Encoding = Encoding::Struct("CGSize", &[f64::ENCODING, f64::ENCODING]);
    }
    unsafe impl Encode for Rc {
        const ENCODING: Encoding = Encoding::Struct("CGRect", &[Pt::ENCODING, Sz::ENCODING]);
    }

    /// Sends an Edit-menu action (cut:, copy:, paste:, selectAll:, undo:,
    /// redo:) to a web view.
    pub fn edit_action(view: &wry::WebView, what: &str) {
        use wry::WebViewExtMacOS;
        let wv = view.webview();
        let nil: *mut AnyObject = std::ptr::null_mut();
        unsafe {
            match what {
                "cut" => { let _: () = msg_send![&*wv, cut: nil]; }
                "copy" => { let _: () = msg_send![&*wv, copy: nil]; }
                "paste" => { let _: () = msg_send![&*wv, paste: nil]; }
                "select-all" => { let _: () = msg_send![&*wv, selectAll: nil]; }
                "undo" | "redo" => {
                    let um: *mut AnyObject = msg_send![&*wv, undoManager];
                    if !um.is_null() {
                        if what == "undo" { let _: () = msg_send![um, undo]; } else { let _: () = msg_send![um, redo]; }
                    }
                }
                _ => {}
            }
        }
    }

    /// Whether the keyboard is in this web view (it, or one of WebKit's
    /// inner views, is the window's first responder).
    pub fn has_focus(view: &wry::WebView) -> bool {
        use wry::WebViewExtMacOS;
        let wv = view.webview();
        unsafe {
            let window: *mut AnyObject = msg_send![&*wv, window];
            if window.is_null() {
                return false;
            }
            let fr: *mut AnyObject = msg_send![window, firstResponder];
            let me = &*wv as *const _ as *const AnyObject;
            if fr.is_null() {
                return false;
            }
            if fr as *const AnyObject == me {
                return true;
            }
            // An inner view of the web view.
            let is_view: bool = msg_send![fr, isKindOfClass: objc2::runtime::AnyClass::get(c"NSView").map(|c| c as *const _).unwrap_or(std::ptr::null())];
            is_view && msg_send![fr, isDescendantOf: me]
        }
    }

    /// The text on the system clipboard, if there is any.
    pub fn clipboard_text() -> Option<String> {
        use objc2::runtime::AnyClass;
        let cls = AnyClass::get(c"NSPasteboard")?;
        let ty = objc2_foundation::NSString::from_str("public.utf8-plain-text");
        unsafe {
            let pb: *mut AnyObject = msg_send![cls, generalPasteboard];
            if pb.is_null() {
                return None;
            }
            let s: *mut objc2_foundation::NSString = msg_send![pb, stringForType: &*ty];
            s.as_ref().map(|s| s.to_string())
        }
    }

    /// Rounds a web view's corners so it fits the page card.
    pub fn round(view: &wry::WebView, radius: f64) {
        use wry::WebViewExtMacOS;
        let wv = view.webview();
        unsafe {
            let _: () = msg_send![&*wv, setWantsLayer: true];
            let layer: *mut AnyObject = msg_send![&*wv, layer];
            if !layer.is_null() {
                let _: () = msg_send![layer, setCornerRadius: radius];
                let _: () = msg_send![layer, setMasksToBounds: true];
            }
        }
    }

    /// Moves the traffic lights so the close button starts at `x` and all
    /// three center on `y` (points from the window's top-left). AppKit puts
    /// them back whenever it lays the title bar out, so this runs every frame
    /// and only touches what has drifted. Returns the close button's center.
    pub fn traffic_lights(frame: &eframe::Frame, x: f64, y: f64) -> Option<(f64, f64)> {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let handle = frame.window_handle().ok()?;
        let RawWindowHandle::AppKit(h) = handle.as_raw() else { return None };
        let view = h.ns_view.as_ptr() as *mut AnyObject;
        unsafe {
            let window: *mut AnyObject = msg_send![view, window];
            if window.is_null() {
                return None;
            }
            let close: *mut AnyObject = msg_send![window, standardWindowButton: 0usize];
            let mini: *mut AnyObject = msg_send![window, standardWindowButton: 1usize];
            if close.is_null() || mini.is_null() {
                return None;
            }
            let bar: *mut AnyObject = msg_send![close, superview];
            if bar.is_null() {
                return None;
            }
            let container: *mut AnyObject = msg_send![bar, superview];
            if container.is_null() {
                return None;
            }
            // A taller title bar, so the buttons can sit lower than AppKit's default.
            let height = y * 2.0;
            let wf: Rc = msg_send![window, frame];
            let cf: Rc = msg_send![container, frame];
            if (cf.s.h - height).abs() > 0.5 || (cf.o.y - (wf.s.h - height)).abs() > 0.5 {
                let nf = Rc { o: Pt { x: cf.o.x, y: wf.s.h - height }, s: Sz { w: cf.s.w, h: height } };
                let _: () = msg_send![container, setFrame: nf];
            }
            let c0: Rc = msg_send![close, frame];
            let m0: Rc = msg_send![mini, frame];
            let step = (m0.o.x - c0.o.x).clamp(16.0, 28.0);
            for i in 0..3usize {
                let b: *mut AnyObject = msg_send![window, standardWindowButton: i];
                if b.is_null() {
                    continue;
                }
                let f: Rc = msg_send![b, frame];
                let o = Pt { x: x + i as f64 * step, y: (height - f.s.h) / 2.0 };
                if (f.o.x - o.x).abs() > 0.5 || (f.o.y - o.y).abs() > 0.5 {
                    let _: () = msg_send![b, setFrameOrigin: o];
                }
            }
            let c: Rc = msg_send![close, frame];
            Some((c.o.x + c.s.w / 2.0, height - (c.o.y + c.s.h / 2.0)))
        }
    }
}
