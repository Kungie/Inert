//! Built-in pages: sign-in, new tab, history, bookmarks, downloads,
//! settings, and the pages shown when an inert:// site fails verification.

use crate::app::{Browser, Page};
use crate::chrome::group_icon;
use crate::icons::{self, Icon};
use crate::inert_tab::Failure;
use crate::profile::{GroupKind, Provider, SearchEngine};
use crate::theme::{self, *};
use crate::{adblock, ai};
use eframe::egui::{self, Align, Color32, FontId, Id, Layout, Rect, RichText, Sense, Stroke, Ui, Vec2, pos2, vec2};

fn primary(ui: &mut Ui, text: &str, w: f32) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).font(FontId::new(12.0, mono())).color(BG).extra_letter_spacing(1.4)).fill(FG).corner_radius(2).min_size(vec2(w, 46.0)))
}

fn ghost(ui: &mut Ui, text: &str, w: f32) -> egui::Response {
    ui.add(egui::Button::new(RichText::new(text).font(FontId::new(12.0, mono())).color(FG).extra_letter_spacing(1.4)).fill(Color32::TRANSPARENT).stroke(Stroke::new(1.0, LINE_STRONG)).corner_radius(2).min_size(vec2(w, 46.0)))
}

fn section(ui: &mut Ui, n: &str, title: &str) {
    ui.add_space(28.0);
    ui.label(theme::label(&format!("{n} — {title}")));
    ui.add_space(10.0);
}

impl Browser {
    fn paint_planet(&self, ui: &Ui, rect: Rect, alpha: f32) {
        if let Some(t) = &self.planet {
            let w = rect.width().max(rect.height() * 1.6);
            let h = w / 1.6;
            let r = Rect::from_min_size(pos2(rect.right() - w * 0.9, rect.top() - h * 0.08), vec2(w, h));
            ui.painter().with_clip_rect(rect).image(t.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE.gamma_multiply(alpha));
        }
    }

    // ------------------------------------------------------------------ sign in

