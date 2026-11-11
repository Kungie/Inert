//! Browser state and the per-frame loop.

use crate::ai::{self, Msg, PageContext};
use crate::inert_tab::{self, Failure, Img, InertCmd, InertEvent, InertTab, Status};
use crate::omnibox::{self, Suggestion};
use crate::profile::{GroupKind, Profile, Provider, SavedTab};
use crate::render::{self, Act};
use crate::theme;
use crate::web::{self, WebTab};
use crate::{adblock, vpn};
use eframe::egui;
use inert_proto::{ErrorCode, Fields};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

pub enum Event {
    Inert(u64, InertEvent),
    WebLoad { tab: u64, started: bool, url: String },
    WebTitle { tab: u64, title: String },
    OpenTab { url: String, background: bool },
    Navigate { tab: u64, url: String },
    Download { tab: u64, url: String, path: std::path::PathBuf },
    PageText { tab: u64, text: String },
    Remote(String, Vec<String>),
    AiReply(Result<String, String>),
    Vpn(vpn::Status),
    VpnRelays(Vec<vpn::Country>),
    VpnLogin(Result<(), String>),
    AdblockRules(String, usize),
    AdblockFailed(String),
    Menu(String),
    ServoUrl { tab: u64, url: String },
    ServoCookies { req: u64, list: Vec<inert_servo_ipc::CookieJson> },
    /// The cookie-banner script's answer for a tab: what it clicked, if anything.
    Consent { tab: u64, what: String },
    WkCookies(Vec<inert_servo_ipc::CookieJson>),
    Imported(Box<crate::import::Imported>),
    /// A site icon arrived (raw image bytes).
    Favicon { host: String, bytes: Vec<u8> },
    /// The clip tool's capture of the window, and the region to keep.
    Clipped { rect: egui::Rect, image: std::sync::Arc<egui::ColorImage> },
    /// The text selected on a page (asked for by "Attach to assistant").
    Selection { tab: u64, text: String },
    /// One of Inert's page scripts answered.
    PageAnswer { tab: u64, ask: Ask, text: String },
    /// The model translated the page's text (a JSON array), or failed.
    Translated { tab: u64, result: Result<String, String> },
    /// An instant answer for what was typed: (query, answer).
    Instant(String, String),
    /// Touch ID answered.
    Unlocked { what: Unlock, ok: bool },
    /// Push-to-talk: what the dictation helper heard.
    Dictate(crate::dictate::Heard),
    /// A Servo page asked for its context menu.
    ServoMenu { tab: u64, actions: Vec<String>, link: Option<String>, image: Option<String>, selection: bool, editable: bool },
}

/// A page as Rewind remembers it.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Snap {
    pub url: String,
    pub title: String,
    pub text: String,
    pub t: u64,
}

fn rewind_path(demo: bool) -> std::path::PathBuf {
    Profile::dir(demo).join("rewind.json")
}

fn load_rewind(demo: bool) -> Vec<Snap> {
    std::fs::read_to_string(rewind_path(demo)).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
}

/// What a page script was run for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ask {
    Translate,
    Readable,
    Pip,
    Captions,
}

/// "Ask all tabs": page texts on their way in.
pub struct Gather {
    pub q: String,
    pub waiting: std::collections::HashSet<u64>,
    pub pages: Vec<(String, String, String)>,
    pub started: Instant,
}

/// What Touch ID was asked to open.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Unlock {
    SecretFavorites,
    PrivateTabs,
}

/// The context menu of a web page, drawn by the browser.
pub struct PageMenu {
    pub tab: u64,
    pub at: egui::Pos2,
    pub opened: Instant,
    pub actions: Vec<String>,
    pub link: Option<String>,
    pub image: Option<String>,
    pub selection: bool,
    pub editable: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    NewTab,
    History,
    Bookmarks,
    Settings,
    Downloads,
    Import,
    Chat,
}

impl Page {
    pub fn url(self) -> &'static str {
        match self {
            Page::NewTab => "about:newtab",
            Page::History => "about:history",
            Page::Bookmarks => "about:bookmarks",
            Page::Settings => "about:settings",
            Page::Downloads => "about:downloads",
            Page::Import => "about:import",
            Page::Chat => "about:chat",
        }
    }
    pub fn title(self) -> &'static str {
        match self {
            Page::NewTab => "New Tab",
            Page::History => "History",
            Page::Bookmarks => "Bookmarks",
            Page::Settings => "Settings",
            Page::Downloads => "Downloads",
            Page::Import => "Import from Chrome",
            Page::Chat => "Assistant",
        }
    }
    pub fn from_url(u: &str) -> Option<Page> {
        Some(match u.trim_end_matches('/') {
            "about:newtab" | "about:blank" | "" => Page::NewTab,
            "about:history" => Page::History,
            "about:bookmarks" => Page::Bookmarks,
            "about:settings" | "about:preferences" => Page::Settings,
            "about:downloads" => Page::Downloads,
            "about:import" => Page::Import,
            "about:chat" => Page::Chat,
            _ => return None,
        })
    }
}

pub enum Kind {
    Page(Page),
    Inert(Box<InertTab>),
    Web(WebTab),
}

/// A file downloaded this session.
pub struct Download {
    pub url: String,
    pub path: std::path::PathBuf,
    /// The private tab it came from; such files vanish with the tab unless kept.
    pub private_tab: Option<u64>,
    pub keep: bool,
}

pub struct Tab {
    pub id: u64,
    /// A private tab: sealed data store, no history, erased when closed.
    pub private: bool,
    pub kind: Kind,
    pub group: Option<u64>,
    pub pinned: bool,
    /// Navigation history for internal pages and kind switches.
    pub back: Vec<String>,
    pub fwd: Vec<String>,
}

impl Tab {
    pub fn url(&self) -> String {
        match &self.kind {
            Kind::Page(p) => p.url().into(),
            Kind::Inert(t) => t.url(),
            Kind::Web(w) => w.url.clone(),
        }
    }
    pub fn title(&self) -> String {
        match &self.kind {
            Kind::Page(p) => p.title().into(),
            Kind::Inert(t) => t.title(),
            Kind::Web(w) => {
                if w.title.is_empty() { omnibox::strip(&w.url).to_owned() } else { w.title.clone() }
            }
        }
    }
    pub fn loading(&self) -> bool {
        match &self.kind {
            Kind::Inert(t) => t.status == Status::Connecting || t.busy,
            Kind::Web(w) => w.loading,
            Kind::Page(_) => false,
        }
    }
}

#[derive(Default)]
pub struct Omni {
    pub text: String,
    pub editing: bool,
    pub focus: bool,
    pub select_all: bool,
    pub sugg: Vec<Suggestion>,
    pub remote: Vec<String>,
    pub sel: usize,
    pub typed: String,
    pub last_remote_q: String,
    pub last_edit: Option<Instant>,
}

#[derive(Default)]
pub struct AiState {
    pub open: bool,
    pub msgs: Vec<Msg>,
    pub input: String,
    pub waiting: bool,
    pub pending_question: Option<String>,
    /// What is attached to the next question.
    pub attach: Vec<Attached>,
    /// The next question goes to every open page, not just this one.
    pub all_tabs: bool,
    pub gather: Option<Gather>,
    pub clip: Clip,
    /// When the latest reply arrived (it fades in).
    pub reply_at: f64,
    /// Put the keyboard in the composer on the next frame.
    pub focus: bool,
}

/// An attachment waiting in the composer, with what the chip shows.
pub struct Attached {
    pub a: crate::ai::Attach,
    pub label: String,
    pub thumb: Option<egui::TextureHandle>,
}

/// The clip tool: drag a region of the page to show it to the assistant.
#[derive(Default, Clone, Copy, PartialEq)]
pub enum Clip {
    #[default]
    Off,
    /// Waiting for the drag (and during it: where it started).
    Picking(Option<egui::Pos2>),
    /// Region chosen: the next frame is drawn clean and captured.
    Capture(egui::Rect),
}

#[derive(Default)]
pub struct VpnState {
    pub status: vpn::Status,
    pub relays: Vec<vpn::Country>,
    pub open: bool,
    pub filter: String,
    pub reveal_ip: bool,
    /// The account number being typed (cleared as soon as it is sent).
    pub account_input: String,
    pub show_account: bool,
    pub login_busy: bool,
    pub login_error: Option<String>,
    pub confirm_logout: bool,
}

pub struct AdState {
    pub state: adblock::State,
    #[cfg(target_os = "macos")]
    pub slot: adblock::mac::Slot,
    #[cfg(target_os = "macos")]
    pub list: Option<objc2::rc::Retained<objc2_web_kit::WKContentRuleList>>,
}

#[derive(Default)]
pub struct Signin {
    /// Step 2 of onboarding: bring Chrome along.
    pub import_step: bool,
    /// The model step (API key, or skipped) is behind us.
    pub key_done: bool,
    /// The look step (look, color, layout) was confirmed.
    pub look_done: bool,
    pub provider: Option<Provider>,
    pub key: String,
    pub error: Option<String>,
}

#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
pub enum Popover {
    #[default]
    None,
    Security,
    Vpn,
    Shield,
    Menu,
    Cookies,
    Passwords,
    GroupAdd(u64),
}

