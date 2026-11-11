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

/// How a tab's web view is set up.
#[derive(Clone, Copy, Default)]
pub struct Opts {
    /// A private tab: its own data store that is never written to disk and
    /// is shared with no other tab.
    pub private: bool,
    pub hide_location: bool,
    pub calm: bool,
    /// The Shield is on for this page's site.
    pub shield: bool,
}

/// Creates the web view for a tab as a child of the browser window.
pub fn create(frame: &eframe::Frame, tab: u64, url: &str, rect: egui::Rect, tx: &Sender<Event>, ctx: &egui::Context, opts: Opts) -> Result<wry::WebView, String> {
    let mut script = String::from(crate::page_js::BASE);
    if opts.hide_location {
        script.push_str(crate::page_js::HIDE_LOCATION);
    }
    if opts.calm {
        script.push_str(crate::page_js::CALM);
    }
    if opts.shield {
        script.push_str(crate::page_js::YT_ADS);
    }
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
        .with_new_window_req_handler(move |url, features| {
            // Sign-in popups ("Sign in with Google", Apple, Microsoft…) talk
            // back to the page that opened them, so they open as real popup
            // windows with their opener. Everything else becomes a tab.
            let host = url::Url::parse(&url).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_default();
            let auth = ["accounts.google.com", "appleid.apple.com", "login.microsoftonline.com", "login.live.com", "www.facebook.com", "github.com", "api.twitter.com", "x.com", "discord.com", "auth0.com", "okta.com"].iter().any(|h| host == *h || host.ends_with(&format!(".{h}")));
            if auth || features.size.is_some() || url == "about:blank" || url.is_empty() {
                return wry::NewWindowResponse::Allow;
            }
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
            let _ = t5.send(Event::Download { tab, url, path: dest });
            c5.request_repaint();
            true
        })
        .with_incognito(opts.private)
        .with_initialization_script(script)
        // Camera and microphone: WebKit asks, every time. Nothing is granted silently.
        .with_permission_handler(|_| wry::PermissionResponse::Default)
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
    unsafe impl objc2::encode::RefEncode for Rc {
        const ENCODING_REF: Encoding = Encoding::Pointer(&<Rc as Encode>::ENCODING);
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

    #[link(name = "LocalAuthentication", kind = "framework")]
    unsafe extern "C" {}

    /// Asks for Touch ID (or the Mac's password where there is no sensor).
    /// `done` runs with the result, on some other thread.
    pub fn touch_id(reason: &str, done: impl Fn(bool) + Send + 'static) {
        let Some(cls) = objc2::runtime::AnyClass::get(c"LAContext") else {
            done(false);
            return;
        };
        let reason = objc2_foundation::NSString::from_str(reason);
        let reply = block2::RcBlock::new(move |ok: objc2::runtime::Bool, _err: *mut AnyObject| done(ok.as_bool()));
        unsafe {
            let ctx: *mut AnyObject = msg_send![cls, new];
            if ctx.is_null() {
                return;
            }
            // LAPolicyDeviceOwnerAuthentication = 2: Touch ID, Apple Watch or password.
            let _: () = msg_send![ctx, evaluatePolicy: 2isize, localizedReason: &*reason, reply: &*reply];
            let _: () = msg_send![ctx, release];
        }
    }

    /// Whether this Mac can confirm the user (Touch ID, Watch or password),
    /// and whether Touch ID itself is set up. Asks nothing of the user.
    pub fn touch_id_available() -> (bool, bool) {
        let Some(cls) = objc2::runtime::AnyClass::get(c"LAContext") else { return (false, false) };
        unsafe {
            let ctx: *mut AnyObject = msg_send![cls, new];
            if ctx.is_null() {
                return (false, false);
            }
            let nil: *mut *mut AnyObject = std::ptr::null_mut();
            // 2: device owner (Touch ID or password); 1: biometrics only.
            let owner: objc2::runtime::Bool = msg_send![ctx, canEvaluatePolicy: 2isize, error: nil];
            let bio: objc2::runtime::Bool = msg_send![ctx, canEvaluatePolicy: 1isize, error: nil];
            let _: () = msg_send![ctx, release];
            (owner.as_bool(), bio.as_bool())
        }
    }

    /// The clipboard's change counter (it moves whenever something is copied).
    pub fn pasteboard_count() -> isize {
        let Some(cls) = objc2::runtime::AnyClass::get(c"NSPasteboard") else { return 0 };
        unsafe {
            let pb: *mut AnyObject = msg_send![cls, generalPasteboard];
            if pb.is_null() { 0 } else { msg_send![pb, changeCount] }
        }
    }

    /// Empties the clipboard.
    pub fn pasteboard_clear() {
        let Some(cls) = objc2::runtime::AnyClass::get(c"NSPasteboard") else { return };
        unsafe {
            let pb: *mut AnyObject = msg_send![cls, generalPasteboard];
            if !pb.is_null() {
                let _: isize = msg_send![pb, clearContents];
            }
        }
    }

    /// Hides the window from screenshots and screen sharing, or shows it again.
    pub fn set_capture_hidden(frame: &eframe::Frame, hidden: bool) {
        use raw_window_handle::{HasWindowHandle, RawWindowHandle};
        let Ok(handle) = frame.window_handle() else { return };
        let RawWindowHandle::AppKit(h) = handle.as_raw() else { return };
        let view = h.ns_view.as_ptr() as *mut AnyObject;
        unsafe {
            let window: *mut AnyObject = msg_send![view, window];
            if !window.is_null() {
                // NSWindowSharingNone = 0, NSWindowSharingReadOnly = 1
                let _: () = msg_send![window, setSharingType: if hidden { 0usize } else { 1usize }];
            }
        }
    }

    /// Whether an Option key is held right now (asked of AppKit, so it
    /// works whichever view has the keyboard).
    pub fn option_down() -> bool {
        let Some(cls) = objc2::runtime::AnyClass::get(c"NSEvent") else { return false };
        // NSEventModifierFlagOption = 1 << 19
        let flags: usize = unsafe { msg_send![cls, modifierFlags] };
        flags & (1 << 19) != 0
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

    #[repr(C)]
    pub struct CGImage {
        _p: [u8; 0],
    }
    unsafe impl objc2::encode::RefEncode for CGImage {
        const ENCODING_REF: Encoding = Encoding::Pointer(&Encoding::Struct("CGImage", &[]));
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        fn CGImageGetWidth(img: *const CGImage) -> usize;
        fn CGImageGetHeight(img: *const CGImage) -> usize;
        fn CGColorSpaceCreateDeviceRGB() -> *mut std::ffi::c_void;
        fn CGColorSpaceRelease(cs: *mut std::ffi::c_void);
        fn CGBitmapContextCreate(data: *mut std::ffi::c_void, w: usize, h: usize, bpc: usize, bpr: usize, cs: *mut std::ffi::c_void, info: u32) -> *mut std::ffi::c_void;
        fn CGContextDrawImage(ctx: *mut std::ffi::c_void, rect: Rc, img: *const CGImage);
        fn CGContextRelease(ctx: *mut std::ffi::c_void);
    }

    /// Takes a picture of what the web view shows, for drawing in its place
    /// while a menu or panel covers the page (a native view can't sit under
    /// Inert's own drawing). `done` gets the pixels on the main thread.
    pub fn snapshot(view: &wry::WebView, done: impl Fn(eframe::egui::ColorImage) + 'static) {
        use wry::WebViewExtMacOS;
        let wv = view.webview();
        let block = block2::RcBlock::new(move |img: *mut AnyObject, _err: *mut AnyObject| {
            if img.is_null() {
                return;
            }
            unsafe {
                let nil: *mut AnyObject = std::ptr::null_mut();
                let cg: *const CGImage = msg_send![img, CGImageForProposedRect: std::ptr::null_mut::<Rc>(), context: nil, hints: nil];
                if cg.is_null() {
                    return;
                }
                let (w, h) = (CGImageGetWidth(cg), CGImageGetHeight(cg));
                if w == 0 || h == 0 || w > 8192 || h > 8192 {
                    return;
                }
                let mut buf = vec![0u8; w * h * 4];
                let cs = CGColorSpaceCreateDeviceRGB();
                // RGBA, premultiplied, byte order 32 big.
                let ctx = CGBitmapContextCreate(buf.as_mut_ptr() as *mut _, w, h, 8, w * 4, cs, 1 | (4 << 12));
                if !ctx.is_null() {
                    CGContextDrawImage(ctx, Rc { o: Pt { x: 0.0, y: 0.0 }, s: Sz { w: w as f64, h: h as f64 } }, cg);
                    CGContextRelease(ctx);
                }
                CGColorSpaceRelease(cs);
                done(eframe::egui::ColorImage::from_rgba_premultiplied([w, h], &buf));
            }
        });
        unsafe {
            let nil: *mut AnyObject = std::ptr::null_mut();
            let _: () = msg_send![&*wv, takeSnapshotWithConfiguration: nil, completionHandler: &*block];
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
