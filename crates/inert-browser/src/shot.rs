//! `--demo --shot <dir>`: walks through the browser's features with demo
//! data and writes `<dir>/NN-name.ready` markers when a screen is settled, so
//! an external `screencapture` loop can photograph exactly what is on screen
//! (including native web views). Demo mode never touches the real profile,
//! the real VPN state or real API keys.

use crate::app::{Browser, Kind, Popover};
use crate::inert_tab::{Failure, FormValue, Status};
use eframe::egui;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub struct Shot {
    dir: PathBuf,
    step: usize,
    since: Instant,
    marked: Option<Instant>,
}

impl Shot {
    pub fn new(dir: PathBuf) -> Self {
        let _ = std::fs::create_dir_all(&dir);
        Shot { dir, step: 0, since: Instant::now(), marked: None }
    }
}

fn find_tab(b: &Browser, url_prefix: &str) -> Option<u64> {
    b.tabs.iter().find(|t| t.url().starts_with(url_prefix)).map(|t| t.id)
}

fn inert_ready(b: &Browser, id: u64, screen: &str) -> bool {
    matches!(b.tab(id).map(|t| &t.kind), Some(Kind::Inert(it)) if it.status == Status::Ready && !it.busy && it.screen.as_ref().is_some_and(|s| s.screen == screen))
}

fn images_loaded(b: &Browser, id: u64) -> bool {
    matches!(b.tab(id).map(|t| &t.kind), Some(Kind::Inert(it)) if it.images.values().all(|i| !matches!(i, crate::inert_tab::Img::Loading)))
}

