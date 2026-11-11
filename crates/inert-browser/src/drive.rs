//! `--demo --drive <dir>`: end-to-end UI tests with real input.
//!
//! Widgets record their on-screen rects in a per-frame registry. The driver
//! turns each test step into genuine egui input events (pointer move, press,
//! release, text, keys) injected through `App::raw_input_hook`, so every click
//! travels the same path a physical click does: hit-testing, `Response`
//! state, focus, text editing, shortcuts. After each step it asserts on the
//! browser's state and writes `results.json` plus screenshot markers.

use crate::app::{Browser, Kind, Page, Popover};
use crate::inert_tab::Status;
#[allow(unused_imports)]
use crate::profile;
use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2, Rect};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::{Duration, Instant};

thread_local! {
    static ON: Cell<bool> = const { Cell::new(false) };
    static REG: RefCell<HashMap<String, Rect>> = RefCell::new(HashMap::new());
}

/// Records a widget's rect for this frame (first registration wins).
pub fn reg(key: impl Into<String>, r: Rect) {
    if ON.with(Cell::get) {
        REG.with(|m| {
            m.borrow_mut().entry(key.into()).or_insert(r);
        });
    }
}

pub fn clear() {
    if ON.with(Cell::get) {
        REG.with(|m| m.borrow_mut().clear());
    }
}

fn lookup(key: &str) -> Option<Rect> {
    REG.with(|m| m.borrow().get(key).copied())
}

type Cond = Box<dyn Fn(&Browser) -> bool>;
type Act = Box<dyn Fn(&mut Browser)>;

enum Step {
    Click(String),
    RightClick(String),
    Type(String),
    Press(Key, Modifiers),
    /// Wait for a condition, then record a pass/fail check with this name.
    Expect(String, Cond, u64),
    /// Direct setup that is not part of what is being tested.
    Setup(Act),
    Shot(String),
    Pause(u64),
}

pub struct Driver {
    out: PathBuf,
    steps: Vec<Step>,
    i: usize,
    started: Instant,
    queue: VecDeque<Vec<Event>>,
    results: Vec<(String, bool, String)>,
    shot_until: Option<Instant>,
    pending_mods: Modifiers,
    finished: bool,
}

fn expect(name: &str, timeout_ms: u64, f: impl Fn(&Browser) -> bool + 'static) -> Step {
    Step::Expect(name.to_owned(), Box::new(f), timeout_ms)
}

fn inert_on(b: &Browser, screen: &str) -> bool {
    matches!(b.active_tab().map(|t| &t.kind), Some(Kind::Inert(it)) if it.status == Status::Ready && !it.busy && it.screen.as_ref().is_some_and(|s| s.screen == screen))
}

fn data_len(b: &Browser, slot: &str) -> usize {
    match b.active_tab().map(|t| &t.kind) {
        Some(Kind::Inert(it)) => it.screen.as_ref().and_then(|s| s.data.get(slot)).and_then(|v| v.as_array()).map_or(0, <[_]>::len),
        _ => 0,
    }
}

fn dump(b: &Browser) -> String {
    let t = b.active_tab();
    format!(
        "active={:?} omni={:?} editing={} popover={:?} rename={:?} groups={:?} tabs={} closed={:?}",
        t.map(|t| t.url()),
        b.omni.text,
        b.omni.editing,
        b.popover,
        b.rename,
        b.profile.groups.iter().map(|g| g.name.clone()).collect::<Vec<_>>(),
        b.tabs.len(),
        b.closed.iter().map(|c| c.url.clone()).collect::<Vec<_>>()
    )
}

fn has_text(b: &Browser, needle: &str) -> bool {
    match b.active_tab().map(|t| &t.kind) {
        Some(Kind::Inert(it)) => it.semantic_text().contains(needle),
        _ => false,
    }
}

