//! The user's profile: settings, bookmarks, history, tab groups, session and
//! key pins. Stored as JSON in the app data directory. Secrets (AI keys) are
//! never stored here — they live in the macOS Keychain (see `ai.rs`).

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    Claude,
    Gpt,
}

impl Provider {
    pub fn name(self) -> &'static str {
        match self {
            Provider::Claude => "Claude",
            Provider::Gpt => "GPT",
        }
    }
    pub fn default_model(self) -> &'static str {
        match self {
            // The everyday tier of each: same price ($2 / $10 per MTok).
            Provider::Claude => "claude-sonnet-5-5",
            Provider::Gpt => "gpt-6.1-sol",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchEngine {
    DuckDuckGo,
    Google,
    Brave,
}

impl SearchEngine {
    pub const ALL: [SearchEngine; 3] = [SearchEngine::DuckDuckGo, SearchEngine::Google, SearchEngine::Brave];
    pub fn name(self) -> &'static str {
        match self {
            SearchEngine::DuckDuckGo => "DuckDuckGo",
            SearchEngine::Google => "Google",
            SearchEngine::Brave => "Brave Search",
        }
    }
    pub fn search_url(self, q: &str) -> String {
        let q = url::form_urlencoded::byte_serialize(q.as_bytes()).collect::<String>();
        match self {
            // kae=d: DuckDuckGo's dark theme.
            SearchEngine::DuckDuckGo => format!("https://duckduckgo.com/?q={q}&kae=d"),
            SearchEngine::Google => format!("https://www.google.com/search?q={q}"),
            SearchEngine::Brave => format!("https://search.brave.com/search?q={q}"),
        }
    }
}

/// Engine for the unsecured web.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Engine {
    /// Servo, the Rust web engine (default).
    #[default]
    Servo,
    /// The system WebKit (WKWebView), for maximum compatibility.
    WebKit,
}

impl Engine {
    pub fn name(self) -> &'static str {
        match self {
            Engine::Servo => "Servo",
            Engine::WebKit => "WebKit",
        }
    }
    pub fn other(self) -> Engine {
        match self {
            Engine::Servo => Engine::WebKit,
            Engine::WebKit => Engine::Servo,
        }
    }
}

fn yes() -> bool {
    true
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub provider: Option<Provider>,
    pub model: String,
    pub search: SearchEngine,
    pub remote_suggestions: bool,
    pub adblock: bool,
    pub memory_saver: bool,
    pub bookmarks_bar: bool,
    pub vpn_last_location: Option<String>,
    #[serde(default)]
    pub web_engine: Engine,
    /// Inbox tabs (mail, chat) use WebKit for compatibility.
    #[serde(default = "yes")]
    pub inbox_webkit: bool,
    /// Cookies extension: reject cookie banners automatically.
    #[serde(default = "yes")]
    pub cookie_banners: bool,
    /// Cookies extension: sites (eTLD+1) whose cookies are deleted after every load.
    #[serde(default)]
    pub cookie_block: Vec<String>,
    /// The tab sidebar is showing (Cmd+S hides it).
    #[serde(default = "yes")]
    pub sidebar: bool,
    /// Dark look (dark frame, blue glyphs) instead of the blue frame.
    /// `None` until the user picks one on first run.
    #[serde(default)]
    pub dark: Option<bool>,
    /// Index into `theme::ACCENTS`.
    #[serde(default)]
    pub accent: usize,
    /// Tabs in a strip on top instead of the sidebar.
    #[serde(default)]
    pub tabs_top: bool,
    /// Assistant panel: 0 sidebar, 1 floating.
    #[serde(default)]
    pub ai_mode: u8,
    /// The "log in to all your sites" nudge was dismissed.
    #[serde(default)]
    pub import_nudge_off: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            provider: None,
            model: String::new(),
            search: SearchEngine::DuckDuckGo,
            remote_suggestions: true,
            adblock: true,
            memory_saver: true,
            bookmarks_bar: true,
            vpn_last_location: None,
            web_engine: Engine::Servo,
            inbox_webkit: true,
            cookie_banners: true,
            cookie_block: Vec::new(),
            sidebar: true,
            dark: None,
            accent: 0,
            tabs_top: false,
            ai_mode: 0,
            import_nudge_off: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Bookmark {
    pub title: String,
    pub url: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub url: String,
    pub title: String,
    pub visits: u32,
    pub last: u64,
}

/// A saved login. The password itself is in the Keychain (`passwords.rs`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LoginRef {
    pub origin: String,
    pub username: String,
    #[serde(default)]
    pub from: String,
}