pub struct Browser {
    pub demo: bool,
    pub profile: Profile,
    pub dirty: bool,
    pub last_save: Instant,
    pub tabs: Vec<Tab>,
    pub active: u64,
    pub closed: Vec<SavedTab>,
    pub rt: tokio::runtime::Runtime,
    pub tx: Sender<Event>,
    pub rx: Receiver<Event>,
    pub omni: Omni,
    /// The new tab page's own search box.
    pub ntp: Omni,
    pub ai: AiState,
    pub vpn: VpnState,
    pub ad: AdState,
    pub signin: Signin,
    pub popover: Popover,
    pub popover_opened: Option<Instant>,
    pub downloads: Vec<Download>,
    /// The clipboard's change counter when the first private tab opened.
    pub private_clip: Option<isize>,
    /// Private tabs are covered until Touch ID unlocks them.
    pub private_locked: bool,
    /// Secret favorites, while unlocked (and until when).
    pub secrets: Option<(Vec<crate::profile::Bookmark>, Instant)>,
    /// Split view: the tab shown in the right pane, and where.
    pub split: Option<u64>,
    pub split_rect: egui::Rect,
    /// The tab that was active before this one.
    pub prev_active: u64,
    /// Rewind: the text of pages seen (newest last), kept on this Mac.
    pub rewind: Vec<Snap>,
    /// "Read aloud" while it is speaking.
    pub say: Option<std::process::Child>,
    rewind_dirty: bool,
    /// When the user last did anything (private tabs erase themselves when idle).
    pub last_input: Instant,
    capture_hidden: bool,
    pub toast: Option<(String, Instant)>,
    pub find: Option<String>,
    pub inspector: bool,
    pub rename: Option<(u64, String)>,
    pub ctx: egui::Context,
    pub menu: Option<crate::menu::AppMenu>,
    pub shot: Option<crate::shot::Shot>,
    pub driver: Option<crate::drive::Driver>,
    pub overlay_rects: Vec<egui::Rect>,
    pub pending_input: Vec<egui::Event>,
    pub favicons: crate::favicon::Favicons,
    pub page_menu: Option<PageMenu>,
    pub dictation: Option<crate::dictate::Dictation>,
    pub content_rect: egui::Rect,
    pub sites: Vec<(String, String)>,
    /// Servo, created on first use.
    pub servo: Option<crate::servo_web::Host>,
    pub servo_error: Option<String>,
    pub servo_restarts: u32,
    pub last_relief: Instant,
    /// Tabs closing in the strip (animation only).
    pub closing: Vec<crate::chrome::Ghost>,
    pub cookie: crate::cookies::State,
    /// Imported cookies waiting for the Servo process to start.
    pub pending_servo_cookies: Vec<(String, inert_servo_ipc::CookieJson)>,
    pub import: ImportState,
}

#[derive(Default)]
pub struct ImportState {
    pub sources: Vec<crate::import::Source>,
    pub pick: usize,
    pub what: crate::import::What,
    pub running: bool,
    /// What the last import brought over, one line per kind.
    pub summary: Vec<String>,
    pub errors: Vec<String>,
    /// Browsers whose data folder macOS wouldn't let Inert read.
    pub blocked: Vec<&'static str>,
}