impl Driver {
    pub fn new(out: PathBuf) -> Self {
        ON.with(|c| c.set(true));
        let _ = std::fs::create_dir_all(&out);
        use Step::*;
        let cmd = Modifiers::COMMAND;
        let s = |x: &str| x.to_owned();
        let steps = vec![
            // 1. Sign-in: the only thing asked is the AI provider.
            Pause(1200),
            expect("signin.visible", 3000, |b| !b.profile.onboarded),
            Shot(s("d01-signin")),
            Click(s("signin:claude")),
            expect("signin.claude_selected", 2000, |b| b.signin.provider == Some(crate::profile::Provider::Claude)),
            Click(s("signin:skip")),
            Click(s("look:blue")),
            Click(s("look:next")),
            // Step 2: import from Chrome (a fake Chrome profile in demo mode).
            expect("import.step_after_signin", 2000, |b| b.signin.import_step && !b.profile.onboarded),
            Shot(s("d01b-import")),
            Click(s("import:go")),
            expect("import.reads_chrome_profile", 10000, |b| !b.import.summary.is_empty() && !b.import.running),
            expect("import.bookmarks", 1000, |b| b.profile.bookmarks.iter().any(|x| x.title == "Hacker News") && b.profile.bookmarks.iter().any(|x| x.title == "Printables")),
            expect("import.history_and_visits", 1000, |b| b.profile.history.iter().any(|h| h.url == "https://news.ycombinator.com/" && h.visits >= 42) && b.profile.visits.len() >= 4),
            expect("import.passwords_to_store", 1000, |b| b.profile.logins.len() == 2 && crate::passwords::get("https://news.ycombinator.com", "demo_user", true).as_deref() == Some("correct horse battery staple")),
            expect("import.habits", 1000, |b| b.profile.settings.search == crate::profile::SearchEngine::DuckDuckGo && b.profile.imported_from.is_some()),
            expect("import.cookies", 1000, |b| b.import.summary.iter().any(|l| l.contains("Signed in on 2 sites"))),
            Pause(300),
            Shot(s("d01c-import-done")),
            Click(s("import:continue")),
            expect("signin.done_with_claude", 2000, |b| b.profile.onboarded && b.profile.settings.provider == Some(crate::profile::Provider::Claude)),
            // Seed browsing history so completion has something to complete (setup, not under test).
            Setup(Box::new(|b| {
                for _ in 0..6 {
                    b.profile.record_visit("https://github.com/anthropics", "GitHub");
                }
            })),
            // 2. Omnibox: typed text completes inline and shows suggestions.
            Click(s("omnibox")),
            expect("omnibox.focused", 2000, |b| b.omni.editing),
            Type(s("g")),
            Type(s("i")),
            Type(s("t")),
            Type(s("h")),
            expect("omnibox.inline_completion", 2000, |b| b.omni.text == "github.com"),
            expect("omnibox.suggestions", 2000, |b| b.omni.sugg.iter().any(|x| x.target.contains("github.com"))),
            Pause(500), // let the dropdown finish fading in
            Shot(s("d02-omnibox")),
            Press(Key::Escape, Modifiers::NONE),
            expect("omnibox.escape_restores", 2000, |b| !b.omni.editing),
            // 3. Bank: open by clicking its tab, navigate, transfer.
            Click(s("tab:Inert Bank")),
            expect("bank.overview_verified", 15000, |b| inert_on(b, "overview")),
            expect("bank.secured_badge", 1000, |b| matches!(b.active_tab().map(|t| &t.kind), Some(Kind::Inert(it)) if it.key_id.starts_with("ed25519:"))),
            Click(s("link:Send money →")),
            expect("bank.transfer_screen", 8000, |b| inert_on(b, "transfer")),
            Click(s("input:to")),
            Type(s("DE89 3704 0044 0532 0130 00")),
            Click(s("input:amount")),
            Type(s("99999")),
            Click(s("btn:Send")),
            expect("bank.over_limit_refused_locally", 5000, |b| matches!(b.active_tab().map(|t| &t.kind), Some(Kind::Inert(it)) if it.field_errors.contains_key("amount"))),
            Shot(s("d03-bank-limit")),
            Click(s("input:amount")),
            Press(Key::A, cmd),
            Type(s("12.50")),
            Click(s("input:note")),
            Type(s("Driver test")),
            Setup(Box::new(|b| b.ai.input = String::new())),
            Click(s("btn:Send")),
            expect("bank.transfer_accepted", 8000, |b| inert_on(b, "overview") && has_text(b, "Driver test")),
            Shot(s("d04-bank-after")),
            // 4. Shop: catalog → product (link with params) → add to cart.
            Click(s("tab:Inert Supply")),
            expect("shop.catalog", 15000, |b| inert_on(b, "catalog") && data_len(b, "products") == 4),
            Click(s("link:View →")),
            expect("shop.product", 8000, |b| inert_on(b, "product")),
            Click(s("btn:Add to cart")),
            expect("shop.cart_has_line", 8000, |b| inert_on(b, "cart") && data_len(b, "lines") >= 1),
            Click(s("btn:Remove")),
            expect("shop.remove_line", 8000, |b| inert_on(b, "cart") && data_len(b, "lines") == 0),
            // 5. Notes: add, open, toggle, save.
            Click(s("tab:Inert Notes")),
            expect("notes.list", 15000, |b| inert_on(b, "list")),
            Click(s("input:title")),
            Type(s("Note from the UI driver")),
            Click(s("btn:Add")),
            expect("notes.added", 8000, |b| inert_on(b, "list") && has_text(b, "Note from the UI driver")),
            Click(s("link:Open →")),
            expect("notes.opened", 8000, |b| inert_on(b, "note")),
            Click(s("toggle:done")),
            Click(s("btn:Save")),
            expect("notes.saved_done", 8000, |b| inert_on(b, "note") && matches!(b.active_tab().map(|t| &t.kind), Some(Kind::Inert(it)) if it.screen.as_ref().and_then(|s| s.data.get("done")).and_then(|v| v.as_bool()) == Some(true))),
            Click(s("back")),
            expect("notes.back_button", 8000, |b| inert_on(b, "list")),
            // 6. Bookmarks: star button and shortcut (start from "not bookmarked").
            Setup(Box::new(|b| {
                let u = b.active_tab().map(|t| t.url()).unwrap_or_default();
                b.profile.bookmarks.retain(|x| x.url != u);
            })),
            Click(s("star")),
            expect("bookmark.star_adds", 2000, |b| b.profile.is_bookmarked(&b.active_tab().map(|t| t.url()).unwrap_or_default())),
            Press(Key::D, cmd),
            expect("bookmark.cmd_d_removes", 2000, |b| !b.profile.is_bookmarked(&b.active_tab().map(|t| t.url()).unwrap_or_default())),
            // 7. Tab groups: collapse/expand, new group from the menu, pin from the context menu.
            Click(s("group:Inbox")),
            expect("groups.inbox_expands", 2000, |b| b.profile.groups.iter().any(|g| g.name == "Inbox" && !g.collapsed)),
            Shot(s("d05-groups")),
            Click(s("group:Inbox")),
            expect("groups.inbox_collapses", 2000, |b| b.profile.groups.iter().any(|g| g.name == "Inbox" && g.collapsed)),
            Click(s("menu")),
            expect("menu.opens", 2000, |b| b.popover == Popover::Menu),
            Click(s("menu:New tab group")),
            expect("groups.new_group_rename", 2000, |b| b.rename.is_some()),
            Press(Key::A, cmd),
            Type(s("Research")),
            Press(Key::Enter, Modifiers::NONE),
            expect("groups.renamed", 2000, |b| b.profile.groups.iter().any(|g| g.name == "Research")),
            RightClick(s("tab:Inert Notes")),
            Click(s("ctx:Pin")),
            expect("tabs.pin_from_context_menu", 2000, |b| b.tabs.iter().any(|t| t.pinned && t.title().contains("Notes"))),
            // 8. Normal web: new tab, type a URL, Enter, it loads as unsecured.
            Press(Key::T, cmd),
            expect("tabs.cmd_t_new_tab", 2000, |b| matches!(b.active_tab().map(|t| &t.kind), Some(Kind::Page(Page::NewTab)))),
            // The new tab page's own search box: typing stays in it.
            Click(s("ntp:search")),
            Type(s("gith")),
            expect("ntp.search_types_in_place", 2000, |b| b.ntp.text == "github.com" && b.ntp.editing && !b.ntp.sugg.is_empty() && !b.omni.editing && b.omni.text.is_empty()),
            Pause(300),
            Shot(s("d05b-ntp-search")),
            Press(Key::Escape, Modifiers::NONE),
            expect("ntp.escape_clears", 2000, |b| b.ntp.text.is_empty() && !b.ntp.editing),
            Click(s("omnibox")),
            Type(s("example.com")),
            Press(Key::Enter, Modifiers::NONE),
            expect("web.servo_renders_page", 20000, |b| matches!(b.active_tab().map(|t| &t.kind), Some(Kind::Web(w)) if w.engine == Some(profile::Engine::Servo) && w.sv.is_some() && w.url.starts_with("https://example.com") && !w.loading && w.title == "Example Domain")),
            Pause(800),
            Shot(s("d06-web-servo")),
            Click(s("cookies")),
            expect("cookies.panel_servo", 5000, |b| b.popover == Popover::Cookies && !b.cookie.loading && b.cookie.host == "example.com"),
            Pause(400),
            Shot(s("d06c-cookies")),
            Press(Key::Escape, Modifiers::NONE),
            // Engine switch from the site security panel: same page in WebKit.
            Click(s("badge")),
            Click(s("engine:switch")),
            expect("web.switch_to_webkit", 15000, |b| matches!(b.active_tab().map(|t| &t.kind), Some(Kind::Web(w)) if w.engine == Some(profile::Engine::WebKit) && w.view.is_some() && w.sv.is_none())),
            Click(s("back")),
            expect("web.back_to_new_tab", 5000, |b| matches!(b.active_tab().map(|t| &t.kind), Some(Kind::Page(Page::NewTab)))),
            // Input forwarding into Servo: a click on a page-filling link…
            Setup(Box::new(|b| {
                let id = b.active;
                b.navigate(id, "data:text/html,<a href=\"https://example.com/?clicked=1\" style=\"display:block;position:fixed;left:0;top:0;right:0;bottom:0;background:%23ffd400;font:48px sans-serif\">click anywhere</a>");
            })),
            expect("servo.page_ready", 15000, |b| matches!(b.active_tab().map(|t| &t.kind), Some(Kind::Web(w)) if w.sv.is_some() && !w.loading)),
            Pause(600),
            Click(s("servo:surface")),
            expect("servo.click_follows_link", 15000, |b| matches!(b.active_tab().map(|t| &t.kind), Some(Kind::Web(w)) if w.url.contains("clicked=1"))),
            // Let that page finish before navigating away from it (setup, not under test).
            Pause(2500),
            // …and typing into a page-filling text field.
            Setup(Box::new(|b| {
                let id = b.active;
                b.navigate(id, "data:text/html,<input style=\"position:fixed;left:0;top:0;width:100%;height:100%;font:48px sans-serif\" oninput=\"document.title=this.value\" placeholder=\"type here\">");
            })),
            expect("servo.form_ready", 15000, |b| matches!(b.active_tab().map(|t| &t.kind), Some(Kind::Web(w)) if w.sv.is_some() && !w.loading && w.url.starts_with("data:"))),
            Pause(600),
            Click(s("servo:surface")),
            Type(s("servo ok")),
            expect("servo.typing_reaches_page", 10000, |b| matches!(b.active_tab().map(|t| &t.kind), Some(Kind::Web(w)) if w.title == "servo ok")),
            Shot(s("d06b-servo-typing")),
            Press(Key::W, cmd),
            // Inbox tabs open in WebKit for compatibility.
            Click(s("group:Inbox")),
            Click(s("tab:Gmail")),
            expect("inbox.uses_webkit", 15000, |b| matches!(b.active_tab().map(|t| &t.kind), Some(Kind::Web(w)) if w.engine == Some(profile::Engine::WebKit) && w.view.is_some())),
            Click(s("cookies")),
            expect("cookies.panel_webkit", 5000, |b| b.popover == Popover::Cookies && !b.cookie.loading),
            Press(Key::Escape, Modifiers::NONE),
            Click(s("group:Inbox")),
            Press(Key::T, cmd),
            Press(Key::W, cmd),
            expect("tabs.cmd_w_closes", 2000, |b| b.closed.iter().any(|s| s.url == "about:newtab")),
            Press(Key::T, Modifiers::COMMAND | Modifiers::SHIFT),
            expect("tabs.cmd_shift_t_reopens", 3000, |b| b.closed.iter().all(|s| s.url != "about:newtab") && matches!(b.active_tab().map(|t| &t.kind), Some(Kind::Page(Page::NewTab)))),
            // 9. Panels: VPN, Shield, assistant.
            Click(s("vpn")),
            expect("vpn.panel", 2000, |b| b.popover == Popover::Vpn),
            Shot(s("d07-vpn")),
            Click(s("vpn:logout")),
            Click(s("vpn:logout-confirm")),
            expect("vpn.logout", 5000, |b| b.vpn.status.logged_in == Some(false)),
            Pause(300),
            Shot(s("d07b-vpn-login")),
            Click(s("vpn:account")),
            Type(s("1234")),
            Click(s("vpn:login")),
            expect("vpn.login_rejects_short_number", 2000, |b| b.vpn.login_error.is_some() && b.vpn.status.logged_in == Some(false)),
            Click(s("vpn:account")),
            Type(s(" 5678 9012 3456")),
            Click(s("vpn:login")),
            expect("vpn.login_with_account_number", 5000, |b| b.vpn.status.logged_in == Some(true) && b.vpn.account_input.is_empty() && b.vpn.login_error.is_none()),
            Press(Key::Escape, Modifiers::NONE),
            expect("vpn.closes", 2000, |b| b.popover == Popover::None),
            Click(s("shield")),
            expect("shield.panel", 2000, |b| b.popover == Popover::Shield),
            Press(Key::Escape, Modifiers::NONE),
            Click(s("tab:Inert Bank")),
            expect("bank.back_on_tab", 5000, |b| inert_on(b, "overview")),
            Press(Key::J, cmd),
            expect("assistant.cmd_j", 2000, |b| b.ai.open),
            Click(s("ai:input")),
            Type(s("What can I do here?")),
            Press(Key::Enter, Modifiers::NONE),
            expect("assistant.answers", 8000, |b| b.ai.msgs.len() >= 2 && !b.ai.waiting),
            Shot(s("d08-assistant")),
            Press(Key::J, cmd),
            // 10. History and reload.
            Press(Key::Y, cmd),
            expect("history.cmd_y", 2000, |b| matches!(b.active_tab().map(|t| &t.kind), Some(Kind::Page(Page::History)))),
            expect("history.has_entries", 2000, |b| b.profile.history.iter().any(|h| h.url.starts_with("inert://bank.example"))),
            expect("history.timeline", 2000, |b| b.profile.visits.iter().any(|v| v.url.starts_with("inert://bank.example"))),
            Pause(300),
            Shot(s("d09-history")),
            Click(s("tab:Inert Bank")),
            Press(Key::R, cmd),
            expect("bank.reload_reverifies", 15000, |b| inert_on(b, "overview")),
            // 11. Cookie banners are rejected automatically (a OneTrust-style banner).
            Press(Key::T, cmd),
            Setup(Box::new(|b| {
                let id = b.active;
                b.navigate(id, "data:text/html,<title>banner</title><div id=\"onetrust-banner-sdk\"><p>We use cookies</p><button onclick=\"document.title='accepted'\">Accept all</button><button id=\"onetrust-reject-all-handler\" onclick=\"document.title='rejected'\">Reject all</button></div>");
            })),
            expect("cookies.banner_rejected", 15000, |b| matches!(b.active_tab().map(|t| &t.kind), Some(Kind::Web(w)) if w.title == "rejected")),
            Setup(Box::new(|b| {
                let id = b.active;
                b.navigate(id, "data:text/html,<title>banner2</title><div class=\"cookie-consent\"><button onclick=\"document.title='accepted'\">Accept</button><button onclick=\"document.title='rejected by text'\">Only necessary</button></div>");
            })),
            expect("cookies.banner_rejected_by_text", 15000, |b| matches!(b.active_tab().map(|t| &t.kind), Some(Kind::Web(w)) if w.title == "rejected by text")),
            // 12. A saved password fills on its own site only when asked (local test page).
            Setup(Box::new(|b| {
                let _ = crate::passwords::save("http://127.0.0.1:8766", "ada", "pw!", true);
                b.profile.logins.push(crate::profile::LoginRef { origin: "http://127.0.0.1:8766".into(), username: "ada".into(), from: "test".into() });
                let id = b.active;
                b.navigate(id, "http://127.0.0.1:8766/login.html");
            })),
            expect("passwords.key_in_address_bar", 15000, |b| matches!(b.active_tab().map(|t| &t.kind), Some(Kind::Web(w)) if !w.loading && w.title == "login") && b.logins_here().len() == 1),
            Click(s("omni:key")),
            expect("passwords.panel", 2000, |b| b.popover == Popover::Passwords),
            Pause(400),
            Shot(s("d10-password-fill")),
            Click(s("pw:fill:0")),
            expect("passwords.filled", 5000, |b| matches!(b.active_tab().map(|t| &t.kind), Some(Kind::Web(w)) if w.title == "ada:3")),
            // 13. Search runs on DuckDuckGo in its dark theme.
            Click(s("omnibox")),
            Type(s("inert browser")),
            Press(Key::Enter, Modifiers::NONE),
            expect("search.duckduckgo_dark", 5000, |b| b.active_tab().map(|t| t.url()).is_some_and(|u| u.starts_with("https://duckduckgo.com/") && u.contains("kae=d"))),
            // 14. Clear browsing data from the history page.
            Press(Key::Y, cmd),
            Click(s("history:clear")),
            Click(s("clear:All time")),
            Click(s("clear:go")),
            expect("history.clear_all", 3000, |b| b.profile.visits.is_empty() && b.profile.history.is_empty()),
        ];
        // INERT_DRIVE_SCENARIO=bank-idle: sign in, open Inert Bank, then stay
        // on it for 30 s with the window drawing (memory baseline, spec §16).
        let steps = if std::env::var("INERT_DRIVE_SCENARIO").as_deref() == Ok("bank-idle") {
            vec![
                Pause(1200),
                Click(s("signin:claude")),
                Click(s("signin:skip")),
            Click(s("look:blue")),
            Click(s("look:next")),
                Click(s("import:skip")),
                expect("signin.done_with_claude", 2000, |b| b.profile.onboarded),
                Click(s("tab:Inert Bank")),
                expect("bank.overview_verified", 15000, |b| inert_on(b, "overview")),
                Pause(30000),
                expect("bank.still_verified", 1000, |b| inert_on(b, "overview")),
            ]
        } else if std::env::var("INERT_DRIVE_SCENARIO").as_deref() == Ok("real") {
            // A real (non-demo) run: the actual Mullvad state and a live search.
            vec![
                Pause(1500),
                Click(s("signin:claude")),
                Click(s("signin:skip")),
            Click(s("look:blue")),
            Click(s("look:next")),
                Pause(300),
                Shot(s("r00-import")),
                Click(s("import:skip")),
                expect("signin.done", 2000, |b| b.profile.onboarded),
                Click(s("vpn")),
                expect("vpn.real_state_known", 10000, |b| b.vpn.status.logged_in.is_some()),
                Pause(500),
                Shot(s("r01-vpn")),
                Press(Key::Escape, Modifiers::NONE),
                Press(Key::T, Modifiers::COMMAND),
                Click(s("ntp:search")),
                Type(s("servo web engine")),
                Press(Key::Enter, Modifiers::NONE),
                expect("search.ddg_dark_url", 5000, |b| b.active_tab().map(|t| t.url()).is_some_and(|u| u.contains("duckduckgo.com") && u.contains("kae=d"))),
                expect("search.ddg_loaded", 30000, |b| matches!(b.active_tab().map(|t| &t.kind), Some(Kind::Web(w)) if !w.loading && w.title.contains("DuckDuckGo"))),
                Pause(4000),
                Shot(s("r02-ddg-dark")),
            ]
        } else if std::env::var("INERT_DRIVE_SCENARIO").as_deref() == Ok("import-real") {
            // Onboarding import from a profile a real Chrome wrote (see INERT_IMPORT_DIR).
            vec![
                Pause(1200),
                Click(s("signin:claude")),
                Click(s("signin:skip")),
            Click(s("look:blue")),
            Click(s("look:next")),
                expect("import.step", 2000, |b| b.signin.import_step),
                Click(s("import:go")),
                expect("import.done", 10000, |b| !b.import.summary.is_empty() && !b.import.running && b.import.errors.is_empty()),
                expect("import.real_bookmark", 1000, |b| b.profile.bookmarks.iter().any(|x| x.title == "Inert Test Mark")),
                expect("import.real_history", 1000, |b| b.profile.history.iter().any(|h| h.title == "Alpha page" && h.visits >= 2)),
                expect("import.real_password", 1000, |b| b.profile.logins.iter().any(|l| l.username == "ada") && crate::passwords::get("https://login.example.com", "ada", true).as_deref() == Some("hunter2-real")),
                expect("import.real_cookie", 1000, |b| b.import.summary.iter().any(|l| l.contains("Signed in on 1 site (1 cookie)"))),
                Pause(300),
                Shot(s("i1-real-chrome-import")),
                Click(s("import:continue")),
                expect("signin.done", 2000, |b| b.profile.onboarded),
                Pause(400),
                Shot(s("i2-bookmarks-bar")),
            ]
        } else if std::env::var("INERT_DRIVE_SCENARIO").as_deref() == Ok("vpn") {
            // The VPN panel in whatever state INERT_DEMO_VPN sets; with
            // logged-out it also signs in with a (demo) account number.
            let signin = std::env::var("INERT_DEMO_VPN").as_deref() == Ok("logged-out");
            let mut v = vec![
                Pause(1200),
                Click(s("signin:claude")),
                Click(s("signin:skip")),
            Click(s("look:blue")),
            Click(s("look:next")),
                Click(s("import:skip")),
                expect("signin.done", 2000, |b| b.profile.onboarded),
                Click(s("vpn")),
                expect("vpn.panel", 2000, |b| b.popover == Popover::Vpn),
                Pause(500),
                Shot(s("v1-panel")),
            ];
            if signin {
                v.extend([
                    Click(s("vpn:account")),
                    Type(s("1234 5678 9012 3456")),
                    Pause(300),
                    Shot(s("v2-number-typed")),
                    Click(s("vpn:login")),
                    expect("vpn.signed_in", 5000, |b| b.vpn.status.logged_in == Some(true)),
                    Pause(500),
                    Shot(s("v3-signed-in")),
                ]);
            }
            v
        } else if std::env::var("INERT_DRIVE_SCENARIO").as_deref() == Ok("look") {
            look_steps()
        } else {
            steps
        };
        Driver { out, steps, i: 0, started: Instant::now(), queue: VecDeque::new(), results: Vec::new(), shot_until: None, pending_mods: Modifiers::NONE, finished: false }
    }

