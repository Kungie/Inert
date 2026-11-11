//! The import screens: step 2 of onboarding, `about:import`, and applying
//! what was imported to the profile, the Keychain and both cookie stores.

use crate::app::{Browser, Event};
use crate::icons::{self, Icon};
use crate::import::{self, Imported};
use crate::theme::{self, *};
use eframe::egui::{self, Color32, FontId, RichText, Sense, Stroke, Ui, Vec2, vec2};

fn pill(ui: &mut Ui, text: &str, on: bool) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).font(FontId::new(12.5, sans())).color(if on { BG } else { FG })).fill(if on { WHITE } else { RAISED }).stroke(Stroke::new(1.0, if on { WHITE } else { LINE })).corner_radius(16).min_size(vec2(0.0, 32.0)))
}

impl Browser {
    /// Source picker, checkboxes, Import button and the result. `onboarding`
    /// adds Skip / Continue, which finish the first run.
    pub fn import_panel(&mut self, ui: &mut Ui, onboarding: bool) {
        if self.import.sources.is_empty() && !self.import.blocked.is_empty() {
            // The data is there, but macOS protects it until the user allows it.
            let names = self.import.blocked.join(", ");
            ui.label(theme::label_c("macOS is protecting this data", WARN));
            ui.add_space(6.0);
            ui.label(theme::body(&format!("{names} is on this Mac, but macOS keeps other apps out of its data. Allow Inert once:")));
            ui.add_space(8.0);
            for (n, t) in [("1", "Open Privacy & Security → Full Disk Access."), ("2", "Turn on Inert (add it with + if it isn't listed)."), ("3", "Come back and press Try again.")] {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                    ui.painter().circle_filled(r.center(), 10.0, accent());
                    ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, n, FontId::new(11.0, mono()), WHITE);
                    ui.label(theme::body(t));
                });
            }
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let o = big(ui, "Open Privacy settings", true);
                crate::drive::reg("import:privacy", o.rect);
                if o.clicked() {
                    let _ = std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles").spawn();
                }
                let t = big(ui, "Try again", false);
                crate::drive::reg("import:retry", t.rect);
                if t.clicked() {
                    self.import.sources = import::sources();
                    self.import.blocked = import::blocked();
                    self.import.pick = 0;
                }
                if onboarding {
                    let s = big(ui, "Skip", false);
                    crate::drive::reg("import:skip", s.rect);
                    if s.clicked() {
                        self.finish_onboarding();
                    }
                }
            });
            ui.add_space(10.0);
            ui.label(theme::muted("Inert reads only the browser's own files, and only when you press Import. You can turn the access off again afterwards."));
            ui.label(theme::muted("Already on, and still blocked? macOS forgets the permission when Inert is updated: turn the switch off and on again, or remove Inert from the list and add it back."));
            return;
        }
        if self.import.sources.is_empty() {
            ui.label(theme::body("No Chrome, Dia, Brave, Edge, Arc, Vivaldi or Chromium profile was found on this Mac."));
            if onboarding {
                ui.add_space(18.0);
                let b = big(ui, "Continue", true);
                crate::drive::reg("import:skip", b.rect);
                if b.clicked() {
                    self.finish_onboarding();
                }
            }
            return;
        }
        // Some browsers are readable, others are kept out by macOS: say so
        // next to the ones that are, so the blocked one isn't just missing.
        if !self.import.blocked.is_empty() {
            let names = self.import.blocked.join(", ");
            egui::Frame::new().fill(WARN.gamma_multiply(0.10)).corner_radius(14).inner_margin(egui::Margin::symmetric(14, 10)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.label(theme::label_c(&format!("{names} is here, but macOS keeps Inert out"), WARN));
                    ui.label(theme::muted("Full Disk Access → Inert on, then Try again."));
                });
                ui.horizontal(|ui| {
                    if ui.add(theme::cta("Open Privacy settings", accent(), WHITE, 32.0)).clicked() {
                        let _ = std::process::Command::new("open").arg("x-apple.systempreferences:com.apple.preference.security?Privacy_AllFiles").spawn();
                    }
                    let t = ui.add(theme::cta_ghost("Try again", 32.0));
                    crate::drive::reg("import:retry-partial", t.rect);
                    if t.clicked() {
                        self.import.sources = import::sources();
                        self.import.blocked = import::blocked();
                        self.import.pick = 0;
                    }
                });
            });
            ui.add_space(12.0);
        }
        ui.label(theme::label("From"));
        ui.add_space(4.0);
        ui.horizontal_wrapped(|ui| {
            for i in 0..self.import.sources.len() {
                let l = self.import.sources[i].label();
                let r = pill(ui, &l, self.import.pick == i);
                crate::drive::reg(format!("import:source:{i}"), r.rect);
                if r.clicked() {
                    self.import.pick = i;
                }
            }
        });
        ui.add_space(14.0);
        let done = !self.import.summary.is_empty();
        if self.import.running {
            // While macOS asks for the password that unlocks Chrome's key.
            let src = &self.import.sources[self.import.pick.min(self.import.sources.len() - 1)];
            let locked = (self.import.what.passwords || self.import.what.cookies) && src.demo_key.is_none();
            ui.label(theme::title(if locked { "Unlock your data." } else { "Importing…" }, 24.0));
            ui.add_space(6.0);
            if locked {
                ui.label(theme::body("Passwords and sign-ins are encrypted. macOS will ask for your Mac's login password so Inert can read them. Choose Allow."));
                ui.label(theme::muted(&format!("The prompt names \"{}\". The password goes to macOS; Inert never sees it.", src.keychain.0)));
            }
            ui.add_space(16.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let (r, _) = ui.allocate_exact_size(vec2(250.0, 46.0), Sense::hover());
                crate::drive::reg("import:go", r);
                let p = ui.painter();
                p.rect_filled(r, 23.0, RAISED);
                let c = egui::pos2(r.left() + 26.0, r.center().y);
                let a = ui.input(|i| i.time) as f32 * 5.0;
                let arc: Vec<egui::Pos2> = (0..=10).map(|i| c + egui::vec2((a + i as f32 * 0.22).cos(), (a + i as f32 * 0.22).sin()) * 7.0).collect();
                p.add(egui::Shape::line(arc, Stroke::new(1.8, WHITE)));
                p.text(egui::pos2(r.left() + 46.0, r.center().y), egui::Align2::LEFT_CENTER, if locked { "Waiting for password…" } else { "Reading Chrome's files…" }, FontId::new(13.5, sans_medium()), FG);
                ui.ctx().request_repaint();
                if onboarding {
                    let s = big(ui, "Skip importing", false);
                    crate::drive::reg("import:skip", s.rect);
                    if s.clicked() {
                        self.finish_onboarding();
                    }
                }
            });
            return;
        }
        if !done {
            let w = &mut self.import.what;
            for (on, title, sub, key) in [
                (&mut w.bookmarks, "Bookmarks", "the bookmarks bar first, then the rest", "bookmarks"),
                (&mut w.history, "History", "visits and what the address bar should suggest", "history"),
                (&mut w.passwords, "Saved passwords", "into the macOS Keychain; fill them with the key in the address bar", "passwords"),
                (&mut w.cookies, "Sign-ins (cookies)", "stay signed in to Gmail, GitHub and the rest", "cookies"),
                (&mut w.settings, "Habits", "search engine, bookmarks bar, pages that open at startup", "settings"),
            ] {
                ui.horizontal(|ui| {
                    let r = ui.checkbox(on, RichText::new(title).font(FontId::new(15.0, sans())).color(FG));
                    crate::drive::reg(format!("import:what:{key}"), r.rect);
                    ui.label(theme::muted(sub));
                });
            }
            let src = &self.import.sources[self.import.pick.min(self.import.sources.len() - 1)];
            if (self.import.what.passwords || self.import.what.cookies) && src.demo_key.is_none() {
                ui.add_space(6.0);
                ui.label(theme::label_c(&format!("macOS will ask before Inert may read \"{}\" — choose Allow.", src.keychain.0), WARN));
            }
            ui.add_space(18.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                let running = self.import.running;
                let b = big(ui, if running { "Importing…" } else { "Import" }, true);
                crate::drive::reg("import:go", b.rect);
                if b.clicked() && !running {
                    self.start_import();
                }
                if onboarding {
                    let s = big(ui, "Skip", false);
                    crate::drive::reg("import:skip", s.rect);
                    if s.clicked() {
                        self.finish_onboarding();
                    }
                }
            });
        } else {
            for line in self.import.summary.clone() {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                    icons::paint(ui.painter(), r, Icon::Check, OK);
                    ui.label(RichText::new(line).font(FontId::new(15.0, sans())).color(FG));
                });
            }
            for e in self.import.errors.clone() {
                ui.label(RichText::new(e).font(FontId::new(12.0, mono())).color(WARN));
            }
            ui.add_space(18.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 12.0;
                if onboarding {
                    let b = big(ui, "Continue", true);
                    crate::drive::reg("import:continue", b.rect);
                    if b.clicked() {
                        self.finish_onboarding();
                    }
                } else if big(ui, "Import again", false).clicked() {
                    self.import.summary.clear();
                    self.import.errors.clear();
                }
            });
        }
    }

    pub fn finish_onboarding(&mut self) {
        self.signin.import_step = false;
        self.profile.onboarded = true;
        self.dirty = true;
    }

    fn start_import(&mut self) {
        let Some(src) = self.import.sources.get(self.import.pick).cloned() else { return };
        let what = self.import.what;
        self.import.running = true;
        let tx = self.tx.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let r = import::run(&src, what);
            let _ = tx.send(Event::Imported(Box::new(r)));
            ctx.request_repaint();
        });
    }

    pub fn apply_import(&mut self, mut r: Imported) {
        self.import.running = false;
        let mut summary = Vec::new();
        // Site icons from Chrome's cache (no network).
        for (host, png) in std::mem::take(&mut r.favicons) {
            self.favicons.put(&host, &png);
        }
        let p = &mut self.profile;
        // Bookmarks: the user's own come first (they fill the bookmarks bar),
        // Inert's starters after them; no duplicates.
        let before = p.bookmarks.len();
        let mine: Vec<_> = r.bookmarks.into_iter().filter(|b| !p.bookmarks.iter().any(|x| x.url == b.url)).collect();
        p.bookmarks.splice(0..0, mine);
        let nb = p.bookmarks.len() - before;
        if nb > 0 {
            summary.push(plural(nb, "bookmark"));
        }
        // History: per-URL counts add up; the visit timeline merges by time.
        let nh = r.history.len();
        for h in r.history {
            match p.history.iter_mut().find(|x| x.url == h.url) {
                Some(x) => {
                    x.visits += h.visits;
                    x.last = x.last.max(h.last);
                    if x.title.is_empty() {
                        x.title = h.title;
                    }
                }
                None => p.history.push(h),
            }
        }
        let nv = r.visits.len();
        p.visits.extend(r.visits);
        p.visits.sort_by_key(|v| v.t);
        p.visits.dedup();
        let len = p.visits.len();
        if len > 20_000 {
            p.visits.drain(..len - 20_000);
        }
        if nh > 0 {
            summary.push(format!("History: {}, {}", plural(nh, "site"), plural(nv, "visit")));
        }
        // Passwords into the Keychain; the profile keeps only who and where.
        let mut np = 0;
        let mut errors = r.errors;
        for l in &r.logins {
            match crate::passwords::save(&l.origin, &l.username, &l.password, self.demo) {
                Ok(()) => {
                    if !p.logins.iter().any(|x| x.origin == l.origin && x.username == l.username) {
                        p.logins.push(crate::profile::LoginRef { origin: l.origin.clone(), username: l.username.clone(), from: r.source.clone() });
                    }
                    np += 1;
                }
                Err(e) => {
                    errors.push(format!("Keychain: {e}"));
                    break;
                }
            }
        }
        if np > 0 {
            summary.push(format!("{} (in the Keychain)", plural(np, "saved password")));
        }
        // Habits.
        let mut habits = Vec::new();
        if let Some(e) = r.search {
            p.settings.search = e;
            habits.push(format!("search with {}", e.name()));
        }
        if let Some(b) = r.show_bookmarks_bar {
            p.settings.bookmarks_bar = b;
            habits.push(if b { "bookmarks bar on".into() } else { "bookmarks bar off".into() });
        }
        p.imported_from = Some(r.source.clone());
        let startup = r.startup_urls;
        if !startup.is_empty() {
            habits.push(format!("{} startup page{}", startup.len(), if startup.len() == 1 { "" } else { "s" }));
        }
        if !habits.is_empty() {
            summary.push(format!("Habits: {}", habits.join(", ")));
        }
        // Cookies: both engines, so imported sign-ins work in every tab.
        let nc = r.cookies.len();
        if nc > 0 {
            crate::cookies::import(self, &r.cookies);
            let sites: std::collections::BTreeSet<String> = r.cookies.iter().map(|(_, c)| crate::cookies::site_of(&c.domain)).collect();
            summary.push(format!("Signed in on {} ({})", plural(sites.len(), "site"), plural(nc, "cookie")));
        }
        for u in startup {
            if !self.tabs.iter().any(|t| t.url() == u) {
                self.add_tab(&u, None, false, None);
            }
        }
        if summary.is_empty() {
            summary.push("Nothing new to bring over".into());
        }
        self.import.summary = summary;
        self.import.errors = errors;
        self.dirty = true;
    }

    /// Step 2 of onboarding, in the sign-in screen's style.
    pub fn import_screen(&mut self, ui: &mut Ui) {
        crate::pages::progress(ui, 3);
        ui.label(theme::label_c("Step 3 of 3 · Permissions and import", ACCENT));
        ui.add_space(10.0);
        ui.label(theme::title("Bring Chrome with you.", 38.0));
        ui.add_space(8.0);
        ui.label(RichText::new("Bookmarks, history, passwords, sign-ins and habits move over in one go. Nothing leaves this Mac.").font(FontId::new(15.0, sans())).color(MUTED));
        ui.add_space(22.0);
        self.import_panel(ui, true);
    }

    pub fn import_page(&mut self, ui: &mut Ui) {
        ui.add_space(48.0);
        ui.label(theme::label("Import"));
        ui.label(theme::title("Bring Chrome with you.", 44.0));
        ui.add_space(6.0);
        if let Some(f) = &self.profile.imported_from {
            ui.label(theme::muted(&format!("Last import: {f}.")));
        }
        ui.add_space(16.0);
        self.import_panel(ui, false);
    }
}

/// "1 site", "2 sites".
fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

pub fn big(ui: &mut Ui, text: &str, primary: bool) -> egui::Response {
    let b = if primary { theme::cta(text, accent(), WHITE, 46.0) } else { theme::cta_ghost(text, 46.0) };
    ui.add(b.min_size(vec2(if primary { 180.0 } else { 130.0 }, 46.0)))
}