impl Browser {
    pub fn new(cc: &eframe::CreationContext<'_>, demo: bool, shot_dir: Option<std::path::PathBuf>, drive_dir: Option<std::path::PathBuf>) -> Self {
        let ctx = cc.egui_ctx.clone();
        theme::install_fonts(&ctx);
        theme::install_style(&ctx);
        // A dark window: WebKit views inherit it, so pages see prefers-color-scheme: dark.
        ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(egui::SystemTheme::Dark));
        if demo {
            // Demo mode always starts from a clean, fake profile (safe for screenshots).
            let _ = std::fs::remove_dir_all(Profile::dir(true));
        }
        let profile = Profile::load(demo);
        theme::set_dark(profile.settings.dark == Some(true));
        theme::set_accent(profile.settings.accent);
        let (tx, rx) = channel();
        let rt = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build().expect("tokio runtime");
        let menu = crate::menu::AppMenu::install(tx.clone(), ctx.clone());
        let mut b = Browser {
            demo,
            profile,
            dirty: false,
            last_save: Instant::now(),
            tabs: Vec::new(),
            active: 0,
            closed: Vec::new(),
            rt,
            tx,
            rx,
            omni: Omni::default(),
            ntp: Omni::default(),
            ai: AiState::default(),
            vpn: VpnState::default(),
            ad: AdState {
                state: adblock::State::Off,
                #[cfg(target_os = "macos")]
                slot: Default::default(),
                #[cfg(target_os = "macos")]
                list: None,
            },
            signin: Signin::default(),
            popover: Popover::None,
            popover_opened: None,
            downloads: Vec::new(),
            toast: None,
            find: None,
            inspector: false,
            rename: None,
            ctx: ctx.clone(),
            menu,
            shot: shot_dir.map(crate::shot::Shot::new),
            driver: drive_dir.map(crate::drive::Driver::new),
            overlay_rects: Vec::new(),
            pending_input: Vec::new(),
            favicons: crate::favicon::Favicons::new(Profile::dir(demo), demo),
            page_menu: None,
            dictation: None,
            content_rect: egui::Rect::NOTHING,
            sites: Vec::new(),
            servo: None,
            servo_error: None,
            servo_restarts: 0,
            last_relief: Instant::now(),
            private_clip: None,
            private_locked: false,
            secrets: None,
            split: None,
            split_rect: egui::Rect::NOTHING,
            prev_active: 0,
            rewind: load_rewind(demo),
            say: None,
            rewind_dirty: false,
            last_input: Instant::now(),
            capture_hidden: false,
            closing: Vec::new(),
            cookie: Default::default(),
            pending_servo_cookies: Vec::new(),
            import: ImportState::default(),
        };
        b.refresh_sites();
        b.import.sources = crate::import::sources();
        b.import.blocked = crate::import::blocked();
        if demo {
            // The demo profile (and the UI tests that run on it) keeps Servo,
            // so the Servo path stays covered: input, cookies, context menu.
            b.profile.settings.web_engine = crate::profile::Engine::Servo;
            // The demo profile imports from a fake Chrome profile, never the real one.
            match crate::import::write_fixture(&Profile::dir(true).join("chrome-demo"), "demo-safe-storage") {
                Ok(mut s) => {
                    s.profile = "Demo profile (demo data)".into();
                    b.import.sources = vec![s];
                }
                Err(e) => tracing::warn!("demo import fixture: {e}"),
            }
            // Tests: import a profile written by a real Chrome started with
            // --use-mock-keychain (its key is known; no Keychain involved).
            if let (Ok(dir), Ok(key)) = (std::env::var("INERT_IMPORT_DIR"), std::env::var("INERT_IMPORT_KEY")) {
                b.import.sources = vec![crate::import::Source { browser: "Chrome".into(), dir: dir.into(), profile: "real Chrome test profile".into(), keychain: (String::new(), String::new()), demo_key: Some(key) }];
            }
        }
        // Restore without activating, so collapsed groups stay collapsed.
        let session = b.profile.session.clone();
        for s in &session {
            let id = b.profile.new_id();
            let kind = b.make_kind(id, &s.url, Some(&s.title));
            b.tabs.push(Tab { id, private: false, kind, group: s.group, pinned: s.pinned, back: Vec::new(), fwd: Vec::new() });
        }
        if b.tabs.is_empty() {
            b.add_tab("about:newtab", None, true, None);
        }
        let order = b.visible_order();
        let idx = b.profile.active.min(order.len() - 1);
        b.active = order[idx];
        b.omni.text = display_url(&b.tab(b.active).map(|t| t.url()).unwrap_or_default());
        vpn::start_polling(b.tx.clone(), ctx.clone(), demo);
        if b.profile.settings.adblock {
            b.ad.state = adblock::State::Preparing;
            adblock::prepare(b.tx.clone(), ctx.clone());
        }
        b
    }

    pub fn refresh_sites(&mut self) {
        self.sites = inert_tab::known_sites()
            .into_iter()
            .map(|t| {
                let name = match t.host.as_str() {
                    "bank.example" => "Inert Bank".to_owned(),
                    "shop.example" => "Inert Supply".to_owned(),
                    "notes.example" => "Inert Notes".to_owned(),
                    h => h.to_owned(),
                };
                (t.host, name)
            })
            .collect();
    }

    // ------------------------------------------------------------------ tabs

    pub fn tab(&self, id: u64) -> Option<&Tab> {
        self.tabs.iter().find(|t| t.id == id)
    }
    pub fn tab_mut(&mut self, id: u64) -> Option<&mut Tab> {
        self.tabs.iter_mut().find(|t| t.id == id)
    }
    pub fn active_tab(&self) -> Option<&Tab> {
        self.tab(self.active)
    }

    fn make_kind(&mut self, id: u64, url: &str, title: Option<&str>) -> Kind {
        if let Some(p) = Page::from_url(url) {
            return Kind::Page(p);
        }
        if let Some((host, screen, raw)) = inert_tab::parse_url(url) {
            // Raw query params are typed once the manifest arrives.
            let mut tab = Box::new(InertTab::new(&host, screen, Fields::new()));
            self.connect_inert(id, &mut tab, raw);
            return Kind::Inert(tab);
        }
        Kind::Web(WebTab::new(&crate::profile::dark_web(url), title.unwrap_or("")))
    }

    fn connect_inert(&mut self, id: u64, t: &mut InertTab, raw: Vec<(String, String)>) {
        self.refresh_sites();
        match inert_tab::resolve(&t.host, &self.profile.pins) {
            Ok((site, new_pin)) => {
                let seen = self.profile.pins.get(&t.host).map(|p| p.max_version).filter(|v| *v > 0);
                if let Some(p) = new_pin {
                    self.profile.pins.insert(t.host.clone(), p);
                    self.dirty = true;
                }
                t.dev = site.dev;
                t.cmd = Some(inert_tab::spawn(self.rt.handle(), id, site, seen, self.tx.clone(), self.ctx.clone()));
                // Stash raw query params; they are typed on connect.
                RAW_PARAMS.with(|r| r.borrow_mut().insert(id, raw));
            }
            Err(f) => {
                t.status = Status::Failed;
                t.failure = Some(f);
            }
        }
    }

    pub fn add_tab(&mut self, url: &str, group: Option<u64>, activate: bool, title: Option<&str>) -> u64 {
        let id = self.profile.new_id();
        let kind = self.make_kind(id, url, title);
        // Sites on the always-private list never open in a normal tab.
        let private = self.wants_private(url);
        if private {
            self.note_private_opened();
        }
        let tab = Tab { id, private, kind, group, pinned: false, back: Vec::new(), fwd: Vec::new() };
        // Insert next to the active tab (Chrome-like), or at the end.
        let pos = self.tabs.iter().position(|t| t.id == self.active).map(|i| i + 1).unwrap_or(self.tabs.len());
        if activate || self.tabs.is_empty() {
            self.tabs.insert(pos.min(self.tabs.len()), tab);
            self.activate(id);
        } else {
            self.tabs.push(tab);
        }
        self.dirty = true;
        id
    }

    pub fn activate(&mut self, id: u64) {
        if self.active != id {
            self.prev_active = self.active;
        }
        // Bringing the split pane's tab forward ends the split.
        if self.split == Some(id) {
            self.split = None;
        }
        self.active = id;
        if let Some(t) = self.tab_mut(id) {
            if let Kind::Web(w) = &mut t.kind {
                w.last_active = Instant::now();
            }
        }
        // Expanding a collapsed group when one of its tabs gets focus.
        if let Some(g) = self.tab(id).and_then(|t| t.group) {
            if let Some(gd) = self.profile.groups.iter_mut().find(|x| x.id == g) {
                gd.collapsed = false;
            }
        }
        self.omni.editing = false;
        self.omni.text = display_url(&self.tab(id).map(|t| t.url()).unwrap_or_default());
        self.dirty = true;
    }

    pub fn close_tab(&mut self, id: u64) {
        let Some(i) = self.tabs.iter().position(|t| t.id == id) else { return };
        // Leave a shrinking ghost where the tab stood.
        let order = self.visible_order();
        let w: Option<f32> = self.ctx.data(|d| d.get_temp(egui::Id::unique(("tab-h", id))));
        if let (Some(w), Some(pos)) = (w, order.iter().position(|x| *x == id)) {
            let after = if pos == 0 { None } else { Some(order[pos - 1]) };
            let title = self.tabs[i].title();
            self.closing.push(crate::chrome::Ghost { after, h: w, start: self.ctx.input(|x| x.time), title });
        }
        let t = self.tabs.remove(i);
        if t.private {
            self.wipe_private(t.id);
        } else {
            self.closed.push(SavedTab { url: t.url(), title: t.title(), group: t.group, pinned: t.pinned });
        }
        if self.tabs.is_empty() {
            self.add_tab("about:newtab", None, true, None);
        } else if self.active == id {
            let n = i.min(self.tabs.len() - 1);
            self.activate(self.tabs[n].id);
        }
        self.dirty = true;
    }

    pub fn reopen_closed(&mut self) {
        if let Some(s) = self.closed.pop() {
            let id = self.add_tab(&s.url, s.group, true, Some(&s.title));
            if let Some(t) = self.tab_mut(id) {
                t.pinned = s.pinned;
            }
        }
    }

    pub fn cycle(&mut self, d: i32) {
        let order = self.visible_order();
        if order.is_empty() {
            return;
        }
        let i = order.iter().position(|x| *x == self.active).unwrap_or(0) as i32;
        let n = order.len() as i32;
        let next = order[((i + d).rem_euclid(n)) as usize];
        self.activate(next);
    }

    /// Tab ids in strip order (pinned first, then groups, then the rest).
    pub fn visible_order(&self) -> Vec<u64> {
        let mut v: Vec<u64> = self.tabs.iter().filter(|t| t.pinned).map(|t| t.id).collect();
        for g in &self.profile.groups {
            v.extend(self.tabs.iter().filter(|t| !t.pinned && t.group == Some(g.id)).map(|t| t.id));
        }
        v.extend(self.tabs.iter().filter(|t| !t.pinned && t.group.is_none_or(|g| !self.profile.groups.iter().any(|x| x.id == g))).map(|t| t.id));
        v
    }

    /// Navigates the active tab (or `id`) to `url`.
    pub fn navigate(&mut self, id: u64, url: &str) {
        let url = crate::profile::dark_web(url.trim());
        if self.wants_private(&url) && !self.tab(id).is_some_and(|t| t.private) {
            self.add_tab(&url, None, true, None);
            return;
        }
        // Same kind web→web: reuse the web view.
        let old = self.tab(id).map(|t| t.url()).unwrap_or_default();
        if let Some(t) = self.tab_mut(id) {
            if let Kind::Web(w) = &mut t.kind {
                if !url.starts_with("inert://") && Page::from_url(&url).is_none() {
                    w.url = url.clone();
                    w.loading = true;
                    if let Some(v) = &w.view {
                        let _ = v.load_url(&url);
                    }
                    if let Some(s) = &w.sv {
                        s.load(&url);
                    }
                    return;
                }
            }
        }
        let kind = self.make_kind(id, &url, None);
        if let Some(t) = self.tab_mut(id) {
            if !old.is_empty() {
                t.back.push(old);
                t.fwd.clear();
            }
            t.kind = kind;
        }
        if id == self.active {
            self.omni.text = display_url(&url);
            self.omni.editing = false;
        }
        self.dirty = true;
    }

    pub fn open_url_from_omnibox(&mut self, input: &str) {
        let url = match omnibox::classify(input) {
            omnibox::Nav::Url(u) => u,
            omnibox::Nav::Search(q) => self.profile.settings.search.search_url(&q),
        };
        let id = self.active;
        self.navigate(id, &url);
    }

    fn tab_history(&mut self, back: bool) {
        let id = self.active;
        let Some(t) = self.tab_mut(id) else { return };
        let target = if back { t.back.pop() } else { t.fwd.pop() };
        let Some(target) = target else { return };
        let cur = t.url();
        if back { t.fwd.push(cur) } else { t.back.push(cur) }
        let k = self.make_kind(id, &target, None);
        if let Some(t) = self.tab_mut(id) {
            t.kind = k;
        }
        self.omni.text = display_url(&target);
    }

    pub fn go_back(&mut self) {
        let id = self.active;
        let Some(t) = self.tab_mut(id) else { return };
        match &mut t.kind {
            Kind::Inert(it) if !it.back.is_empty() => {
                it.go_back();
            }
            Kind::Web(w) if w.sv.as_ref().is_some_and(|s| s.can_back()) => {
                if let Some(s) = &w.sv {
                    s.back();
                }
            }
            Kind::Web(w) if w.sv.is_none() && w.pos > 0 => {
                w.pending = Some(-1);
                web::js(w, "history.back()");
            }
            _ => self.tab_history(true),
        }
    }

    pub fn go_forward(&mut self) {
        let id = self.active;
        let Some(t) = self.tab_mut(id) else { return };
        match &mut t.kind {
            Kind::Inert(it) if !it.fwd.is_empty() => {
                it.go_forward();
            }
            Kind::Web(w) if w.sv.as_ref().is_some_and(|s| s.can_fwd()) => {
                if let Some(s) = &w.sv {
                    s.forward();
                }
            }
            Kind::Web(w) if w.sv.is_none() && w.pos + 1 < w.hist.len() => {
                w.pending = Some(1);
                web::js(w, "history.forward()");
            }
            _ => self.tab_history(false),
        }
    }

    pub fn can_go(&self, back: bool) -> bool {
        let Some(t) = self.active_tab() else { return false };
        let inner = match &t.kind {
            Kind::Inert(it) => if back { !it.back.is_empty() } else { !it.fwd.is_empty() },
            Kind::Web(w) => match &w.sv {
                Some(s) => if back { s.can_back() } else { s.can_fwd() },
                None => if back { w.pos > 0 } else { w.pos + 1 < w.hist.len() },
            },
            Kind::Page(_) => false,
        };
        inner || if back { !t.back.is_empty() } else { !t.fwd.is_empty() }
    }

    pub fn reload(&mut self) {
        let id = self.active;
        let url = self.tab(id).map(|t| t.url()).unwrap_or_default();
        match self.tab_mut(id).map(|t| &mut t.kind) {
            Some(Kind::Web(w)) => {
                if let Some(v) = &w.view {
                    let _ = v.reload();
                    w.loading = true;
                }
                if let Some(s) = &w.sv {
                    s.reload();
                    w.loading = true;
                }
            }
            Some(Kind::Inert(_)) => {
                let k = self.make_kind(id, &url, None);
                if let Some(t) = self.tab_mut(id) {
                    t.kind = k;
                }
            }
            _ => {}
        }
    }

    pub fn stop(&mut self) {
        if let Some(Kind::Web(w)) = self.tab_mut(self.active).map(|t| &mut t.kind) {
            web::js(w, "window.stop()");
            w.loading = false;
        }
    }

    pub fn zoom(&mut self, delta: f32) {
        if let Some(t) = self.tab_mut(self.active) {
            match &mut t.kind {
                Kind::Inert(it) => it.zoom = if delta == 0.0 { 1.0 } else { (it.zoom + delta).clamp(0.6, 2.0) },
                Kind::Web(w) => {
                    w.zoom = if delta == 0.0 { 1.0 } else { (w.zoom + delta as f64).clamp(0.5, 3.0) };
                    if let Some(v) = &w.view {
                        let _ = v.zoom(w.zoom);
                    }
                    if let Some(s) = &w.sv {
                        s.zoom(w.zoom as f32);
                    }
                }
                _ => {}
            }
        }
    }

    pub fn toast(&mut self, s: impl Into<String>) {
        self.toast = Some((s.into(), Instant::now()));
    }

    pub fn group_new(&mut self, tab: u64, name: &str) -> u64 {
        let colors = ["blue", "purple", "teal", "pink", "orange", "green", "red", "grey"];
        let color = colors[self.profile.groups.len() % colors.len()].to_owned();
        let id = self.profile.new_id();
        self.profile.groups.push(crate::profile::GroupDef { id, name: name.into(), color, kind: GroupKind::Normal, collapsed: false });
        if let Some(t) = self.tab_mut(tab) {
            t.group = Some(id);
        }
        self.rename = Some((id, name.into()));
        self.dirty = true;
        id
    }

    // ------------------------------------------------------------------ events

    fn handle_events(&mut self, frame: &eframe::Frame) {
        let _ = frame;
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Event::Inert(id, e) => self.on_inert(id, e),
                Event::WebLoad { tab, started, url } => {
                    let mut title = String::new();
                    if let Some(t) = self.tab_mut(tab) {
                        if let Kind::Web(w) = &mut t.kind {
                            w.loading = started;
                            w.url = url.clone();
                            title = w.title.clone();
                            if !started {
                                w.track(&url);
                            }
                        }
                    }
                    let private = self.tab(tab).is_some_and(|t| t.private);
                    if !started {
                        self.after_web_load(tab, &url);
                        // A private tab leaves no history and asks no one for an icon.
                        if !private {
                            self.profile.record_visit(&url, &title);
                            self.dirty = true;
                            let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
                            self.favicons.want(&url, &tx, &ctx);
                            // Rewind: ask the page for its text once it has settled.
                            if self.profile.settings.rewind && !self.demo {
                                if let Some(Kind::Web(w)) = self.tab(tab).map(|t| &t.kind) {
                                    web::read_text(w, tab, &tx, &ctx);
                                }
                            }
                        }
                    }
                    if tab == self.active && !self.omni.editing {
                        self.omni.text = url;
                    }
                }
                Event::WebTitle { tab, title } => {
                    let mut url = String::new();
                    if let Some(Kind::Web(w)) = self.tab_mut(tab).map(|t| &mut t.kind) {
                        w.title = title.clone();
                        url = w.url.clone();
                    }
                    self.profile.record_visit(&url, &title);
                    self.dirty = true;
                }
                Event::OpenTab { url, background } => {
                    let g = self.active_tab().and_then(|t| t.group);
                    self.add_tab(&url, g, !background, None);
                }
                Event::Navigate { tab, url } => self.navigate(tab, &url),
                Event::Download { tab, url, path } => {
                    let private_tab = self.tab(tab).filter(|t| t.private).map(|t| t.id);
                    self.toast(if private_tab.is_some() { format!("Downloading to {} — it is deleted with the private tab unless you keep it (Downloads)", path.display()) } else { format!("Downloading to {}", path.display()) });
                    self.downloads.push(Download { url, path, private_tab, keep: false });
                }
                Event::PageText { tab, text } => {
                    self.remember_page(tab, &text);
                    // "Ask all tabs" collects every page before it sends.
                    if let Some(g) = &mut self.ai.gather {
                        if g.waiting.remove(&tab) {
                            let (title, url) = self.tabs.iter().find(|t| t.id == tab).map(|t| (t.title(), t.url())).unwrap_or_default();
                            g.pages.push((title, url, text.clone()));
                        }
                        if g.waiting.is_empty() {
                            self.finish_gather();
                        }
                        continue;
                    }
                    if tab == self.active {
                        if let Some(q) = self.ai.pending_question.take() {
                            let url = self.tab(tab).map(|t| t.url()).unwrap_or_default();
                            self.send_ai(q, PageContext::Web { url, text });
                        }
                    }
                }
                Event::Remote(q, list) => {
                    if q == self.omni.typed {
                        self.omni.remote = list;
                    } else if q == self.ntp.typed {
                        self.ntp.remote = list;
                    }
                }
                Event::AiReply(r) => {
                    self.ai.waiting = false;
                    let text = match r {
                        Ok(s) => s,
                        Err(e) => format!("⚠ {e}"),
                    };
                    self.ai.msgs.push(Msg::new(false, text));
                    self.ai.reply_at = self.ctx.input(|i| i.time);
                }
                Event::Clipped { rect, image } => self.on_clipped(rect, &image),
                Event::Dictate(h) => self.on_heard(h),
                Event::PageAnswer { tab, ask, text } => self.on_page_answer(tab, ask, text),
                Event::Translated { tab, result } => match result {
                    Ok(json) => {
                        if let Some(Kind::Web(w)) = self.tab(tab).map(|t| &t.kind) {
                            web::js(w, &crate::page_js::translate_apply(&json));
                        }
                        self.toast("Translated in place. Reload the page to see the original.");
                    }
                    Err(e) => self.toast(format!("Translation failed: {e}")),
                },
                Event::Instant(q, text) => {
                    if self.profile.settings.instant {
                        if q == self.omni.typed && self.omni.editing {
                            crate::omnibox::put_answer(&mut self.omni.sugg, &q, text.clone());
                        }
                        if q == self.ntp.typed && self.ntp.editing {
                            crate::omnibox::put_answer(&mut self.ntp.sugg, &q, text);
                        }
                    }
                }
                Event::Unlocked { what, ok } => match (what, ok) {
                    (Unlock::PrivateTabs, true) => self.private_locked = false,
                    (Unlock::SecretFavorites, true) => self.secrets = Some((crate::passwords::secrets_load(self.demo), Instant::now())),
                    (_, false) => self.toast("Touch ID didn't confirm it was you"),
                },
                Event::Selection { tab, text } => {
                    let text = text.trim().to_owned();
                    if text.is_empty() {
                        self.toast("Select some text on the page first");
                    } else {
                        let url = self.tab(tab).map(|t| t.url()).unwrap_or_default();
                        let label: String = text.chars().take(40).collect();
                        self.ai.attach.push(Attached { a: crate::ai::Attach::Text { text: text.chars().take(8000).collect(), url }, label, thumb: None });
                        self.ai.open = true;
                        self.ai.focus = true;
                    }
                }
                Event::Vpn(s) => self.vpn.status = s,
                Event::VpnRelays(r) => self.vpn.relays = r,
                Event::VpnLogin(r) => {
                    self.vpn.login_busy = false;
                    match r {
                        Ok(()) => {
                            self.vpn.login_error = None;
                            self.toast("Signed in to Mullvad");
                        }
                        Err(e) => self.vpn.login_error = Some(e),
                    }
                }
                Event::AdblockRules(json, n) => {
                    self.ad.state = adblock::State::Compiling(n);
                    // The filter lists are cached now: Servo's blocker can use them too.
                    if let Some(h) = &self.servo {
                        h.load_filters();
                    }
                    #[cfg(target_os = "macos")]
                    adblock::mac::compile(&json, self.ad.slot.clone(), self.ctx.clone());
                    let _ = json;
                }
                Event::AdblockFailed(e) => self.ad.state = adblock::State::Failed(e),
                Event::Menu(id) => self.on_menu(&id),
                Event::Favicon { host, bytes } => self.favicons.put(&host, &bytes),
                Event::ServoMenu { tab, actions, link, image, selection, editable } => {
                    if tab == self.active {
                        let at = self.ctx.pointer_latest_pos().unwrap_or(self.content_rect.center());
                        self.page_menu = Some(PageMenu { tab, at, opened: Instant::now(), actions, link, image, selection, editable });
                    } else if let Some(Kind::Web(w)) = self.tab(tab).map(|t| &t.kind) {
                        if let Some(sv) = &w.sv {
                            sv.menu_pick(None);
                        }
                    }
                }
                Event::ServoCookies { req, list } => self.on_servo_cookies(req, list),
                Event::WkCookies(list) => self.on_wk_cookies(list),
                Event::Imported(r) => self.apply_import(*r),
                Event::Consent { tab, what } => {
                    self.cookie.consent.insert(tab, what);
                }
                Event::ServoUrl { tab, url } => {
                    if let Some(Kind::Web(w)) = self.tab_mut(tab).map(|t| &mut t.kind) {
                        w.url = url.clone();
                    }
                    if tab == self.active && !self.omni.editing {
                        self.omni.text = url;
                    }
                }
            }
        }
        #[cfg(target_os = "macos")]
        {
            let done = self.ad.slot.borrow_mut().take();
            if let Some(r) = done {
                match r {
                    Ok(list) => {
                        let n = match self.ad.state {
                            adblock::State::Compiling(n) => n,
                            _ => 0,
                        };
                        self.ad.state = adblock::State::Ready(n);
                        self.ad.list = Some(list);
                    }
                    Err(e) => self.ad.state = adblock::State::Failed(e),
                }
            }
        }
    }

    fn on_inert(&mut self, id: u64, e: InertEvent) {
        let raw = RAW_PARAMS.with(|r| r.borrow().get(&id).cloned()).unwrap_or_default();
        let mut visit: Option<(String, String)> = None;
        let mut pin_update: Option<(String, u64)> = None;
        let active = self.active;
        let Some(t) = self.tab_mut(id) else { return };
        let Kind::Inert(it) = &mut t.kind else { return };
        match e {
            InertEvent::Connected { manifest, key_id } => {
                it.status = Status::Ready;
                it.key_id = key_id;
                let (screen, params) = match it.pending_open.take() {
                    Some((s, _)) if manifest.screens.contains_key(&s) => {
                        let p = inert_tab::typed_params(&manifest, &s, &raw);
                        (s, p)
                    }
                    _ => (manifest.entry.clone(), Fields::new()),
                };
                pin_update = Some((it.host.clone(), manifest.version));
                it.manifest = Some(manifest);
                it.open(&screen, params, false);
            }
            InertEvent::Screen(s) => {
                it.busy = false;
                if let Some((_, p)) = it.pending_open.take() {
                    it.params = p;
                }
                it.screen = Some(s);
                it.reset_forms();
                visit = Some((it.url(), it.title()));
            }
            InertEvent::ActionDone(o) => {
                it.busy = false;
                match o {
                    inert_client::ActionOutcome::Ok(next) => {
                        if let Some(n) = next {
                            if it.screen.as_ref().map(|s| &s.screen) != Some(&n.screen) {
                                if let Some(cur) = &it.screen {
                                    it.back.push((cur.screen.clone(), it.params.clone()));
                                    it.fwd.clear();
                                }
                                it.params = Fields::new();
                            }
                            it.screen = Some(n);
                            it.reset_forms();
                            visit = Some((it.url(), it.title()));
                        }
                    }
                    inert_client::ActionOutcome::Error { code, field } => match field {
                        Some(f) => {
                            let base = render::field_message(code);
                            let msg = match (code, it.field_hint(&f)) {
                                (ErrorCode::FieldInvalid, Some(h)) => format!("{base} {h}"),
                                _ => base.to_owned(),
                            };
                            it.field_errors.insert(f, msg);
                        }
                        None => it.banner = Some(format!("The site refused this action ({}).", code.name())),
                    },
                }
            }
            InertEvent::Asset { hash, mime, bytes } => {
                let img = match inert_tab::decode_image(&mime, &bytes) {
                    Some(ci) => Img::Ready(self.ctx.load_texture(format!("inert-{}", hex::encode(&hash[..8])), ci, egui::TextureOptions::LINEAR)),
                    None => Img::Broken,
                };
                if let Some(Kind::Inert(it)) = self.tab_mut(id).map(|t| &mut t.kind) {
                    it.images.insert(hash, img);
                }
            }
            InertEvent::Failed(f) => {
                it.busy = false;
                it.status = Status::Failed;
                it.failure = Some(f);
                it.cmd = None;
            }
        }
        if let Some((u, t)) = visit {
            if id == active && !self.omni.editing {
                self.omni.text = u.clone();
            }
            self.profile.record_visit(&u, &t);
            self.dirty = true;
        }
        if let Some((host, v)) = pin_update {
            if let Some(p) = self.profile.pins.get_mut(&host) {
                if v > p.max_version {
                    p.max_version = v;
                    self.dirty = true;
                }
            }
        }
    }

    /// Re-pins a host to the key in its (dev) trust file after the user agreed.
    pub fn trust_new_key(&mut self, id: u64) {
        let host = match self.tab(id).map(|t| &t.kind) {
            Some(Kind::Inert(it)) => it.host.clone(),
            _ => return,
        };
        self.profile.pins.remove(&host);
        self.dirty = true;
        let url = format!("inert://{host}");
        let k = self.make_kind(id, &url, None);
        if let Some(t) = self.tab_mut(id) {
            t.kind = k;
        }
        self.toast(format!("Pinned the new key for {host}"));
    }

    pub fn apply_acts(&mut self, id: u64, acts: Vec<Act>) {
        for a in acts {
            let Some(Kind::Inert(it)) = self.tab_mut(id).map(|t| &mut t.kind) else { return };
            match a {
                Act::Open(s, p) => it.open(&s, p, true),
                Act::Submit { action, fields } => {
                    let Some(scr) = it.screen.clone() else { continue };
                    it.busy = true;
                    it.field_errors.clear();
                    it.banner = None;
                    it.last_action = Some(action.clone());
                    it.send(InertCmd::Action { screen: scr.screen.clone(), data: scr.data.clone(), action, fields });
                }
                Act::Fetch(h) => {
                    it.images.insert(h, Img::Loading);
                    it.send(InertCmd::Asset(h));
                }
                Act::LocalError(f, m) => {
                    it.field_errors.insert(f, m);
                }
                Act::External(h) => {
                    it.confirm_href = Some(h);
                }
            }
        }
    }

    // ------------------------------------------------------------------ AI

    pub fn ai_ready(&self) -> Option<Provider> {
        self.profile.settings.provider
    }

    pub fn model(&self) -> String {
        let p = self.profile.settings.provider.unwrap_or(Provider::Claude);
        if self.profile.settings.model.trim().is_empty() { p.default_model().to_owned() } else { self.profile.settings.model.clone() }
    }

    pub fn ask_ai(&mut self, q: String) {
        if self.ai.all_tabs {
            self.ask_all_tabs(q);
            return;
        }
        self.ai.open = true;
        let attach = self.ai.attach.drain(..).map(|a| a.a).collect();
        self.ai.msgs.push(Msg { user: true, text: q.clone(), attach });
        self.ai.waiting = true;
        let id = self.active;
        let is_web = matches!(self.tab(id).map(|t| &t.kind), Some(Kind::Web(w)) if w.view.is_some() || w.sv.is_some());
        let inert = match self.tab(id).map(|t| &t.kind) {
            Some(Kind::Inert(it)) => Some(PageContext::Inert { url: it.url(), text: it.semantic_text() }),
            _ => None,
        };
        if is_web {
            self.ai.pending_question = Some(q);
            if let Some(Kind::Web(w)) = self.tab(id).map(|t| &t.kind) {
                web::read_text(w, id, &self.tx, &self.ctx);
            }
        } else {
            // No page here: the assistant gets the open tabs and recent pages instead.
            let page = inert.unwrap_or_else(|| {
                let mut list: Vec<String> = self.tabs.iter().filter(|t| !matches!(t.kind, Kind::Page(_))).map(|t| format!("open tab: {} — {}", t.title(), t.url())).collect();
                list.extend(self.profile.visits.iter().rev().take(20).map(|v| format!("recent: {} — {}", v.title, v.url)));
                if list.is_empty() { PageContext::None } else { PageContext::Tabs { list: list.join("\n") } }
            });
            self.send_ai(q, page);
        }
    }

    /// "Ask all tabs": one question, with the text of every open page.
    pub fn ask_all_tabs(&mut self, q: String) {
        self.ai.open = true;
        let attach = self.ai.attach.drain(..).map(|a| a.a).collect();
        self.ai.msgs.push(Msg { user: true, text: q.clone(), attach });
        self.ai.waiting = true;
        let mut g = Gather { q, waiting: Default::default(), pages: Vec::new(), started: Instant::now() };
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        for t in self.tabs.iter().filter(|t| !t.private).take(24) {
            match &t.kind {
                // A live page is read now; a sleeping one gives its title and address.
                Kind::Web(w) if w.view.is_some() || w.sv.is_some() => {
                    g.waiting.insert(t.id);
                    web::read_text(w, t.id, &tx, &ctx);
                }
                Kind::Web(w) => g.pages.push((w.title.clone(), w.url.clone(), "(not loaded)".into())),
                Kind::Inert(it) => g.pages.push((t.title(), it.url(), it.semantic_text())),
                Kind::Page(_) => {}
            }
        }
        self.ai.gather = Some(g);
        if self.ai.gather.as_ref().is_some_and(|g| g.waiting.is_empty()) {
            self.finish_gather();
        }
    }

    /// Every page answered (or time ran out): the question goes to the model.
    pub fn finish_gather(&mut self) {
        let Some(mut g) = self.ai.gather.take() else { return };
        // Pages that did not answer in time still count, by title and address.
        for id in g.waiting.drain() {
            if let Some(t) = self.tabs.iter().find(|t| t.id == id) {
                g.pages.push((t.title(), t.url(), "(no answer)".into()));
            }
        }
        let page = if g.pages.is_empty() { PageContext::None } else { PageContext::Many { pages: g.pages } };
        self.send_ai(g.q, page);
    }

    /// Runs one of Inert's page scripts in the active page.
    pub fn ask_page(&mut self, ask: Ask, script: &str) {
        let id = self.active;
        match self.tab(id).map(|t| &t.kind) {
            Some(Kind::Web(w)) => {
                if let Some(sv) = &w.sv {
                    crate::servo_web::ask(sv, id, ask, script);
                } else if let Some(v) = &w.view {
                    let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
                    let _ = v.evaluate_script_with_callback(script, move |s| {
                        let text = serde_json::from_str::<String>(&s).unwrap_or(s);
                        let _ = tx.send(Event::PageAnswer { tab: id, ask, text });
                        ctx.request_repaint();
                    });
                }
            }
            Some(Kind::Inert(it)) if ask == Ask::Readable => {
                let text = it.semantic_text();
                self.on_page_answer(id, ask, text);
            }
            _ => self.toast("That works on web pages"),
        }
    }

    fn on_page_answer(&mut self, tab: u64, ask: Ask, text: String) {
        match ask {
            Ask::Pip => {
                if text == "none" {
                    self.toast("No video is playing on this page");
                }
            }
            Ask::Captions => self.toast(if text == "0" || text.is_empty() { "This video carries no captions of its own" } else { "Captions toggled" }),
            Ask::Readable => {
                let text = text.trim().to_owned();
                if text.len() < 2 {
                    self.toast("Nothing to read on this page");
                    return;
                }
                self.stop_reading();
                // `say` reads from a file; the text never touches a shell.
                let path = std::env::temp_dir().join(format!("inert-read-{}.txt", std::process::id()));
                if std::fs::write(&path, text).is_ok() {
                    match std::process::Command::new("/usr/bin/say").arg("-f").arg(&path).spawn() {
                        Ok(c) => self.say = Some(c),
                        Err(e) => self.toast(format!("Couldn't start speaking: {e}")),
                    }
                }
            }
            Ask::Translate => {
                let Some(p) = self.profile.settings.provider else { return };
                let lang = crate::chrome::ui_language().to_owned();
                crate::ai::translate(p, self.model(), text, lang, tab, self.tx.clone(), self.ctx.clone());
            }
        }
    }

    /// Whether "Read aloud" is speaking right now.
    pub fn reading(&mut self) -> bool {
        match &mut self.say {
            Some(c) => match c.try_wait() {
                Ok(None) => true,
                _ => {
                    self.say = None;
                    false
                }
            },
            None => false,
        }
    }

    pub fn stop_reading(&mut self) {
        if let Some(mut c) = self.say.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    /// Rewind: keeps a page's text so it can be found again later.
    fn remember_page(&mut self, tab: u64, text: &str) {
        if !self.profile.settings.rewind {
            return;
        }
        let Some(t) = self.tab(tab).filter(|t| !t.private) else { return };
        let (url, title) = (t.url(), t.title());
        if !url.starts_with("http") || text.trim().len() < 40 {
            return;
        }
        let text: String = text.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(6000).collect();
        self.rewind.retain(|s| s.url != url);
        self.rewind.push(Snap { url, title, text, t: crate::profile::now() });
        if self.rewind.len() > 1500 {
            let drop = self.rewind.len() - 1500;
            self.rewind.drain(..drop);
        }
        self.rewind_dirty = true;
        self.dirty = true;
    }

    /// Forgets Rewind's pages seen since `since` (0 = all of them).
    pub fn clear_rewind_since(&mut self, since: u64) {
        self.rewind.retain(|s| s.t < since);
        self.rewind_dirty = true;
    }

    /// Split view on or off. On: the page that was open before joins on the right.
    pub fn toggle_split(&mut self) {
        if self.split.take().is_some() {
            return;
        }
        let ok = |t: &Tab| matches!(t.kind, Kind::Web(_) | Kind::Inert(_));
        let other = self.tab(self.prev_active).filter(|t| t.id != self.active && ok(t)).map(|t| t.id).or_else(|| self.tabs.iter().rev().find(|t| t.id != self.active && ok(t)).map(|t| t.id));
        match other {
            Some(id) => self.split = Some(id),
            None => self.toast("Open a second page first. Then: Split view, or right-click a tab → Open in split view."),
        }
    }

    /// Asks for Touch ID; `Event::Unlocked` carries the answer.
    pub fn ask_touch_id(&mut self, what: Unlock, reason: &str) {
        if self.demo {
            // The demo profile has nothing to protect and no one to ask.
            let _ = self.tx.send(Event::Unlocked { what, ok: true });
            return;
        }
        let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
        #[cfg(target_os = "macos")]
        web::mac::touch_id(reason, move |ok| {
            let _ = tx.send(Event::Unlocked { what, ok });
            ctx.request_repaint();
        });
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (reason, tx.send(Event::Unlocked { what, ok: false }), ctx);
        }
    }

    /// Adds the active page to the secret favorites (no unlock needed to add).
    pub fn add_secret_favorite(&mut self) {
        let (url, title) = self.active_tab().map(|t| (t.url(), t.title())).unwrap_or_default();
        if !url.starts_with("http") && !url.starts_with("inert://") {
            self.toast("Open a page first");
            return;
        }
        let mut list = crate::passwords::secrets_load(self.demo);
        if !list.iter().any(|b| b.url == url) {
            list.push(crate::profile::Bookmark { title, url });
        }
        match crate::passwords::secrets_save(&list, self.demo) {
            Ok(()) => {
                if let Some((open, _)) = &mut self.secrets {
                    *open = list;
                }
                self.toast("Added to secret favorites (Bookmarks → Secret favorites)");
            }
            Err(e) => self.toast(format!("Keychain error: {e}")),
        }
    }

    /// Whether a URL's site is on the always-private list.
    pub fn wants_private(&self, url: &str) -> bool {
        if self.profile.settings.private_sites.is_empty() || !url.starts_with("http") {
            return false;
        }
        let site = crate::cookies::site_of(&crate::cookies::host_of(url));
        self.profile.settings.private_sites.contains(&site)
    }

    /// A private tab is about to exist: remember where the clipboard stood.
    fn note_private_opened(&mut self) {
        #[cfg(target_os = "macos")]
        if self.private_clip.is_none() {
            self.private_clip = Some(web::mac::pasteboard_count());
        }
        self.last_input = Instant::now();
    }

    /// Opens a private tab: a WebKit view with its own data store that is
    /// never written to disk and shared with no other tab.
    pub fn add_private_tab(&mut self, url: &str) -> u64 {
        self.note_private_opened();
        let id = self.add_tab(url, None, true, None);
        if let Some(t) = self.tab_mut(id) {
            t.private = true;
        }
        id
    }

    /// A private tab closed: its downloads vanish unless kept, and when it
    /// was the last one, whatever was copied meanwhile is cleared.
    fn wipe_private(&mut self, id: u64) {
        self.downloads.retain(|d| {
            if d.private_tab == Some(id) && !d.keep {
                let _ = std::fs::remove_file(&d.path);
                false
            } else {
                true
            }
        });
        if !self.tabs.iter().any(|t| t.private) {
            #[cfg(target_os = "macos")]
            if let Some(before) = self.private_clip.take() {
                if web::mac::pasteboard_count() != before {
                    web::mac::pasteboard_clear();
                }
            }
            self.private_locked = false;
        }
    }

    /// Closes every private tab at once. Returns how many there were.
    pub fn close_private_tabs(&mut self) -> usize {
        let ids: Vec<u64> = self.tabs.iter().filter(|t| t.private).map(|t| t.id).collect();
        for id in &ids {
            self.close_tab(*id);
        }
        // Private tabs fold away without a ghost.
        self.closing.clear();
        ids.len()
    }

    /// The panic key: every private tab is gone and Inert steps aside.
    pub fn panic(&mut self) {
        self.close_private_tabs();
        self.popover = Popover::None;
        self.ai.open = false;
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
    }

    /// Per-frame upkeep for private tabs: erase when idle, hide from capture.
    fn private_upkeep(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        if ctx.input(|i| !i.events.is_empty() || i.pointer.is_moving()) {
            self.last_input = Instant::now();
        }
        let any = self.tabs.iter().any(|t| t.private);
        let idle = self.profile.settings.private_idle_min;
        if any && idle > 0 {
            if self.last_input.elapsed() > Duration::from_secs(idle as u64 * 60) {
                let n = self.close_private_tabs();
                self.toast(format!("{n} private tab{} erased after {idle} idle minutes", if n == 1 { "" } else { "s" }));
            } else {
                ctx.request_repaint_after(Duration::from_secs(20));
            }
        }
        // Lock private tabs when Inert goes to the background.
        if any && self.profile.settings.private_lock && !ctx.input(|i| i.focused) {
            self.private_locked = true;
        }
        let hide = self.profile.settings.private_no_capture && self.active_tab().is_some_and(|t| t.private);
        if hide != self.capture_hidden {
            self.capture_hidden = hide;
            #[cfg(target_os = "macos")]
            web::mac::set_capture_hidden(frame, hide);
        }
        let _ = frame;
    }

    /// Option+Space went down: start listening (once; key repeat is ignored).
    pub fn dictate_start(&mut self) {
        if self.dictation.is_some() {
            return;
        }
        match crate::dictate::Dictation::start(&self.tx, &self.ctx) {
            Ok(d) => self.dictation = Some(d),
            Err(e) => self.toast(e),
        }
    }

    fn on_heard(&mut self, h: crate::dictate::Heard) {
        use crate::dictate::Heard;
        match h {
            Heard::Ready => {
                if let Some(d) = &mut self.dictation {
                    d.live = true;
                }
            }
            Heard::Partial(t) => {
                if let Some(d) = &mut self.dictation {
                    d.partial = t;
                }
            }
            Heard::Failed(e) => {
                self.dictation = None;
                self.toast(e);
            }
            Heard::Final(text) => {
                self.dictation = None;
                if text.is_empty() {
                    return;
                }
                // Typed where the keyboard is: a WebKit page, an egui field
                // or Servo page, or else the assistant's composer.
                #[cfg(target_os = "macos")]
                if let Some(Kind::Web(w)) = self.active_tab().map(|t| &t.kind) {
                    if let Some(v) = &w.view {
                        if web::mac::has_focus(v) {
                            let lit = serde_json::to_string(&text).unwrap_or_default();
                            let _ = v.evaluate_script(&format!("document.execCommand('insertText', false, {lit})"));
                            return;
                        }
                    }
                }
                if self.ctx.memory(|m| m.focused().is_some()) {
                    self.pending_input.push(egui::Event::Text(text));
                } else {
                    if !self.ai.input.is_empty() && !self.ai.input.ends_with(' ') {
                        self.ai.input.push(' ');
                    }
                    self.ai.input.push_str(&text);
                    self.ai.open = true;
                    self.ai.focus = true;
                }
            }
        }
    }

    /// Asks the active page for its selected text; `Event::Selection` answers.
    pub fn attach_selection(&mut self) {
        let id = self.active;
        match self.tab(id).map(|t| &t.kind) {
            Some(Kind::Web(w)) => {
                if let Some(sv) = &w.sv {
                    crate::servo_web::read_selection(sv, id);
                } else if let Some(v) = &w.view {
                    let (tx, ctx) = (self.tx.clone(), self.ctx.clone());
                    let _ = v.evaluate_script_with_callback("String(window.getSelection()).slice(0, 8000)", move |s| {
                        let text = serde_json::from_str::<String>(&s).unwrap_or(s);
                        let _ = tx.send(Event::Selection { tab: id, text });
                        ctx.request_repaint();
                    });
                }
            }
            _ => self.toast("Selections can be attached from web pages"),
        }
    }

    /// The clip tool's capture arrived: keep the chosen region as an attachment.
    fn on_clipped(&mut self, rect: egui::Rect, image: &egui::ColorImage) {
        let screen = self.ctx.content_rect();
        let k = image.size[0] as f32 / screen.width().max(1.0);
        let x0 = ((rect.left() * k).round().max(0.0) as usize).min(image.size[0]);
        let y0 = ((rect.top() * k).round().max(0.0) as usize).min(image.size[1]);
        let x1 = ((rect.right() * k).round().max(0.0) as usize).min(image.size[0]);
        let y1 = ((rect.bottom() * k).round().max(0.0) as usize).min(image.size[1]);
        if x1 <= x0 + 4 || y1 <= y0 + 4 {
            return;
        }
        let (w, h) = (x1 - x0, y1 - y0);
        let mut px = Vec::with_capacity(w * h);
        for y in y0..y1 {
            px.extend_from_slice(&image.pixels[y * image.size[0] + x0..y * image.size[0] + x1]);
        }
        let mut crop = egui::ColorImage::new([w, h], px);
        // Models read images best at up to ~1500 px on the long edge.
        let long = w.max(h);
        if long > 1500 {
            let rgba: Vec<u8> = crop.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
            if let Some(buf) = image::RgbaImage::from_raw(w as u32, h as u32, rgba) {
                let f = 1500.0 / long as f32;
                let small = image::imageops::resize(&buf, (w as f32 * f) as u32, (h as f32 * f) as u32, image::imageops::FilterType::Triangle);
                crop = egui::ColorImage::from_rgba_unmultiplied([small.width() as usize, small.height() as usize], small.as_raw());
            }
        }
        let png = crate::drive::png_bytes(&crop);
        let thumb = self.ctx.load_texture(format!("clip:{}", self.ai.attach.len()), crop.clone(), egui::TextureOptions::LINEAR);
        self.ai.attach.push(Attached { a: crate::ai::Attach::Image { png }, label: format!("Clip {}×{}", crop.size[0], crop.size[1]), thumb: Some(thumb) });
        self.ai.open = true;
        self.ai.focus = true;
    }

    fn send_ai(&mut self, _q: String, page: PageContext) {
        let Some(p) = self.profile.settings.provider else {
            self.ai.waiting = false;
            self.ai.msgs.push(Msg::new(false, "Choose an AI provider first (Settings → AI)."));
            return;
        };
        if self.demo {
            let reply = match &page {
                PageContext::Inert { text, .. } => format!(
                    "This is a signed inert:// screen, so I'm reading its verified structure rather than scraped HTML.\n\n{}\n\nNothing on this page can run code in your browser.",
                    text.lines().take(6).collect::<Vec<_>>().join("\n")
                ),
                _ => "Demo mode: add your own API key in Settings to get real answers.".into(),
            };
            let _ = self.tx.send(Event::AiReply(Ok(reply)));
            return;
        }
        ai::ask(p, self.model(), self.ai.msgs.clone(), page, self.tx.clone(), self.ctx.clone());
    }

    // ------------------------------------------------------------------ persistence

    fn snapshot(&mut self) {
        let order = self.visible_order();
        let mut session = Vec::new();
        for id in &order {
            if let Some(t) = self.tab(*id).filter(|t| !t.private) {
                session.push(SavedTab { url: t.url(), title: t.title(), group: t.group, pinned: t.pinned });
            }
        }
        self.profile.active = order.iter().position(|x| *x == self.active).unwrap_or(0);
        self.profile.session = session;
    }

    pub fn save_now(&mut self) {
        self.snapshot();
        self.profile.save(self.demo);
        if self.rewind_dirty {
            self.rewind_dirty = false;
            if let Ok(s) = serde_json::to_string(&self.rewind) {
                let _ = std::fs::write(rewind_path(self.demo), s);
            }
        }
        self.dirty = false;
        self.last_save = Instant::now();
    }

    fn memory_saver(&mut self) {
        if self.last_relief.elapsed() > Duration::from_secs(3) {
            self.last_relief = Instant::now();
            if std::env::var_os("INERT_DEBUG_MEM").is_some() {
                let tm = self.ctx.tex_manager();
                let tm = tm.read();
                let list: Vec<String> = tm.allocated().map(|(_, m)| format!("{}:{}x{}", m.name, m.size[0], m.size[1])).collect();
                eprintln!("MEM pass {} textures {}", self.ctx.cumulative_pass_nr(), list.join(" "));
            }
        }
        if !self.profile.settings.memory_saver {
            return;
        }
        let groups = self.profile.groups.clone();
        let active = self.active;
        for t in &mut self.tabs {
            if t.id == active {
                continue;
            }
            let inbox = t.group.and_then(|g| groups.iter().find(|x| x.id == g)).is_some_and(|g| g.kind == GroupKind::Inbox);
            if let Kind::Web(w) = &mut t.kind {
                if (w.view.is_some() || w.sv.is_some()) && !inbox && w.last_active.elapsed() > Duration::from_secs(15 * 60) {
                    w.discard();
                }
            }
        }
    }

    /// Creates, places, shows and hides web views for this frame.
    pub fn sync_webviews(&mut self, frame: &eframe::Frame, content: egui::Rect, overlay: bool) {
        let active = self.active;
        let adblock_on = self.profile.settings.adblock;
        let off_sites = self.profile.settings.adblock_off.clone();
        #[cfg(target_os = "macos")]
        let list = self.ad.list.clone();
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        let mut err = None;
        let groups = self.profile.groups.clone();
        let (default_engine, inbox_webkit) = (self.profile.settings.web_engine, self.profile.settings.inbox_webkit);
        let (hide_location, calm) = (self.profile.settings.hide_location, self.profile.settings.calm);
        // Split view: the right pane's tab (WebKit pages only keep a second live view).
        let split = self.split.filter(|s| *s != active);
        let split_rect = self.split_rect;
        // Pick an engine for the active web tab, starting Servo on first use.
        let mut need_servo = false;
        for t in &mut self.tabs {
            let Kind::Web(w) = &mut t.kind else { continue };
            if t.private {
                w.engine = Some(crate::profile::Engine::WebKit);
            }
            if w.engine.is_none() {
                let inbox = t.group.and_then(|g| groups.iter().find(|x| x.id == g)).is_some_and(|g| g.kind == crate::profile::GroupKind::Inbox);
                w.engine = Some(if inbox && inbox_webkit { crate::profile::Engine::WebKit } else { default_engine });
            }
            if t.id == active && w.engine == Some(crate::profile::Engine::Servo) {
                need_servo = true;
            }
        }
        // The Servo process died (crash or kill): drop its views; it restarts below.
        if self.servo.as_ref().is_some_and(|h| h.dead()) {
            for t in &mut self.tabs {
                if let Kind::Web(w) = &mut t.kind {
                    w.sv = None;
                }
            }
            self.servo = None;
            self.servo_restarts += 1;
            tracing::warn!("Servo process exited (restart {})", self.servo_restarts);
            if self.servo_restarts > 3 {
                self.servo_error = Some("Servo keeps exiting; using WebKit.".into());
            }
        }
        if need_servo && self.servo.is_none() && self.servo_error.is_none() {
            match crate::servo_web::Host::new(&self.ctx, Profile::dir(self.demo), self.tx.clone()) {
                Ok(h) => {
                    if !self.pending_servo_cookies.is_empty() {
                        h.set_cookies(std::mem::take(&mut self.pending_servo_cookies));
                    }
                    self.servo = Some(h)
                }
                Err(e) => self.servo_error = Some(e),
            }
        }
        if let Some(h) = &self.servo {
            h.set_adblock(self.profile.settings.adblock);
            h.set_adblock_allow(&self.profile.settings.adblock_off);
        }
        let servo_ok = self.servo.is_some();
        for t in &mut self.tabs {
            let Kind::Web(w) = &mut t.kind else { continue };
            if w.engine == Some(crate::profile::Engine::Servo) && !servo_ok {
                // Servo could not start: fall back to WebKit.
                w.engine = Some(crate::profile::Engine::WebKit);
            }
            if w.engine == Some(crate::profile::Engine::Servo) {
                w.view = None;
                if t.id == active && w.sv.is_none() {
                    if let Some(h) = &self.servo {
                        match crate::servo_web::create(h, t.id, &w.url) {
                            Ok(v) => {
                                w.sv = Some(v);
                                w.loading = true;
                            }
                            Err(e) => err = Some(e),
                        }
                    }
                }
                if t.id == active {
                    w.last_active = Instant::now();
                }
                continue;
            }
            w.sv = None;
            if t.id == active {
                w.last_active = Instant::now();
                if w.view.is_none() {
                    let opts = web::Opts { private: t.private, hide_location, calm };
                    match web::create(frame, t.id, &w.url, content, &tx, &ctx, opts) {
                        Ok(v) => {
                            #[cfg(target_os = "macos")]
                            web::mac::round(&v, crate::chrome::CARD_R as f64);
                            w.view = Some(v);
                            w.visible = true;
                            w.bounds = Some(content);
                            w.loading = true;
                        }
                        Err(e) => err = Some(e),
                    }
                }
                web::place(w, content);
                web::set_visible(w, !overlay);
            } else if split == Some(t.id) && split_rect.width() > 40.0 {
                // The right pane of split view.
                if w.view.is_none() {
                    let opts = web::Opts { private: t.private, hide_location, calm };
                    match web::create(frame, t.id, &w.url, split_rect, &tx, &ctx, opts) {
                        Ok(v) => {
                            #[cfg(target_os = "macos")]
                            web::mac::round(&v, crate::chrome::CARD_R as f64 - 4.0);
                            w.view = Some(v);
                            w.visible = true;
                            w.bounds = Some(split_rect);
                            w.loading = true;
                        }
                        Err(e) => err = Some(e),
                    }
                }
                web::place(w, split_rect);
                web::set_visible(w, !overlay);
            } else {
                web::set_visible(w, false);
            }
            #[cfg(target_os = "macos")]
            if let Some(v) = &w.view {
                // The Shield can be off for this tab's site.
                let adblock_on = adblock_on && !off_sites.contains(&crate::cookies::site_of(&crate::cookies::host_of(&w.url)));
                match (&list, adblock_on, w.has_adblock) {
                    (Some(l), true, false) => {
                        adblock::mac::attach(v, l);
                        w.has_adblock = true;
                    }
                    (_, false, true) => {
                        adblock::mac::detach(v);
                        w.has_adblock = false;
                    }
                    _ => {}
                }
            }
        }
        let _ = adblock_on;
        if let Some(e) = err {
            self.toast(format!("Could not open a web view: {e}"));
        }
    }
}