    /// Feeds queued events into egui's input for this frame.
    pub fn inject(&mut self, raw: &mut egui::RawInput) {
        if let Some(batch) = self.queue.pop_front() {
            for e in &batch {
                if let Event::Key { modifiers, .. } = e {
                    raw.events.push(Event::ModifiersChanged(*modifiers));
                }
            }
            raw.events.extend(batch);
        } else if self.pending_mods != Modifiers::NONE {
            raw.events.push(Event::ModifiersChanged(Modifiers::NONE));
            self.pending_mods = Modifiers::NONE;
        }
    }

    fn click(&mut self, at: Pos2, button: PointerButton) {
        let m = Modifiers::NONE;
        self.queue.push_back(vec![Event::PointerMoved(at)]);
        self.queue.push_back(vec![Event::PointerButton { pos: at, button, pressed: true, modifiers: m }]);
        self.queue.push_back(vec![Event::PointerButton { pos: at, button, pressed: false, modifiers: m }]);
        self.queue.push_back(vec![]);
    }

    fn record(&mut self, name: &str, ok: bool, note: String) {
        eprintln!("{} {name} {note}", if ok { "PASS" } else { "FAIL" });
        self.results.push((name.to_owned(), ok, note));
    }

    pub fn step(b: &mut Browser, ctx: &egui::Context) {
        ctx.request_repaint_after(Duration::from_millis(30));
        // A pass egui is about to lay out again has widgets in the wrong
        // places; act on the pass that is actually shown.
        if ctx.will_discard() {
            return;
        }
        let Some(mut d) = b.driver.take() else { return };
        d.advance(b, ctx);
        if !d.finished {
            b.driver = Some(d);
        }
    }