/// One visit, for the history page's timeline.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Visit {
    pub url: String,
    pub title: String,
    pub t: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroupKind {
    Normal,
    /// Mail, chat and other inboxes.
    Inbox,
    /// Self-hosted services on the home network.
    Homeserver,
    /// Web development: local dev servers, docs, repos.
    Dev,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GroupDef {
    pub id: u64,
    pub name: String,
    pub color: String,
    pub kind: GroupKind,
    pub collapsed: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SavedTab {
    pub url: String,
    pub title: String,
    pub group: Option<u64>,
    #[serde(default)]
    pub pinned: bool,
}

/// What a browser remembers about an `inert://` host (spec §8).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Pin {
    pub key_b64: String,
    pub pinned_at: u64,
    pub max_version: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
pub struct Profile {
    pub settings: Settings,
    pub bookmarks: Vec<Bookmark>,
    pub history: Vec<HistoryEntry>,
    /// Every visit, newest last (capped). `history` keeps per-URL counts.
    #[serde(default)]
    pub visits: Vec<Visit>,
    #[serde(default)]
    pub logins: Vec<LoginRef>,
    /// Where the last import came from ("Chrome · Default").
    #[serde(default)]
    pub imported_from: Option<String>,
    pub groups: Vec<GroupDef>,
    pub session: Vec<SavedTab>,
    pub active: usize,
    pub pins: BTreeMap<String, Pin>,
    pub next_id: u64,
    #[serde(default)]
    pub onboarded: bool,
}

pub fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

impl Profile {
    pub fn dir(demo: bool) -> PathBuf {
        if let Ok(p) = std::env::var("INERT_PROFILE_DIR") {
            return PathBuf::from(p);
        }
        let base = dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("Inert");
        if demo { base.join("demo-profile") } else { base.join("profile") }
    }

    pub fn load(demo: bool) -> Profile {
        let p = Self::dir(demo).join("profile.json");
        std::fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_else(Profile::first_run)
    }

    pub fn save(&self, demo: bool) {
        let d = Self::dir(demo);
        let _ = std::fs::create_dir_all(&d);
        let tmp = d.join("profile.json.tmp");
        if let Ok(s) = serde_json::to_string_pretty(self) {
            if std::fs::write(&tmp, s).is_ok() {
                let _ = std::fs::rename(&tmp, d.join("profile.json"));
            }
        }
    }

    pub fn new_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Defaults on first launch: the three special groups, the demo sites, a bookmarks bar.
    pub fn first_run() -> Profile {
        let mut p = Profile { next_id: 100, ..Default::default() };
        let g = |id, name: &str, color: &str, kind| GroupDef { id, name: name.into(), color: color.into(), kind, collapsed: true };
        p.groups = vec![
            g(1, "Inbox", "blue", GroupKind::Inbox),
            g(2, "Homeserver", "green", GroupKind::Homeserver),
            g(3, "Developer", "orange", GroupKind::Dev),
            GroupDef { id: 4, name: "Inert demos".into(), color: "purple".into(), kind: GroupKind::Normal, collapsed: false },
        ];
        let t = |url: &str, title: &str, group| SavedTab { url: url.into(), title: title.into(), group, pinned: false };
        p.session = vec![
            t("https://mail.google.com/", "Gmail", Some(1)),
            t("https://web.whatsapp.com/", "WhatsApp", Some(1)),
            t("http://homeassistant.local:8123/", "Home Assistant", Some(2)),
            t("http://localhost:3000/", "localhost:3000", Some(3)),
            t("https://developer.mozilla.org/", "MDN Web Docs", Some(3)),
            t("inert://bank.example", "Inert Bank", Some(4)),
            t("inert://shop.example", "Inert Supply", Some(4)),
            t("inert://notes.example", "Inert Notes", Some(4)),
            t("about:newtab", "New Tab", None),
        ];
        p.active = p.session.len() - 1;
        let b = |title: &str, url: &str| Bookmark { title: title.into(), url: url.into() };
        p.bookmarks = vec![
            b("Inert Bank", "inert://bank.example"),
            b("Inert Supply", "inert://shop.example"),
            b("Inert Notes", "inert://notes.example"),
            b("GitHub", "https://github.com/"),
            b("MDN", "https://developer.mozilla.org/"),
            b("Wikipedia", "https://www.wikipedia.org/"),
            b("crates.io", "https://crates.io/"),
        ];
        p
    }

    pub fn record_visit(&mut self, url: &str, title: &str) {
        if url.starts_with("about:") || url.is_empty() {
            return;
        }
        let t = now();
        if let Some(h) = self.history.iter_mut().find(|h| h.url == url) {
            h.visits += 1;
            h.last = t;
            if !title.is_empty() {
                h.title = title.to_owned();
            }
        } else {
            self.history.push(HistoryEntry { url: url.into(), title: title.into(), visits: 1, last: t });
        }
        // A title arriving right after the load updates that visit instead of adding one.
        match self.visits.last_mut() {
            Some(v) if v.url == url && t.saturating_sub(v.t) < 60 => {
                if !title.is_empty() {
                    v.title = title.to_owned();
                }
            }
            _ => self.visits.push(Visit { url: url.into(), title: title.into(), t }),
        }
        if self.visits.len() > 20_000 {
            self.visits.drain(..5_000);
        }
        if self.history.len() > 5000 {
            self.history.sort_by(|a, b| b.last.cmp(&a.last));
            self.history.truncate(4000);
        }
    }

    /// Forgets visits since `since` (0 = everything).
    pub fn clear_history_since(&mut self, since: u64) {
        self.visits.retain(|v| v.t < since);
        if since == 0 {
            self.history.clear();
        } else {
            // Recount from what is left.
            self.history.retain(|h| h.last < since || self.visits.iter().any(|v| v.url == h.url));
            for h in &mut self.history {
                if h.last >= since {
                    if let Some(v) = self.visits.iter().rev().find(|v| v.url == h.url) {
                        h.last = v.t;
                    }
                }
            }
        }
    }

    pub fn forget_url(&mut self, url: &str) {
        self.visits.retain(|v| v.url != url);
        self.history.retain(|h| h.url != url);
    }

    pub fn is_bookmarked(&self, url: &str) -> bool {
        self.bookmarks.iter().any(|b| b.url == url)
    }

    pub fn toggle_bookmark(&mut self, url: &str, title: &str) {
        if let Some(i) = self.bookmarks.iter().position(|b| b.url == url) {
            self.bookmarks.remove(i);
        } else if !url.starts_with("about:") {
            self.bookmarks.push(Bookmark { title: if title.is_empty() { url.into() } else { title.into() }, url: url.into() });
        }
    }
}

/// Quick-add presets for the special groups.
pub fn presets(kind: GroupKind) -> &'static [(&'static str, &'static str)] {
    match kind {
        GroupKind::Inbox => &[
            ("Gmail", "https://mail.google.com/"),
            ("Outlook", "https://outlook.live.com/mail/"),
            ("Proton Mail", "https://mail.proton.me/"),
            ("WhatsApp", "https://web.whatsapp.com/"),
            ("Telegram", "https://web.telegram.org/"),
            ("Slack", "https://app.slack.com/client"),
            ("Discord", "https://discord.com/app"),
            ("Messenger", "https://www.messenger.com/"),
        ],
        GroupKind::Homeserver => &[
            ("Home Assistant", "http://homeassistant.local:8123/"),
            ("Proxmox", "https://proxmox.local:8006/"),
            ("Portainer", "https://localhost:9443/"),
            ("Pi-hole", "http://pi.hole/admin/"),
            ("Jellyfin", "http://localhost:8096/"),
            ("Router", "http://192.168.1.1/"),
            ("OctoPrint", "http://octopi.local/"),
            ("Grafana", "http://localhost:3001/"),
        ],
        GroupKind::Dev => &[
            ("localhost:3000", "http://localhost:3000/"),
            ("localhost:5173", "http://localhost:5173/"),
            ("localhost:8080", "http://localhost:8080/"),
            ("GitHub", "https://github.com/"),
            ("MDN Web Docs", "https://developer.mozilla.org/"),
            ("Can I use", "https://caniuse.com/"),
            ("docs.rs", "https://docs.rs/"),
            ("Inert Bank (dev)", "inert://bank.example"),
        ],
        GroupKind::Normal => &[],
    }
}

/// DuckDuckGo starts in its dark theme (`kae=d`) unless the URL picks one.
pub fn dark_web(url: &str) -> String {
    let Ok(mut u) = url::Url::parse(url) else { return url.to_owned() };
    let ddg = matches!(u.host_str(), Some("duckduckgo.com" | "www.duckduckgo.com" | "start.duckduckgo.com"));
    if ddg && !u.query_pairs().any(|(k, _)| k == "kae") {
        u.query_pairs_mut().append_pair("kae", "d");
        return u.to_string();
    }
    url.to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn duckduckgo_dark() {
        assert_eq!(dark_web("https://duckduckgo.com/"), "https://duckduckgo.com/?kae=d");
        assert_eq!(dark_web("https://duckduckgo.com/?q=rust"), "https://duckduckgo.com/?q=rust&kae=d");
        assert_eq!(dark_web("https://duckduckgo.com/?q=rust&kae=-1"), "https://duckduckgo.com/?q=rust&kae=-1");
        assert_eq!(dark_web("https://example.com/"), "https://example.com/");
        assert!(SearchEngine::DuckDuckGo.search_url("a b").ends_with("&kae=d"));
    }
}