/// What the address bar shows for a URL (empty on the new tab page, like Chrome).
pub fn display_url(u: &str) -> String {
    if u == "about:newtab" { String::new() } else { u.to_owned() }
}

thread_local! {
    static RAW_PARAMS: std::cell::RefCell<HashMap<u64, Vec<(String, String)>>> = Default::default();
}

impl eframe::App for Browser {
    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        // Edit-menu commands (Cmd+C/V/X/A/Z) arrive as menu events; they
        // reach egui's text fields as input on the next frame.
        raw.events.append(&mut self.pending_input);
        if let Some(d) = &mut self.driver {
            d.inject(raw);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        crate::drive::clear();
        self.handle_events(frame);
        self.overlay_rects.clear();
        let ctx = ui.ctx().clone();
        self.shortcuts(&ctx);
        // Push-to-talk ends when Option comes back up.
        #[cfg(target_os = "macos")]
        if let Some(d) = &mut self.dictation {
            if !d.stopping && !web::mac::option_down() {
                d.stop();
            }
            ctx.request_repaint_after(Duration::from_millis(40));
        }
        // The traffic lights sit on the toolbar's line.
        #[cfg(target_os = "macos")]
        if !ctx.input(|i| i.viewport().fullscreen.unwrap_or(false)) {
            // Centered on the toolbar's line, or on the tab strip's when tabs are on top.
            let y = if self.profile.onboarded && self.profile.settings.tabs_top { crate::chrome::STRIP_H as f64 / 2.0 } else { crate::chrome::LIGHTS.1 };
            let at = web::mac::traffic_lights(frame, crate::chrome::LIGHTS.0, y);
            if std::env::var_os("INERT_DEBUG_LIGHTS").is_some() && ctx.cumulative_pass_nr() % 120 == 5 {
                eprintln!("traffic lights: close button center = {at:?}");
            }
        }

        if !self.profile.onboarded {
            self.hide_all_webviews();
            self.signin_screen(ui);
            self.after_frame(&ctx, frame);
            return;
        }

        self.private_upkeep(&ctx, frame);
        if self.ai.gather.as_ref().is_some_and(|g| g.started.elapsed() > Duration::from_secs(4)) {
            self.finish_gather();
        }
        self.chrome(ui);
        let content = self.content_rect;
        let locked = self.private_locked && self.active_tab().is_some_and(|t| t.private);
        let overlay = locked || self.popover != Popover::None || (!self.omni.sugg.is_empty() && self.omni.editing) || egui::Popup::is_any_open(&ctx) || self.rename.is_some() || self.confirming() || self.page_menu.is_some();
        self.sync_webviews(frame, content, overlay);
        self.memory_saver();
        self.after_frame(&ctx, frame);
    }