    fn advance(&mut self, b: &mut Browser, ctx: &egui::Context) {
        if !self.queue.is_empty() {
            return;
        }
        if let Some(t) = self.shot_until {
            if Instant::now() < t {
                return;
            }
            self.shot_until = None;
            self.i += 1;
            self.started = Instant::now();
        }
        if self.i >= self.steps.len() {
            self.finish(ctx);
            return;
        }
        if matches!(self.steps[self.i], Step::Setup(_)) {
            if let Step::Setup(f) = std::mem::replace(&mut self.steps[self.i], Step::Pause(0)) {
                f(b);
            }
            self.i += 1;
            self.started = Instant::now();
            return;
        }
        enum Do {
            Click(String, bool),
            Type(String),
            Key(Key, Modifiers),
            Expect(String, bool, u64),
            Shot(String),
            Pause(u64),
        }
        let elapsed = self.started.elapsed();
        let what = match &self.steps[self.i] {
            Step::Click(k) => Do::Click(k.clone(), false),
            Step::RightClick(k) => Do::Click(k.clone(), true),
            Step::Type(t) => Do::Type(t.clone()),
            Step::Press(k, m) => Do::Key(*k, *m),
            Step::Expect(n, cond, t) => Do::Expect(n.clone(), cond(b), *t),
            Step::Shot(n) => Do::Shot(n.clone()),
            Step::Pause(ms) => Do::Pause(*ms),
            Step::Setup(_) => Do::Pause(0),
        };
        let mut next = true;
        match what {
            Do::Click(k, right) => match lookup(&k) {
                Some(r) => self.click(r.center(), if right { PointerButton::Secondary } else { PointerButton::Primary }),
                None if elapsed < Duration::from_millis(4000) => next = false,
                None => self.record(&format!("click:{k}"), false, "widget not found on screen".into()),
            },
            Do::Type(t) => {
                for ch in t.chars() {
                    self.queue.push_back(vec![Event::Text(ch.to_string())]);
                }
                self.queue.push_back(vec![]);
            }
            Do::Key(k, m) => {
                self.pending_mods = m;
                self.queue.push_back(vec![Event::Key { key: k, physical_key: Some(k), pressed: true, repeat: false, modifiers: m }]);
                self.queue.push_back(vec![Event::Key { key: k, physical_key: Some(k), pressed: false, repeat: false, modifiers: m }]);
                self.queue.push_back(vec![]);
            }
            Do::Expect(name, ok, timeout) => {
                if ok {
                    self.record(&name, true, format!("({} ms)", elapsed.as_millis()));
                } else if elapsed < Duration::from_millis(timeout) {
                    next = false;
                } else {
                    let state = dump(b);
                    self.record(&name, false, format!("timed out after {timeout} ms | {state}"));
                }
            }
            Do::Shot(_) if std::env::var_os("INERT_DRIVE_NO_SHOTS").is_some() => {
                // Memory runs: egui-wgpu keeps a window-sized capture texture and
                // readback buffer after the first screenshot, which is test overhead.
            }
            Do::Shot(name) => {
                // In-app capture: works with the screen locked or asleep. Native
                // WebKit views are not part of it; everything else (Servo included) is.
                let path = self.out.join(format!("{name}.png"));
                let ectx = ctx.clone();
                ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::ScreenshotCallback::new(move |img| {
                    if std::env::var_os("INERT_DRIVE_NOSAVE").is_none() {
                        save_png(&path, &img);
                    }
                    ectx.request_repaint();
                })));
                self.shot_until = Some(Instant::now() + Duration::from_millis(900));
                return;
            }
            Do::Pause(ms) => next = elapsed >= Duration::from_millis(ms),
        }
        if next {
            self.i += 1;
            self.started = Instant::now();
        }
    }

    fn finish(&mut self, ctx: &egui::Context) {
        if self.finished {
            return;
        }
        self.finished = true;
        let pass = self.results.iter().filter(|r| r.1).count();
        let json: Vec<serde_json::Value> = self.results.iter().map(|(n, ok, note)| serde_json::json!({"check": n, "pass": ok, "note": note})).collect();
        let _ = std::fs::write(self.out.join("results.json"), serde_json::to_string_pretty(&serde_json::json!({"passed": pass, "total": self.results.len(), "checks": json})).unwrap_or_default());
        eprintln!("UI DRIVER: {pass}/{} checks passed", self.results.len());
        let _ = std::fs::write(self.out.join("DONE"), "done");
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

/// INERT_DRIVE_SCENARIO=look: a short visual tour for checking the chrome.
fn look_steps() -> Vec<Step> {
    use Step::*;
    let s = |x: &str| x.to_owned();
    vec![
        Pause(1200),
        Shot(s("l00-signin")),
        Click(s("signin:claude")),
        Pause(300),
        Shot(s("l00b-key")),
        Click(s("signin:skip")),
            Click(s("look:blue")),
            Click(s("look:next")),
        Pause(300),
        Shot(s("l00c-import")),
        Click(s("import:skip")),
        expect("signin.done", 2000, |b| b.profile.onboarded),
        Pause(600),
        Shot(s("l01-newtab")),
        Click(s("ntp:search")),
        Type(s("gith")),
        Pause(500),
        Shot(s("l01b-composer")),
        Press(Key::Escape, Modifiers::NONE),
        Click(s("tab:Inert Bank")),
        expect("bank.overview", 15000, |b| inert_on(b, "overview")),
        Pause(400),
        Shot(s("l02-bank")),
        Click(s("group:Inbox")),
        Pause(80),
        Shot(s("l03-group-opening")),
        Pause(500),
        Shot(s("l04-group-open")),
        Click(s("omnibox")),
        Type(s("gith")),
        Pause(400),
        Shot(s("l05-omnibox")),
        Press(Key::Escape, Modifiers::NONE),
        Click(s("vpn")),
        Pause(400),
        Shot(s("l06-vpn")),
        Press(Key::Escape, Modifiers::NONE),
        Click(s("menu")),
        Pause(400),
        Shot(s("l07-menu")),
        Click(s("menu:Settings")),
        Pause(500),
        Shot(s("l08-settings")),
        Press(Key::Y, Modifiers::COMMAND),
        Pause(500),
        Shot(s("l09-history")),
        Click(s("ai")),
        Pause(500),
        Shot(s("l10-assistant")),
        Click(s("ai")),
        Click(s("sidebar")),
        Pause(600),
        Shot(s("l11-sidebar-hidden")),
        Click(s("sidebar")),
        Pause(400),
        // The dark look (setup: what the Settings switch does).
        Setup(Box::new(|b| {
            b.profile.settings.dark = Some(true);
            crate::theme::set_dark(true);
        })),
        Press(Key::T, Modifiers::COMMAND),
        Pause(700),
        Shot(s("l12-dark-newtab")),
        Click(s("tab:Inert Bank")),
        Pause(600),
        Shot(s("l13-dark-bank")),
    ]
}

/// An image as PNG bytes.
pub fn png_bytes(img: &egui::ColorImage) -> Vec<u8> {
    let mut out = Vec::new();
    let mut e = png::Encoder::new(&mut out, img.size[0] as u32, img.size[1] as u32);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    if let Ok(mut w) = e.write_header() {
        let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
        let _ = w.write_image_data(&bytes);
    }
    out
}

pub fn save_png(path: &std::path::Path, img: &egui::ColorImage) {
    let Ok(f) = std::fs::File::create(path) else { return };
    let mut e = png::Encoder::new(std::io::BufWriter::new(f), img.size[0] as u32, img.size[1] as u32);
    e.set_color(png::ColorType::Rgba);
    e.set_depth(png::BitDepth::Eight);
    if let Ok(mut w) = e.write_header() {
        let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
        let _ = w.write_image_data(&bytes);
    }
}