    pub fn signin_screen(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, BG);
        self.paint_planet(ui, rect, 1.0);
        // Window drag on the whole background.
        let drag = ui.interact(Rect::from_min_size(rect.min, vec2(rect.width(), 48.0)), Id::unique("signin-drag"), Sense::drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(72.0, 0.0))).layout(Layout::top_down(Align::Min)));
        let ui = &mut child;
        ui.add_space(56.0);
        ui.label(RichText::new("INERT").font(FontId::new(26.0, pixel())).color(WHITE));
        ui.add_space((rect.height() * 0.22).max(40.0));
        ui.label(theme::label("inert:// — an application protocol"));
        ui.add_space(18.0);
        ui.label(RichText::new("The web,\nbehind glass.").font(FontId::new(96.0, serif())).color(WHITE).extra_letter_spacing(-3.0).line_height(Some(92.0)));
        ui.add_space(26.0);
        match self.signin.provider {
            None => {
                ui.label(RichText::new("Bring your own model. That is the only thing Inert asks for.").font(FontId::new(18.0, sans())).color(theme::MUTED));
                ui.add_space(36.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    let c = primary(ui, "SIGN IN WITH CLAUDE →", 250.0);
                    crate::drive::reg("signin:claude", c.rect);
                    if c.clicked() {
                        self.signin.provider = Some(Provider::Claude);
                    }
                    let g = ghost(ui, "SIGN IN WITH GPT →", 230.0);
                    crate::drive::reg("signin:gpt", g.rect);
                    if g.clicked() {
                        self.signin.provider = Some(Provider::Gpt);
                    }
                });
                ui.add_space(14.0);
                ui.label(theme::label_c("No account. No email. No tracking.", DIM));
            }
            Some(p) => {
                let (who, hint, console) = match p {
                    Provider::Claude => ("Anthropic", "sk-ant-…", "https://platform.claude.com/settings/keys"),
                    Provider::Gpt => ("OpenAI", "sk-…", "https://platform.openai.com/api-keys"),
                };
                ui.label(RichText::new(format!("Connect {} with your {who} API key.", p.name())).font(FontId::new(18.0, sans())).color(theme::MUTED));
                ui.add_space(22.0);
                ui.label(theme::label(&format!("{who} API key")));
                ui.add_space(6.0);
                let r = ui.add(egui::TextEdit::singleline(&mut self.signin.key).password(true).hint_text(hint).desired_width(480.0).font(FontId::new(15.0, mono())).margin(vec2(12.0, 12.0)));
                if let Some(e) = &self.signin.error {
                    ui.label(RichText::new(e).color(DANGER).font(FontId::new(12.0, mono())));
                }
                ui.add_space(6.0);
                ui.label(theme::muted("Stored in your macOS Keychain. Sent only to the provider you chose — never to sites."));
                ui.add_space(22.0);
                let mut finish = false;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    crate::drive::reg("signin:key", r.rect);
                    let cont = primary(ui, "CONTINUE →", 200.0);
                    crate::drive::reg("signin:continue", cont.rect);
                    let go = cont.clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                    if go {
                        let k = self.signin.key.trim().to_owned();
                        if k.len() < 20 {
                            self.signin.error = Some("That doesn't look like an API key.".into());
                        } else if let Err(e) = ai::save_key(p, &k) {
                            self.signin.error = Some(format!("Keychain error: {e}"));
                        } else {
                            finish = true;
                        }
                    }
                    let sk = ghost(ui, "SKIP FOR NOW", 170.0);
                    crate::drive::reg("signin:skip", sk.rect);
                    if sk.clicked() {
                        finish = true;
                    }
                    if ui.add(egui::Label::new(theme::label("← back")).sense(Sense::click())).clicked() {
                        self.signin.provider = None;
                        self.signin.error = None;
                    }
                });
                ui.add_space(12.0);
                if ui.add(egui::Label::new(theme::label_c(&format!("Get a key at {}", console.trim_start_matches("https://")), SECURE)).sense(Sense::click())).clicked() {
                    // Opens after sign-in completes, in a normal (unsecured) tab.
                    self.profile.session.push(crate::profile::SavedTab { url: console.into(), title: "API keys".into(), group: None, pinned: false });
                }
                if finish {
                    self.signin.key.clear();
                    self.profile.settings.provider = Some(p);
                    self.profile.settings.model = p.default_model().into();
                    self.profile.onboarded = true;
                    self.dirty = true;
                }
            }
        }
        // Footer, Stencil style.
        let f = rect.shrink2(vec2(72.0, 36.0));
        ui.painter().text(pos2(f.left(), f.bottom()), egui::Align2::LEFT_BOTTOM, "© 2026 INERT", FontId::new(10.5, mono()), DIM);
        ui.painter().text(pos2(f.right(), f.bottom()), egui::Align2::RIGHT_BOTTOM, "RUST · SERVO · TLS 1.3 + ML-KEM", FontId::new(10.5, mono()), DIM);
    }

    // ------------------------------------------------------------------ pages

    pub fn page(&mut self, ui: &mut Ui, p: Page) {
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, BG);
        if p == Page::NewTab {
            self.paint_planet(ui, rect, 0.85);
        }
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            let w = (ui.available_width() - 96.0).clamp(320.0, 1040.0);
            let side = ((ui.available_width() - w) / 2.0).max(32.0);
            ui.horizontal(|ui| {
                ui.add_space(side);
                ui.vertical(|ui| {
                    ui.set_width(w);
                    match p {
                        Page::NewTab => self.new_tab_page(ui, w),
                        Page::History => self.history_page(ui),
                        Page::Bookmarks => self.bookmarks_page(ui),
                        Page::Settings => self.settings_page(ui),
                        Page::Downloads => self.downloads_page(ui),
                    }
                    ui.add_space(80.0);
                });
            });
        });
    }

    fn new_tab_page(&mut self, ui: &mut Ui, w: f32) {
        ui.add_space(72.0);
        ui.label(theme::label("inert:// — new tab"));
        ui.add_space(12.0);
        ui.label(RichText::new("Where to?").font(FontId::new(64.0, serif())).color(WHITE).extra_letter_spacing(-2.0));
        ui.add_space(22.0);
        // Search box that hands off to the omnibox.
        let (r, resp) = ui.allocate_exact_size(vec2(w.min(640.0), 48.0), Sense::click());
        ui.painter().rect_filled(r, 4.0, RAISED);
        ui.painter().rect_stroke(r, 4.0, Stroke::new(1.0, if resp.hovered() { LINE_STRONG } else { LINE }), egui::StrokeKind::Inside);
        icons::paint(ui.painter(), Rect::from_center_size(pos2(r.left() + 22.0, r.center().y), Vec2::splat(16.0)), Icon::Search, MUTED);
        ui.painter().text(pos2(r.left() + 44.0, r.center().y), egui::Align2::LEFT_CENTER, format!("Search {} or type an address", self.profile.settings.search.name()), FontId::new(15.0, sans()), DIM);
        if resp.clicked() {
            self.omni.focus = true;
        }

        section(ui, "01", "Secured sites");
        let sites = self.sites.clone();
        if sites.is_empty() {
            ui.label(theme::muted("No inert:// sites yet. Start one with `cargo run -p inert-example-bank`."));
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
            for (host, name) in &sites {
                let (r, resp) = ui.allocate_exact_size(vec2(240.0, 104.0), Sense::click());
                let p = ui.painter();
                p.rect_filled(r, 4.0, if resp.hovered() { RAISED } else { Color32::from_black_alpha(160) });
                p.rect_stroke(r, 4.0, Stroke::new(1.0, if resp.hovered() { EDGE } else { LINE }), egui::StrokeKind::Inside);
                icons::paint(p, Rect::from_min_size(r.min + vec2(16.0, 16.0), Vec2::splat(14.0)), Icon::Lock, SECURE);
                p.text(r.min + vec2(38.0, 16.0), egui::Align2::LEFT_TOP, "SECURED", FontId::new(10.0, mono()), SECURE);
                p.text(r.min + vec2(16.0, 42.0), egui::Align2::LEFT_TOP, name, FontId::new(22.0, serif()), WHITE);
                p.text(r.min + vec2(16.0, 76.0), egui::Align2::LEFT_TOP, format!("inert://{host}"), FontId::new(11.0, mono()), MUTED);
                if resp.clicked() {
                    let id = self.active;
                    self.navigate(id, &format!("inert://{host}"));
                }
            }
        });

        section(ui, "02", "Tab groups");
        let groups = self.profile.groups.clone();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
            for g in groups.iter().filter(|g| g.kind != GroupKind::Normal) {
                let c = group_color(&g.color);
                let n = self.tabs.iter().filter(|t| t.group == Some(g.id)).count();
                let (r, resp) = ui.allocate_exact_size(vec2(240.0, 92.0), Sense::click());
                let p = ui.painter();
                p.rect_filled(r, 4.0, if resp.hovered() { c.gamma_multiply(0.1) } else { Color32::from_black_alpha(160) });
                p.rect_stroke(r, 4.0, Stroke::new(1.0, c.gamma_multiply(if resp.hovered() { 0.9 } else { 0.4 })), egui::StrokeKind::Inside);
                if let Some(ic) = group_icon(g.kind) {
                    icons::paint(p, Rect::from_min_size(r.min + vec2(16.0, 16.0), Vec2::splat(22.0)), ic, c);
                }
                p.text(r.min + vec2(16.0, 52.0), egui::Align2::LEFT_TOP, g.name.to_uppercase(), FontId::new(12.0, mono()), c);
                let sub = match g.kind {
                    GroupKind::Inbox => "mail · chat",
                    GroupKind::Homeserver => "self-hosted",
                    GroupKind::Dev => "dev servers · docs",
                    GroupKind::Normal => "",
                };
                p.text(r.min + vec2(16.0, 70.0), egui::Align2::LEFT_TOP, format!("{n} tabs · {sub}"), FontId::new(11.0, mono()), MUTED);
                if resp.clicked() {
                    if let Some(first) = self.tabs.iter().find(|t| t.group == Some(g.id)).map(|t| t.id) {
                        self.activate(first);
                    } else {
                        self.open_popover(crate::app::Popover::GroupAdd(g.id), r);
                    }
                }
            }
        });

        let mut top: Vec<_> = self.profile.history.iter().filter(|h| !h.url.starts_with("about:")).cloned().collect();
        top.sort_by(|a, b| b.visits.cmp(&a.visits));
        if !top.is_empty() {
            section(ui, "03", "Most visited");
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(8.0, 8.0);
                for h in top.iter().take(8) {
                    let label = if h.title.is_empty() { crate::omnibox::strip(&h.url).to_owned() } else { h.title.clone() };
                    let short: String = label.chars().take(26).collect();
                    let b = egui::Button::new(RichText::new(short).font(FontId::new(12.5, sans())).color(FG)).fill(RAISED).stroke(Stroke::new(1.0, LINE)).min_size(vec2(0.0, 34.0));
                    if ui.add(b).on_hover_text(&h.url).clicked() {
                        let id = self.active;
                        self.navigate(id, &h.url);
                    }
                }
            });
        }
    }

    fn history_page(&mut self, ui: &mut Ui) {
        ui.add_space(48.0);
        ui.label(theme::label("History"));
        ui.label(theme::serif_text("Where you've been.", 44.0));
        ui.add_space(12.0);
        let key = Id::unique("hist-filter");
        let mut q: String = ui.ctx().data(|d| d.get_temp(key)).unwrap_or_default();
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut q).hint_text("Search history").desired_width(360.0));
            if ui.button("Clear browsing history").clicked() {
                self.profile.history.clear();
                self.dirty = true;
            }
        });
        ui.ctx().data_mut(|d| d.insert_temp(key, q.clone()));
        ui.add_space(12.0);
        let mut items = self.profile.history.clone();
        items.sort_by(|a, b| b.last.cmp(&a.last));
        let ql = q.to_lowercase();
        let mut remove = None;
        let mut open = None;
        for h in items.iter().filter(|h| ql.is_empty() || h.url.to_lowercase().contains(&ql) || h.title.to_lowercase().contains(&ql)).take(300) {
            ui.horizontal(|ui| {
                let inert = h.url.starts_with("inert://");
                let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                icons::paint(ui.painter(), r, if inert { Icon::Lock } else { Icon::Globe }, if inert { SECURE } else { DIM });
                let t = if h.title.is_empty() { h.url.clone() } else { h.title.clone() };
                if ui.add(egui::Label::new(theme::body(&t)).sense(Sense::click()).truncate()).clicked() {
                    open = Some(h.url.clone());
                }
                ui.label(theme::mono_text(crate::omnibox::strip(&h.url), 11.0).color(DIM));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icons::button(ui, Icon::Close, 20.0, "Remove").clicked() {
                        remove = Some(h.url.clone());
                    }
                    ui.label(theme::mono_text(&format!("×{}", h.visits), 11.0).color(DIM));
                });
            });
        }
        if let Some(u) = remove {
            self.profile.history.retain(|h| h.url != u);
            self.dirty = true;
        }
        if let Some(u) = open {
            let id = self.active;
            self.navigate(id, &u);
        }
    }

    fn bookmarks_page(&mut self, ui: &mut Ui) {
        ui.add_space(48.0);
        ui.label(theme::label("Bookmarks"));
        ui.label(theme::serif_text("Saved for later.", 44.0));
        ui.add_space(16.0);
        let mut remove = None;
        let mut open = None;
        let n = self.profile.bookmarks.len();
        for i in 0..n {
            let b = self.profile.bookmarks[i].clone();
            ui.horizontal(|ui| {
                let inert = b.url.starts_with("inert://");
                let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                icons::paint(ui.painter(), r, if inert { Icon::Lock } else { Icon::Globe }, if inert { SECURE } else { DIM });
                let mut title = b.title.clone();
                if ui.add(egui::TextEdit::singleline(&mut title).desired_width(260.0).frame(egui::Frame::NONE)).changed() {
                    self.profile.bookmarks[i].title = title;
                    self.dirty = true;
                }
                if ui.add(egui::Label::new(theme::mono_text(&b.url, 11.5).color(SECURE)).sense(Sense::click())).clicked() {
                    open = Some(b.url.clone());
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icons::button(ui, Icon::Close, 20.0, "Delete").clicked() {
                        remove = Some(i);
                    }
                });
            });
            ui.separator();
        }
        if let Some(i) = remove {
            self.profile.bookmarks.remove(i);
            self.dirty = true;
        }
        if let Some(u) = open {
            let id = self.active;
            self.navigate(id, &u);
        }
    }

    fn downloads_page(&mut self, ui: &mut Ui) {
        ui.add_space(48.0);
        ui.label(theme::label("Downloads"));
        ui.label(theme::serif_text("Files from this session.", 44.0));
        ui.add_space(16.0);
        if self.downloads.is_empty() {
            ui.label(theme::muted(&format!("Nothing yet. Files are saved to {}.", crate::web::downloads_dir().display())));
        }
        for (url, path, _) in self.downloads.iter().rev() {
            ui.horizontal(|ui| {
                ui.label(theme::body(&path.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default()));
                ui.label(theme::mono_text(url, 11.0).color(DIM));
            });
        }
    }

    fn settings_page(&mut self, ui: &mut Ui) {
        ui.add_space(48.0);
        ui.label(theme::label("Settings"));
        ui.label(theme::serif_text("Make it yours.", 44.0));

        section(ui, "01", "AI provider");
        let current = self.profile.settings.provider;
        ui.horizontal(|ui| {
            for p in [Provider::Claude, Provider::Gpt] {
                let on = current == Some(p);
                let b = egui::Button::new(RichText::new(p.name().to_uppercase()).font(FontId::new(12.0, mono())).color(if on { BG } else { FG }).extra_letter_spacing(1.4)).fill(if on { FG } else { Color32::TRANSPARENT }).stroke(Stroke::new(1.0, LINE_STRONG)).min_size(vec2(120.0, 36.0));
                if ui.add(b).clicked() {
                    self.profile.settings.provider = Some(p);
                    self.profile.settings.model = p.default_model().into();
                    self.dirty = true;
                }
            }
        });
        if let Some(p) = current {
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                ui.label(theme::label("Model"));
                if ui.add(egui::TextEdit::singleline(&mut self.profile.settings.model).desired_width(240.0).font(FontId::new(13.0, mono()))).changed() {
                    self.dirty = true;
                }
            });
            let has = !self.demo && ai::has_key(p);
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(theme::label("API key"));
                ui.label(if has { theme::label_c("● stored in Keychain", OK) } else { theme::label_c("○ not set", WARN) });
            });
            let key = Id::unique("new-key");
            let mut k: String = ui.ctx().data(|d| d.get_temp(key)).unwrap_or_default();
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(&mut k).password(true).hint_text("Paste a new key").desired_width(320.0));
                if ui.button("Save key").clicked() && k.trim().len() >= 20 {
                    match ai::save_key(p, &k) {
                        Ok(()) => self.toast("Key saved to Keychain"),
                        Err(e) => self.toast(format!("Keychain error: {e}")),
                    }
                    k.clear();
                }
                if has && ui.button("Forget key").clicked() {
                    ai::forget_key(p);
                    self.toast("Key removed");
                }
            });
            ui.ctx().data_mut(|d| d.insert_temp(key, k));
        }

        section(ui, "02", "Search");
        ui.horizontal(|ui| {
            for e in SearchEngine::ALL {
                if ui.selectable_label(self.profile.settings.search == e, e.name()).clicked() {
                    self.profile.settings.search = e;
                    self.dirty = true;
                }
            }
        });
        if ui.checkbox(&mut self.profile.settings.remote_suggestions, "Show search suggestions from the search engine while typing").changed() {
            self.dirty = true;
        }
        ui.label(theme::muted("History, bookmarks, open tabs and inert:// sites always complete locally."));

        ui.add_space(6.0);
        ui.label(theme::label("Web engine for unsecured pages"));
        ui.horizontal(|ui| {
            for e in [crate::profile::Engine::Servo, crate::profile::Engine::WebKit] {
                let label = if e == crate::profile::Engine::Servo { "Servo (Rust)" } else { "WebKit (system)" };
                if ui.selectable_label(self.profile.settings.web_engine == e, label).clicked() {
                    self.profile.settings.web_engine = e;
                    self.dirty = true;
                }
            }
        });
        if ui.checkbox(&mut self.profile.settings.inbox_webkit, "Inbox tabs use WebKit (best compatibility for mail and chat apps)").changed() {
            self.dirty = true;
        }
        ui.label(theme::muted("Applies to tabs opened from now on. Any page can switch engines from its site security panel."));

        section(ui, "03", "Privacy & Shield");
        let mut on = self.profile.settings.adblock;
        if ui.checkbox(&mut on, "Block ads & trackers on web pages (EasyList + EasyPrivacy)").changed() {
            self.profile.settings.adblock = on;
            self.dirty = true;
            if on && !matches!(self.ad.state, adblock::State::Ready(_)) {
                self.ad.state = adblock::State::Preparing;
                adblock::prepare(self.tx.clone(), self.ctx.clone());
            }
        }
        let st = match &self.ad.state {
            adblock::State::Ready(n) => format!("Active: {n} rules compiled into WebKit."),
            adblock::State::Failed(e) => format!("Failed: {e}"),
            adblock::State::Off => "Off.".into(),
            _ => "Preparing filter lists…".into(),
        };
        ui.label(theme::muted(&st));
        ui.add_space(6.0);
        ui.label(theme::label("Pinned inert:// keys"));
        let pins: Vec<_> = self.profile.pins.iter().map(|(h, p)| (h.clone(), p.key_b64.clone(), p.max_version)).collect();
        let mut forget = None;
        for (h, k, v) in pins {
            ui.horizontal(|ui| {
                ui.label(theme::mono_text(&h, 12.0));
                ui.label(theme::mono_text(&format!("{}… v{v}", &k[..k.len().min(16)]), 11.0).color(DIM));
                if ui.small_button("Forget").clicked() {
                    forget = Some(h.clone());
                }
            });
        }
        if let Some(h) = forget {
            self.profile.pins.remove(&h);
            self.dirty = true;
        }

        section(ui, "04", "VPN");
        let s = self.vpn.status.clone();
        ui.label(theme::body(if s.installed { "Mullvad is installed. Open the VPN chip in the tab strip to connect or pick a location." } else { "Install Mullvad VPN to enable the built-in VPN." }));
        if s.installed {
            ui.label(theme::muted(&format!("Status: {}{}", s.state, if s.connected() { format!(" · {}, {}", s.city, s.country) } else { String::new() })));
        }

        section(ui, "05", "Performance");
        if ui.checkbox(&mut self.profile.settings.memory_saver, "Memory saver: unload web tabs unused for 15 minutes (Inbox tabs stay awake)").changed() {
            self.dirty = true;
        }
        if ui.checkbox(&mut self.profile.settings.bookmarks_bar, "Show bookmarks bar").changed() {
            self.dirty = true;
        }

        section(ui, "06", "About");
        ui.label(theme::body("Inert Browser 0.1 — Rust, egui + wgpu, WebKit for the unsecured web."));
        ui.label(theme::muted("inert:// protocol v1 · TLS 1.3 with X25519MLKEM768 · Ed25519-signed manifests."));
        ui.label(theme::muted("Typefaces: Geist Serif, Sans, Mono and Pixel by Vercel (SIL Open Font License)."));
        ui.add_space(8.0);
        if ui.button("Sign out of the AI provider").clicked() {
            if let Some(p) = self.profile.settings.provider {
                ai::forget_key(p);
            }
            self.profile.settings.provider = None;
            self.profile.onboarded = false;
            self.signin = Default::default();
            self.dirty = true;
        }
    }

    // ------------------------------------------------------------------ failures

    pub fn failure_page(&mut self, ui: &mut Ui, id: u64, host: &str, f: &Failure) {
        let rect = ui.max_rect();
        let danger = matches!(f, Failure::KeyChanged { .. } | Failure::Rollback { .. } | Failure::Protocol { .. });
        ui.painter().rect_filled(rect, 0.0, if danger { Color32::from_rgb(18, 4, 6) } else { BG });
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2((rect.width() - 720.0).max(64.0) / 2.0, 0.0))).layout(Layout::top_down(Align::Min)));
        let ui = &mut child;
        ui.add_space(80.0);
        let (tag, title, body): (&str, String, String) = match f {
            Failure::NoTrust => (
                "No trust record",
                format!("Inert doesn't know {host} yet."),
                "In this build, inert:// sites are discovered through local dev trust files written by `inert dev`. DNS discovery (_inert TXT records) comes next. Start the site, then reload.".into(),
            ),
            Failure::KeyChanged { pinned, offered } => (
                "Signing key changed",
                format!("{host} is presenting a different key."),
                format!("Pinned: {pinned}\nOffered: {offered}\n\nThis can mean the site rotated its key — or that someone is impersonating it. Inert will not show the page until you decide."),
            ),
            Failure::Rollback { got, seen } => ("Rollback refused", format!("{host} served an older manifest."), format!("You've already seen version {seen}; this connection offered version {got}. Old manifests can bring back fixed problems, so Inert refuses them.")),
            Failure::Protocol { code, detail } => (
                "Verification failed",
                format!("{host} sent something its signed manifest doesn't allow."),
                format!("{}\n\n{detail}\n\nInert never draws a page partially. Nothing from this response was shown.", code.map(|c| c.to_string()).unwrap_or_default()),
            ),
            Failure::Network(e) => ("Can't connect", format!("{host} isn't reachable."), format!("{e}\n\nIs the site running? For the demos: `cargo run -p inert-example-bank`.")),
        };
        ui.label(theme::label_c(tag, if danger { DANGER } else { WARN }));
        ui.add_space(10.0);
        ui.label(RichText::new(title).font(FontId::new(44.0, serif())).color(WHITE));
        ui.add_space(14.0);
        ui.label(RichText::new(body).font(FontId::new(13.5, mono())).color(FG));
        ui.add_space(28.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            if primary(ui, "TRY AGAIN", 160.0).clicked() {
                self.reload();
            }
            if ghost(ui, "BACK TO SAFETY", 190.0).clicked() {
                self.navigate(id, "about:newtab");
            }
            if let Failure::KeyChanged { .. } = f {
                if ui.add(egui::Button::new(RichText::new("TRUST THE NEW KEY").font(FontId::new(12.0, mono())).color(DANGER).extra_letter_spacing(1.4)).fill(Color32::TRANSPARENT).stroke(Stroke::new(1.0, DANGER)).min_size(vec2(200.0, 46.0))).on_hover_text("Re-pin to the key in the site's trust record").clicked() {
                    self.trust_new_key(id);
                }
            }
        });
    }
}
