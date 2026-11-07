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
}

impl WebTab {
    pub fn new(url: &str, title: &str) -> Self {
        WebTab { view: None, url: url.to_owned(), title: title.to_owned(), loading: false, last_active: Instant::now(), zoom: 1.0, visible: false, bounds: None, has_adblock: false, hist: Vec::new(), pos: 0, pending: None }
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
}

/// Reads the page's visible text (for the AI panel). Treated as untrusted.
pub fn read_text(t: &WebTab, tab: u64, tx: &Sender<Event>, ctx: &egui::Context) {
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