pub fn step(b: &mut Browser, ctx: &egui::Context, _frame: &eframe::Frame) {
    ctx.request_repaint_after(Duration::from_millis(100));
    let Some(shot) = b.shot.as_mut() else { return };
    // After marking, give the capture loop time, then advance.
    if let Some(m) = shot.marked {
        if m.elapsed() < Duration::from_millis(1700) {
            return;
        }
        shot.marked = None;
        shot.step += 1;
        shot.since = Instant::now();
        return;
    }
    let step = shot.step;
    let elapsed = shot.since.elapsed();
    let settle = |d: u64| elapsed > Duration::from_millis(d);
    let mut mark: Option<&str> = None;
    match step {
        0 => {
            // Seed some history so completion has something to complete.
            if b.profile.history.is_empty() {
                for (u, t, n) in [("https://github.com/anthropics", "GitHub", 9), ("https://developer.mozilla.org/en-US/docs/Web", "MDN Web Docs", 5), ("https://docs.rs/egui", "egui — docs.rs", 3), ("https://news.ycombinator.com/", "Hacker News", 4)] {
                    for _ in 0..n {
                        b.profile.record_visit(u, t);
                    }
                }
            }
            if settle(1800) {
                mark = Some("01-signin");
            }
        }
        1 => {
            b.signin.provider = Some(crate::profile::Provider::Claude);
            if settle(900) {
                mark = Some("02-signin-key");
            }
        }
        2 => {
            if !b.profile.onboarded {
                b.signin.provider = None;
                b.profile.settings.provider = Some(crate::profile::Provider::Claude);
                b.profile.settings.model = "claude-opus-5-5".into();
                b.profile.onboarded = true;
                b.refresh_sites();
            }
            if settle(1500) {
                mark = Some("03-new-tab");
            }
        }
        3 => {
            if let Some(id) = find_tab(b, "inert://bank.example") {
                if b.active != id {
                    b.activate(id);
                }
                if inert_ready(b, id, "overview") && images_loaded(b, id) && settle(1200) {
                    mark = Some("04-bank-overview");
                }
            }
            if settle(15000) {
                mark = Some("04-bank-overview");
            }
        }
        4 => {
            let id = b.active;
            if let Some(Kind::Inert(it)) = b.tab_mut(id).map(|t| &mut t.kind) {
                if it.screen.as_ref().is_some_and(|s| s.screen == "overview") && !it.busy {
                    it.open("transfer", Default::default(), true);
                }
            }
            if inert_ready(b, id, "transfer") {
                if let Some(Kind::Inert(it)) = b.tab_mut(id).map(|t| &mut t.kind) {
                    it.forms.insert(("transfer".into(), "to".into()), FormValue::Text("DE89 3704 0044 0532 0130 00".into()));
                    it.forms.insert(("transfer".into(), "amount".into()), FormValue::Text("25000".into()));
                    it.forms.insert(("transfer".into(), "note".into()), FormValue::Text("Rent, November".into()));
                }
                if settle(1500) {
                    mark = Some("05-bank-transfer");
                }
            }
        }
        5 => {
            // Submit an amount over the signed max: refused locally, field error shown.
            let id = b.active;
            if elapsed < Duration::from_millis(200) {
                let acts = vec![crate::render::Act::Submit {
                    action: "transfer".into(),
                    fields: {
                        let mut f = inert_proto::Fields::new();
                        f.insert("from".into(), inert_proto::Value::text("chk"));
                        f.insert("to".into(), inert_proto::Value::text("DE89 3704 0044 0532 0130 00"));
                        f.insert("amount".into(), inert_proto::Decimal::new(2_500_000, 2).to_value());
                        f.insert("note".into(), inert_proto::Value::text("Rent, November"));
                        f
                    },
                }];
                b.apply_acts(id, acts);
            }
            if settle(1600) {
                mark = Some("06-bank-field-error");
            }
        }
        6 => {
            if let Some(id) = find_tab(b, "inert://shop.example") {
                if b.active != id {
                    b.activate(id);
                }
                if inert_ready(b, id, "catalog") && images_loaded(b, id) && settle(1500) {
                    mark = Some("07-shop-catalog");
                }
            }
            if settle(15000) {
                mark = Some("07-shop-catalog");
            }
        }
        7 => {
            let id = b.active;
            if let Some(Kind::Inert(it)) = b.tab_mut(id).map(|t| &mut t.kind) {
                if it.screen.as_ref().is_some_and(|s| s.screen == "catalog") && !it.busy {
                    let mut p = inert_proto::Fields::new();
                    p.insert("id".into(), inert_proto::Value::text("lamp"));
                    it.open("product", p, true);
                }
            }
            if inert_ready(b, id, "product") && images_loaded(b, id) && settle(1200) {
                mark = Some("08-shop-product");
            }
        }
        8 => {
            if let Some(id) = find_tab(b, "inert://notes.example") {
                if b.active != id {
                    b.activate(id);
                }
                if inert_ready(b, id, "list") && settle(1200) {
                    mark = Some("09-notes");
                }
            }
            if settle(15000) {
                mark = Some("09-notes");
            }
        }
        9 => {
            for g in &mut b.profile.groups {
                g.collapsed = false;
            }
            if let Some(id) = find_tab(b, "inert://bank.example") {
                if b.active != id {
                    b.activate(id);
                }
            }
            if settle(1200) {
                mark = Some("10-tab-groups");
            }
        }
        10 => {
            if b.popover == Popover::None && elapsed < Duration::from_millis(300) {
                let r = egui::Rect::from_min_size(egui::pos2(520.0, 26.0), egui::vec2(10.0, 10.0));
                b.open_popover(Popover::GroupAdd(2), r);
            }
            if settle(1200) {
                mark = Some("11-homeserver-presets");
            }
        }
        11 => {
            b.popover = Popover::None;
            b.omni.editing = true;
            b.omni.typed = "gith".into();
            b.omni.text = "github.com".into();
            let tabs: Vec<(u64, String, String)> = b.tabs.iter().map(|t| (t.id, t.title(), t.url())).collect();
            b.omni.sugg = crate::omnibox::local(&b.profile, &tabs, &b.sites, "gith", Some("Claude"));
            b.omni.remote = vec!["github copilot".into(), "github actions".into(), "github desktop".into()];
            if settle(1200) {
                mark = Some("12-omnibox-completion");
            }
        }
        12 => {
            b.omni.editing = false;
            b.omni.sugg.clear();
            b.omni.remote.clear();
            if b.popover != Popover::Vpn && elapsed < Duration::from_millis(300) {
                let r = egui::Rect::from_min_size(egui::pos2(b.content_rect.right() - 120.0, 8.0), egui::vec2(80.0, 24.0));
                b.open_popover(Popover::Vpn, r);
            }
            if settle(1300) {
                mark = Some("13-vpn");
            }
        }
        13 => {
            if b.popover != Popover::Security && elapsed < Duration::from_millis(300) {
                let r = egui::Rect::from_min_size(egui::pos2(110.0, 52.0), egui::vec2(120.0, 24.0));
                b.popover = Popover::None;
                b.open_popover(Popover::Security, r);
            }
            if settle(1200) {
                mark = Some("14-site-security");
            }
        }
        14 => {
            b.popover = Popover::None;
            if !b.ai.open {
                b.ask_ai("What can I do on this page?".into());
            }
            if settle(1500) {
                mark = Some("15-assistant");
            }
        }
        15 => {
            b.ai.open = false;
            if find_tab(b, "about:settings").is_none() {
                b.add_tab("about:settings", None, true, None);
            }
            if settle(1200) {
                mark = Some("16-settings");
            }
        }
        16 => {
            // What an impersonation attempt looks like.
            if find_tab(b, "inert://evil-bank.example").is_none() && elapsed < Duration::from_millis(300) {
                let id = b.add_tab("about:newtab", None, true, None);
                let mut t = crate::inert_tab::InertTab::new("bank.example", None, Default::default());
                t.status = Status::Failed;
                t.failure = Some(Failure::KeyChanged { pinned: "ed25519:9f2c41ab07de33c1".into(), offered: "ed25519:4e1100f3a2b9c8d7".into() });
                if let Some(tab) = b.tab_mut(id) {
                    tab.kind = Kind::Inert(Box::new(t));
                }
            }
            if settle(1200) {
                mark = Some("17-key-changed");
            }
        }
        17 => {
            if find_tab(b, "https://example.com").is_none() && elapsed < Duration::from_millis(300) {
                b.add_tab("https://example.com/", None, true, None);
            }
            if settle(4500) {
                mark = Some("18-web-unsecured");
            }
        }
        18 => {
            if b.popover != Popover::Shield && elapsed < Duration::from_millis(300) {
                let r = egui::Rect::from_min_size(egui::pos2(b.content_rect.right() - 90.0, 52.0), egui::vec2(30.0, 30.0));
                b.open_popover(Popover::Shield, r);
            }
            if settle(1200) {
                mark = Some("19-shield");
            }
        }
        _ => {
            let _ = std::fs::write(shot.dir.join("DONE"), "done");
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
    }
    let shot = b.shot.as_mut().unwrap();
    if let Some(name) = mark {
        let _ = std::fs::write(shot.dir.join(format!("{name}.ready")), "");
        shot.marked = Some(Instant::now());
    }
}