    fn on_exit(&mut self) {
        self.save_now();
    }
}

impl Browser {
    fn confirming(&self) -> bool {
        matches!(self.active_tab().map(|t| &t.kind), Some(Kind::Inert(it)) if it.confirm_href.is_some())
    }

    pub fn hide_all_webviews(&mut self) {
        for t in &mut self.tabs {
            if let Kind::Web(w) = &mut t.kind {
                web::set_visible(w, false);
            }
        }
    }

    fn after_frame(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        if self.dirty && self.last_save.elapsed() > Duration::from_secs(2) {
            self.save_now();
        }
        if let Some((_, t)) = &self.toast {
            if t.elapsed() > Duration::from_secs(4) {
                self.toast = None;
            } else {
                ctx.request_repaint_after(Duration::from_millis(250));
            }
        }
        if self.tabs.iter().any(|t| t.loading()) || self.ai.waiting {
            ctx.request_repaint_after(Duration::from_millis(120));
        }
        if self.shot.is_some() {
            crate::shot::step(self, ctx, frame);
        }
        if self.driver.is_some() {
            crate::drive::Driver::step(self, ctx);
        }
    }

    pub fn failure_code(f: &Failure) -> Option<ErrorCode> {
        match f {
            Failure::Protocol { code, .. } => *code,
            _ => None,
        }
    }
}

impl Browser {
    /// Deletes every cookie and site data in both engines (filled in by `cookies.rs`).
    pub fn clear_all_cookies(&mut self) {
        crate::cookies::clear_all(self);
    }
}
