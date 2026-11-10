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
    ui.add(theme::cta(text, BLUE, WHITE, 46.0).min_size(vec2(w, 46.0)))
}

fn ghost(ui: &mut Ui, text: &str, w: f32) -> egui::Response {
    ui.add(theme::cta_ghost(text, 46.0).min_size(vec2(w, 46.0)))
}

/// A rounded panel for page content.
fn card(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    egui::Frame::new().fill(PANEL).stroke(Stroke::new(1.0, LINE)).corner_radius(18).inner_margin(egui::Margin::symmetric(18, 14)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        add(ui);
    });
}

/// A tile on the new tab page: lifts and brightens under the pointer.
/// Returns its rect, response and hover amount.
fn tile(ui: &mut Ui, size: Vec2, edge: Color32) -> (Rect, egui::Response, f32) {
    let (r, resp) = ui.allocate_exact_size(size, Sense::click());
    let h = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.16);
    let r = r.translate(vec2(0.0, -2.0 * h));
    let p = ui.painter();
    if h > 0.0 {
        p.add(egui::epaint::Shadow { offset: [0, 8], blur: 22, spread: 0, color: Color32::from_black_alpha((90.0 * h) as u8) }.as_shape(r, 18));
    }
    p.rect_filled(r, 18.0, PANEL.lerp_to_gamma(RAISED, h));
    p.rect_stroke(r, 18.0, Stroke::new(1.0, LINE.lerp_to_gamma(edge, h)), egui::StrokeKind::Inside);
    (r, resp, h)
}

/// A history row: time, icon, title, host; hover shows a remove button.
/// Returns the row's response and whether remove was clicked.
fn row(ui: &mut Ui, time: &str, title: &str, url: &str, removable: bool) -> (egui::Response, bool) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
    let hov = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.1);
    let p = ui.painter();
    if hov > 0.0 {
        p.rect_filled(rect, 10.0, HOVER.gamma_multiply(hov));
    }
    let mut x = rect.left() + 10.0;
    if !time.is_empty() {
        p.text(pos2(x, rect.center().y), egui::Align2::LEFT_CENTER, time, FontId::new(11.5, mono()), DIM);
        x += 54.0;
    }
    let inert = url.starts_with("inert://");
    icons::paint(p, Rect::from_center_size(pos2(x + 7.0, rect.center().y), Vec2::splat(13.0)), if inert { Icon::Lock } else { Icon::Globe }, if inert { SECURE } else { DIM });
    x += 24.0;
    let host = crate::omnibox::strip(url).split('/').next().unwrap_or("").to_owned();
    let hg = ui.painter().layout_no_wrap(host, FontId::new(11.5, mono()), DIM);
    let mut job = egui::text::LayoutJob::single_section(title.to_owned(), egui::TextFormat::simple(FontId::new(13.5, sans()), FG));
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.right() - x - hg.size().x - 70.0).max(40.0));
    let g = ui.fonts_mut(|f| f.layout_job(job));
    let tw = g.size().x;
    ui.painter().galley(pos2(x, rect.center().y - g.size().y / 2.0), g, FG);
    ui.painter().galley(pos2(x + tw + 12.0, rect.center().y - hg.size().y / 2.0), hg, DIM);
    let mut removed = false;
    if removable && resp.hovered() || removable && ui.rect_contains_pointer(rect) {
        let cr = Rect::from_center_size(pos2(rect.right() - 18.0, rect.center().y), Vec2::splat(22.0));
        let c = ui.interact(cr, resp.id.with("x"), Sense::click()).on_hover_text("Remove from history");
        if c.hovered() {
            ui.painter().circle_filled(cr.center(), 11.0, LINE);
        }
        icons::paint(ui.painter(), cr.shrink(6.0), Icon::Close, if c.hovered() { WHITE } else { MUTED });
        removed = c.clicked();
    }
    (resp.on_hover_text(url), removed)
}

fn section(ui: &mut Ui, n: &str, title: &str) {
    ui.add_space(28.0);
    ui.label(theme::label(&format!("{n} — {title}")));
    ui.add_space(10.0);
}

impl Browser {
    // ------------------------------------------------------------------ sign in

    /// First run. The whole window is the glyph card; the steps that need
    /// typing sit in a dark panel on top of it.
    pub fn signin_screen(&mut self, ui: &mut Ui) {
        let rect = ui.max_rect();
        let ctx = ui.ctx().clone();
        let p = ui.painter().clone();
        p.rect_filled(rect, 0.0, BLUE);
        crate::art::glyphs(&p, &ctx, rect, ctx.pointer_latest_pos(), &crate::art::Glyphs::hero(ctx.input(|i| i.time) as f32));
        if ctx.input(|i| i.focused) {
            ctx.request_repaint_after(std::time::Duration::from_millis(170));
        }
        // Window drag on the top strip.
        let drag = ui.interact(Rect::from_min_size(rect.min, vec2(rect.width(), 52.0)), Id::unique("signin-drag"), Sense::drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        // Corner tags, as on the card this theme comes from.
        let m = 72.0;
        p.rect_filled(Rect::from_min_max(pos2(rect.left() + m - 16.0, rect.top() + 56.0), pos2(rect.left() + m + 150.0, rect.top() + 112.0)), 28.0, BLUE);
        crate::art::mark(&p, pos2(rect.left() + m + 15.0, rect.top() + 84.0), 15.0, WHITE);
        p.text(pos2(rect.left() + m + 42.0, rect.top() + 84.0), egui::Align2::LEFT_CENTER, "INERT", FontId::new(22.0, pixel()), WHITE);
        p.rect_filled(Rect::from_min_max(pos2(rect.right() - m - 80.0, rect.top() + 60.0), pos2(rect.right() - m + 16.0, rect.top() + 108.0)), 16.0, BLUE);
        p.text(pos2(rect.right() - m, rect.top() + 76.0), egui::Align2::RIGHT_CENTER, "INERT://", FontId::new(11.0, mono()), WHITE);
        p.text(pos2(rect.right() - m, rect.top() + 92.0), egui::Align2::RIGHT_CENTER, "2026", FontId::new(11.0, mono()), WHITE);
        let foot = p.layout_no_wrap("RUST · SERVO · TLS 1.3 + ML-KEM".to_owned(), FontId::new(10.5, mono()), WHITE);
        let fr = Rect::from_min_size(pos2(rect.right() - m - foot.size().x, rect.bottom() - 40.0 - foot.size().y), foot.size());
        p.rect_filled(fr.expand2(vec2(14.0, 9.0)), 14.0, BLUE);
        p.galley(fr.min, foot, WHITE);

        let col = Rect::from_min_max(pos2(rect.left() + m, rect.top() + 130.0), pos2(rect.right() - m, rect.bottom() - 40.0));
        // The map is cleared behind the headline so it reads cleanly.
        let clearing = p.add(egui::Shape::Noop);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(col).layout(Layout::top_down(Align::Min)));
        let ui = &mut child;
        if self.signin.import_step {
            ui.add_space((col.height() - 560.0).max(0.0) * 0.5);
            theme::panel_frame(26, 32.0).show(ui, |ui| {
                ui.set_width(600.0);
                self.import_screen(ui);
            });
            return;
        }
        match self.signin.provider {
            None => {
                ui.add_space((col.height() - 372.0).max(0.0));
                let top = ui.cursor().top();
                ui.label(RichText::new("The web,\nbehind glass.").font(FontId::new(88.0, sans())).color(WHITE).extra_letter_spacing(-2.6).line_height(Some(88.0)));
                ui.add_space(18.0);
                ui.label(RichText::new("Bring your own model. That is the only thing Inert asks for.").font(FontId::new(18.0, sans())).color(glass(0.9)));
                ui.add_space(30.0);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    let c = ui.add(theme::cta("Sign in with Claude", WHITE, BLUE_DEEP, 50.0).min_size(vec2(220.0, 50.0)));
                    crate::drive::reg("signin:claude", c.rect);
                    if c.clicked() {
                        self.signin.provider = Some(Provider::Claude);
                    }
                    let g = ui.add(theme::cta("Sign in with GPT", glass(0.10), WHITE, 50.0).stroke(Stroke::new(1.0, glass(0.55))).min_size(vec2(200.0, 50.0)));
                    crate::drive::reg("signin:gpt", g.rect);
                    if g.clicked() {
                        self.signin.provider = Some(Provider::Gpt);
                    }
                });
                ui.add_space(16.0);
                ui.label(theme::label_c("No account. No email. No tracking.", glass(0.85)));
                let block = Rect::from_min_max(pos2(col.left(), top), pos2(col.left() + 700.0, ui.cursor().top())).expand2(vec2(28.0, 22.0));
                p.set(clearing, egui::Shape::rect_filled(block, 30.0, BLUE));
            }
            Some(prov) => {
                let (who, hint, console) = match prov {
                    Provider::Claude => ("Anthropic", "sk-ant-…", "https://platform.claude.com/settings/keys"),
                    Provider::Gpt => ("OpenAI", "sk-…", "https://platform.openai.com/api-keys"),
                };
                ui.add_space((col.height() - 440.0).max(0.0) * 0.5);
                let mut finish = false;
                theme::panel_frame(26, 32.0).show(ui, |ui| {
                    ui.set_width(600.0);
                    ui.label(theme::label_c("Step 1 of 2 · Your model", ACCENT));
                    ui.add_space(10.0);
                    ui.label(theme::title(&format!("Connect {}.", prov.name()), 38.0));
                    ui.add_space(8.0);
                    ui.label(RichText::new(format!("Paste your {who} API key. It powers the assistant, with your own account.")).font(FontId::new(15.0, sans())).color(MUTED));
                    ui.add_space(20.0);
                    ui.label(theme::label(&format!("{who} API key")));
                    ui.add_space(6.0);
                    let r = ui.add(egui::TextEdit::singleline(&mut self.signin.key).password(true).hint_text(hint).desired_width(f32::INFINITY).font(FontId::new(15.0, mono())).margin(vec2(14.0, 12.0)));
                    crate::drive::reg("signin:key", r.rect);
                    if let Some(e) = &self.signin.error {
                        ui.label(RichText::new(e).color(DANGER).font(FontId::new(12.0, mono())));
                    }
                    ui.add_space(6.0);
                    ui.label(theme::muted("Stored in your macOS Keychain. Sent only to the provider you chose — never to sites."));
                    ui.add_space(22.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        let cont = primary(ui, "Continue", 170.0);
                        crate::drive::reg("signin:continue", cont.rect);
                        let go = cont.clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                        if go {
                            let k = self.signin.key.trim().to_owned();
                            if k.len() < 20 {
                                self.signin.error = Some("That doesn't look like an API key.".into());
                            } else if let Err(e) = ai::save_key(prov, &k) {
                                self.signin.error = Some(format!("Keychain error: {e}"));
                            } else {
                                finish = true;
                            }
                        }
                        let sk = ghost(ui, "Skip for now", 150.0);
                        crate::drive::reg("signin:skip", sk.rect);
                        if sk.clicked() {
                            finish = true;
                        }
                        if ui.add(egui::Label::new(theme::label("← back")).sense(Sense::click())).clicked() {
                            self.signin.provider = None;
                            self.signin.error = None;
                        }
                    });
                    ui.add_space(14.0);
                    if ui.add(egui::Label::new(theme::label_c(&format!("Get a key at {}", console.trim_start_matches("https://")), SECURE)).sense(Sense::click())).clicked() {
                        // Opens after sign-in completes, in a normal (unsecured) tab.
                        self.profile.session.push(crate::profile::SavedTab { url: console.into(), title: "API keys".into(), group: None, pinned: false });
                    }
                });
                if finish {
                    self.signin.key.clear();
                    self.profile.settings.provider = Some(prov);
                    self.profile.settings.model = prov.default_model().into();
                    // Step 2: bring Chrome along (skippable).
                    self.signin.import_step = true;
                    self.dirty = true;
                }
            }
        }
    }

    // ------------------------------------------------------------------ pages

    pub fn page(&mut self, ui: &mut Ui, p: Page) {
        let rect = ui.max_rect();
        ui.painter().rect_filled(rect, 0.0, BG);
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
                        Page::Import => self.import_page(ui),
                    }
                    ui.add_space(80.0);
                });
            });
        });
    }

    fn new_tab_page(&mut self, ui: &mut Ui, w: f32) {
        let ctx = ui.ctx().clone();
        ui.add_space(28.0);
        // The hero: the glyph card itself, alive under the cursor.
        let (hero, _) = ui.allocate_exact_size(vec2(w, 268.0), Sense::hover());
        let p = ui.painter().clone();
        p.rect_filled(hero, 0.0, BLUE);
        crate::art::glyphs(&p, &ctx, hero, ctx.pointer_latest_pos(), &crate::art::Glyphs::hero(ctx.input(|i| i.time) as f32));
        if ctx.input(|i| i.focused) {
            ctx.request_repaint_after(std::time::Duration::from_millis(170));
        }
        // The map is cleared behind the headline.
        let head = "The web,\nbehind glass.";
        let job = egui::text::LayoutJob::simple(head.to_owned(), FontId::new(34.0, sans()), WHITE, f32::INFINITY);
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        let at = pos2(hero.left() + 30.0, hero.bottom() - 62.0 - galley.size().y);
        p.rect_filled(Rect::from_min_size(at, galley.size()).expand2(vec2(18.0, 12.0)), 18.0, BLUE);
        p.galley(at, galley, WHITE);
        let tag = Rect::from_min_max(pos2(hero.right() - 120.0, hero.top() + 18.0), pos2(hero.right() - 14.0, hero.top() + 60.0));
        p.rect_filled(tag, 12.0, BLUE);
        p.text(pos2(hero.right() - 28.0, hero.top() + 32.0), egui::Align2::RIGHT_CENTER, "INERT://", FontId::new(11.0, mono()), WHITE);
        p.text(pos2(hero.right() - 28.0, hero.top() + 47.0), egui::Align2::RIGHT_CENTER, "NEW TAB", FontId::new(11.0, mono()), WHITE);
        let mk = pos2(hero.left() + 46.0, hero.top() + 44.0);
        p.circle_filled(mk, 26.0, BLUE);
        crate::art::mark(&p, mk, 15.0, WHITE);
        crate::chrome::round_corners(&p, hero, 26.0, BG);

        // The composer overlaps the hero's lower edge.
        ui.add_space(-36.0);
        let cw = (w - 60.0).min(700.0);
        ui.horizontal(|ui| {
            ui.add_space(((w - cw) / 2.0).max(0.0));
            self.ntp_search(ui, cw);
        });

        section(ui, "01", "Secured sites");
        let sites = self.sites.clone();
        if sites.is_empty() {
            ui.label(theme::muted("No inert:// sites yet. Start one with `cargo run -p inert-example-bank`."));
        }
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(12.0, 12.0);
            for (host, name) in &sites {
                let (r, resp, h) = tile(ui, vec2(240.0, 108.0), ACCENT);
                let p = ui.painter();
                let chip = Rect::from_min_size(r.min + vec2(16.0, 16.0), Vec2::splat(26.0));
                p.rect_filled(chip, 9.0, BLUE);
                icons::paint(p, chip.shrink(6.5), Icon::Lock, WHITE);
                p.text(pos2(chip.right() + 10.0, chip.center().y), egui::Align2::LEFT_CENTER, "SECURED", FontId::new(10.0, mono()), SECURE);
                icons::paint(p, Rect::from_center_size(pos2(r.right() - 22.0, chip.center().y), Vec2::splat(12.0)), Icon::Forward, MUTED.lerp_to_gamma(WHITE, h));
                p.text(r.min + vec2(16.0, 52.0), egui::Align2::LEFT_TOP, name, FontId::new(18.0, sans_medium()), WHITE);
                p.text(r.min + vec2(16.0, 80.0), egui::Align2::LEFT_TOP, format!("inert://{host}"), FontId::new(11.0, mono()), MUTED);
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
                let (r, resp, _) = tile(ui, vec2(240.0, 96.0), c);
                let p = ui.painter();
                let chip = Rect::from_min_size(r.min + vec2(16.0, 16.0), Vec2::splat(26.0));
                p.rect_filled(chip, 9.0, c);
                if let Some(ic) = group_icon(g.kind) {
                    icons::paint(p, chip.shrink(6.0), ic, BG);
                }
                p.text(r.min + vec2(16.0, 52.0), egui::Align2::LEFT_TOP, &g.name, FontId::new(15.0, sans_medium()), WHITE);
                let sub = match g.kind {
                    GroupKind::Inbox => "mail · chat",
                    GroupKind::Homeserver => "self-hosted",
                    GroupKind::Dev => "dev servers · docs",
                    GroupKind::Normal => "",
                };
                p.text(r.min + vec2(16.0, 74.0), egui::Align2::LEFT_TOP, format!("{n} {} · {sub}", if n == 1 { "tab" } else { "tabs" }), FontId::new(11.0, mono()), MUTED);
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
                    let b = egui::Button::new(RichText::new(short).font(FontId::new(12.5, sans())).color(FG)).fill(PANEL).stroke(Stroke::new(1.0, LINE)).corner_radius(17).min_size(vec2(0.0, 34.0));
                    if ui.add(b).on_hover_text(&h.url).clicked() {
                        let id = self.active;
                        self.navigate(id, &h.url);
                    }
                }
            });
        }
    }

    /// The new tab page's search box: a real field with its own suggestions
    /// (it no longer hands off to the address bar).
    fn ntp_search(&mut self, ui: &mut Ui, w: f32) {
        let id = Id::unique("ntp-search");
        let (r, bg) = ui.allocate_exact_size(vec2(w, 100.0), Sense::click());
        crate::drive::reg("ntp:search", r);
        let ctx = ui.ctx().clone();
        let focused = ui.memory(|m| m.has_focus(id));
        let f = ctx.animate_bool_with_time(Id::unique("ntp-focus"), focused, 0.18);
        let hov = ctx.animate_bool_with_time(Id::unique("ntp-hover"), ui.rect_contains_pointer(r), 0.14);
        // A composer: the field on top, a row of chips and the send button below.
        let p = ui.painter().clone();
        p.add(egui::epaint::Shadow { offset: [0, 14], blur: 36, spread: 0, color: Color32::from_black_alpha(130) }.as_shape(r, 24));
        if f > 0.0 {
            p.rect_stroke(r.expand(2.0), 26.0, Stroke::new(4.0, ACCENT.gamma_multiply(0.20 * f)), egui::StrokeKind::Outside);
        }
        p.rect_filled(r, 24.0, RAISED.lerp_to_gamma(HOVER, 0.35 * hov * (1.0 - f)));
        p.rect_stroke(r, 24.0, Stroke::new(1.0, glass(0.10 + 0.06 * hov).lerp_to_gamma(ACCENT, 0.8 * f)), egui::StrokeKind::Inside);
        icons::paint(&p, Rect::from_center_size(pos2(r.left() + 28.0, r.top() + 32.0), Vec2::splat(16.0)), Icon::Search, MUTED.lerp_to_gamma(WHITE, f));
        if bg.clicked() {
            ui.memory_mut(|m| m.request_focus(id));
        }
        // Chips: where the search goes, and the way to the address bar.
        let engine = self.profile.settings.search.name();
        let mut cx = r.left() + 16.0;
        for (txt, ink) in [(engine.to_owned(), FG), ("Cmd+L for the address bar".to_owned(), MUTED)] {
            let g = p.layout_no_wrap(txt, FontId::new(11.5, sans()), ink);
            let chip = Rect::from_min_size(pos2(cx, r.bottom() - 40.0), vec2(g.size().x + 22.0, 26.0));
            p.rect_stroke(chip, 13.0, Stroke::new(1.0, LINE_STRONG), egui::StrokeKind::Inside);
            p.galley(pos2(chip.left() + 11.0, chip.center().y - g.size().y / 2.0), g, ink);
            cx = chip.right() + 8.0;
        }
        // Send: lights up once there is something to send.
        let sr = Rect::from_center_size(pos2(r.right() - 30.0, r.bottom() - 27.0), Vec2::splat(32.0));
        let sresp = ui.interact(sr, Id::unique("ntp-send"), Sense::click()).on_hover_text("Go");
        let ready = ctx.animate_bool_with_time(Id::unique("ntp-ready"), !self.ntp.text.trim().is_empty(), 0.16);
        let sh = ctx.animate_bool_with_time(Id::unique("ntp-send-h"), sresp.hovered(), 0.12);
        p.circle_filled(sr.center(), 16.0 + sh, HOVER.lerp_to_gamma(BLUE, ready));
        icons::paint(&p, sr.shrink(9.0), Icon::ArrowUp, MUTED.lerp_to_gamma(WHITE, ready));
        let send = sresp.clicked() && !self.ntp.text.trim().is_empty();
        let font = FontId::new(17.0, sans());
        let row_h = ui.fonts_mut(|fo| fo.row_height(&font));
        let tr = Rect::from_min_max(pos2(r.left() + 52.0, r.top() + 32.0 - row_h / 2.0), pos2(r.right() - 22.0, r.top() + 32.0 + row_h / 2.0));
        let before = self.ntp.text.clone();
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(tr).layout(Layout::left_to_right(Align::Center)));
        let out = egui::TextEdit::singleline(&mut self.ntp.text)
            .id(id)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .font(font)
            .text_color(FG)
            .hint_text(RichText::new("Search or ask anything…").color(DIM))
            .desired_width(tr.width())
            .show(&mut child);
        let resp = out.response.clone();
        let has = ui.memory(|m| m.has_focus(id));
        if resp.gained_focus() {
            self.ntp.editing = true;
        }
        if has {
            let (down, up, tab) = ui.input_mut(|i| (i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown), i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp), i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)));
            let n = self.ntp.sugg.len() + self.ntp.remote.len();
            if down && n > 0 {
                self.ntp.sel = (self.ntp.sel + 1).min(n - 1);
            }
            if up {
                self.ntp.sel = self.ntp.sel.saturating_sub(1);
            }
            if tab {
                let len = self.ntp.text.chars().count();
                let mut st = out.state.clone();
                st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(len))));
                st.store(ui.ctx(), id);
            }
        }
        if self.ntp.text != before && has {
            self.ntp.editing = true;
            self.ntp.sel = 0;
            let cursor_end = out.cursor_range.map(|c| c.primary.index.0).unwrap_or(0) == self.ntp.text.chars().count();
            let grew = self.ntp.text.len() > before.len() && self.ntp.text.starts_with(before.as_str());
            let typed = self.ntp.text.clone();
            self.ntp.typed = typed.clone();
            if grew && cursor_end {
                if let Some(full) = crate::omnibox::inline_completion(&self.profile, &typed) {
                    let (tl, fl) = (typed.chars().count(), full.chars().count());
                    self.ntp.text = full;
                    let mut st = out.state.clone();
                    st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(fl), egui::text::CCursor::new(tl))));
                    st.store(ui.ctx(), id);
                }
            }
            let tabs: Vec<(u64, String, String)> = self.tabs.iter().filter(|t| t.id != self.active).map(|t| (t.id, t.title(), t.url())).collect();
            let ai = self.profile.settings.provider.map(|p| p.name());
            self.ntp.sugg = crate::omnibox::local(&self.profile, &tabs, &self.sites, &typed, ai);
            self.ntp.remote.clear();
            if self.profile.settings.remote_suggestions && typed.trim().len() >= 2 && !typed.contains("://") && !self.demo {
                crate::omnibox::remote(self.profile.settings.search, typed, self.tx.clone(), self.ctx.clone());
            }
        }
        let enter = send || (resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
        if enter {
            let rows = crate::chrome::rows_of(&self.ntp);
            let choice = if self.ntp.sel == 0 { None } else { rows.get(self.ntp.sel).cloned() };
            let text = self.ntp.text.clone();
            self.ntp = Default::default();
            match choice {
                Some(s) => self.accept_suggestion(s),
                None if !text.trim().is_empty() => self.open_url_from_omnibox(&text),
                None => {}
            }
        } else if resp.lost_focus() {
            let in_dd = ctx.data(|d| d.get_temp::<Rect>(Id::unique("ntp-dd"))).zip(ctx.pointer_latest_pos()).is_some_and(|(r, p)| r.contains(p));
            if !in_dd {
                self.ntp.editing = false;
            }
        }
        // egui drops focus on Escape before we see it, so check lost_focus too.
        if (has || resp.lost_focus()) && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.ntp = Default::default();
            ui.memory_mut(|m| m.surrender_focus(id));
        }
        ctx.data_mut(|d| d.insert_temp(Id::unique("ntp-rect"), r));
    }

    fn history_page(&mut self, ui: &mut Ui) {
        ui.add_space(48.0);
        ui.label(theme::label("History"));
        ui.label(theme::title("Where you've been.", 44.0));
        ui.add_space(16.0);
        let key = Id::unique("hist-filter");
        let mut q: String = ui.ctx().data(|d| d.get_temp(key)).unwrap_or_default();
        let clear_key = Id::unique("hist-clear-open");
        let mut clear_open: bool = ui.ctx().data(|d| d.get_temp(clear_key)).unwrap_or(false);
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut q).hint_text("Search history").desired_width(380.0).margin(vec2(14.0, 9.0)));
            crate::drive::reg("history:search", r.rect);
            let b = ui.add(egui::Button::new(RichText::new("Clear browsing data…").font(FontId::new(13.0, sans()))).corner_radius(16).min_size(vec2(0.0, 34.0)));
            crate::drive::reg("history:clear", b.rect);
            if b.clicked() {
                clear_open = !clear_open;
            }
        });
        ui.ctx().data_mut(|d| d.insert_temp(key, q.clone()));
        if clear_open {
            ui.add_space(12.0);
            clear_open = self.clear_data_card(ui);
        }
        ui.ctx().data_mut(|d| d.insert_temp(clear_key, clear_open));

        // Recently closed tabs, like Chrome's history page.
        if !self.closed.is_empty() && q.is_empty() {
            ui.add_space(18.0);
            let mut reopen = None;
            card(ui, |ui| {
                ui.label(theme::label("Recently closed"));
                ui.add_space(4.0);
                for (i, c) in self.closed.iter().enumerate().rev().take(6) {
                    if row(ui, "", &c.title, &c.url, false).0.clicked() {
                        reopen = Some(i);
                    }
                }
            });
            if let Some(i) = reopen {
                let c = self.closed.remove(i);
                let id = self.add_tab(&c.url, c.group, true, Some(&c.title));
                let _ = id;
            }
        }

        // The timeline, grouped by day.
        let ql = q.to_lowercase();
        let now = crate::profile::now();
        let mut open = None;
        let mut remove = None;
        let mut shown = 0;
        let mut day: Option<(i32, u32, u32)> = None;
        let visits: Vec<crate::profile::Visit> = self.profile.visits.iter().rev().filter(|v| ql.is_empty() || v.url.to_lowercase().contains(&ql) || v.title.to_lowercase().contains(&ql)).take(400).cloned().collect();
        if visits.is_empty() {
            ui.add_space(24.0);
            ui.label(theme::muted(if q.is_empty() { "Pages you visit show up here." } else { "No history matches that search." }));
        }
        let mut i = 0;
        while i < visits.len() {
            let k = crate::when::day_key(visits[i].t);
            let start = i;
            while i < visits.len() && crate::when::day_key(visits[i].t) == k {
                i += 1;
            }
            day = Some(k);
            ui.add_space(18.0);
            card(ui, |ui| {
                ui.label(theme::label(&crate::when::day_label(visits[start].t, now)));
                ui.add_space(4.0);
                for v in &visits[start..i] {
                    let title = if v.title.is_empty() { crate::omnibox::strip(&v.url).to_owned() } else { v.title.clone() };
                    let (resp, x) = row(ui, &crate::when::clock(v.t), &title, &v.url, true);
                    if shown == 0 {
                        crate::drive::reg("history:first", resp.rect);
                    }
                    shown += 1;
                    if x {
                        remove = Some(v.url.clone());
                    } else if resp.clicked() {
                        open = Some(v.url.clone());
                    }
                }
            });
        }
        let _ = day;
        if let Some(u) = remove {
            self.profile.forget_url(&u);
            self.dirty = true;
        }
        if let Some(u) = open {
            let id = self.active;
            self.navigate(id, &u);
        }
    }

    /// Chrome's "Clear browsing data": a time range and what to clear.
    /// Returns whether the card stays open.
    fn clear_data_card(&mut self, ui: &mut Ui) -> bool {
        let rk = Id::unique("clear-range");
        let mut range: usize = ui.ctx().data(|d| d.get_temp(rk)).unwrap_or(1);
        let ck = Id::unique("clear-cookies");
        let mut cookies: bool = ui.ctx().data(|d| d.get_temp(ck)).unwrap_or(false);
        let mut keep = true;
        card(ui, |ui| {
            ui.label(theme::label("Clear browsing data"));
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                for (i, name) in ["Last hour", "Last 24 hours", "Last 7 days", "All time"].iter().enumerate() {
                    let b = ui.add(egui::Button::new(RichText::new(*name).font(FontId::new(12.5, sans())).color(if range == i { BG } else { FG })).fill(if range == i { WHITE } else { RAISED }).corner_radius(15).min_size(vec2(0.0, 30.0)));
                    crate::drive::reg(format!("clear:{name}"), b.rect);
                    if b.clicked() {
                        range = i;
                    }
                }
            });
            ui.add_space(4.0);
            ui.label(theme::body("Browsing history and the address bar's suggestions from it."));
            ui.checkbox(&mut cookies, "Also cookies and site data (signs you out of most sites)");
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let b = ui.add(theme::cta("Clear data", BLUE, WHITE, 36.0).min_size(vec2(130.0, 36.0)));
                crate::drive::reg("clear:go", b.rect);
                if b.clicked() {
                    let now = crate::profile::now();
                    let since = match range {
                        0 => now.saturating_sub(3600),
                        1 => now.saturating_sub(86_400),
                        2 => now.saturating_sub(7 * 86_400),
                        _ => 0,
                    };
                    self.profile.clear_history_since(since);
                    if cookies {
                        self.clear_all_cookies();
                    }
                    self.dirty = true;
                    self.toast("Browsing data cleared");
                    keep = false;
                }
                if ui.button("Cancel").clicked() {
                    keep = false;
                }
            });
        });
        ui.ctx().data_mut(|d| {
            d.insert_temp(rk, range);
            d.insert_temp(ck, cookies);
        });
        keep
    }

    fn bookmarks_page(&mut self, ui: &mut Ui) {
        ui.add_space(48.0);
        ui.label(theme::label("Bookmarks"));
        ui.label(theme::title("Saved for later.", 44.0));
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
        ui.label(theme::title("Files from this session.", 44.0));
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
        ui.label(theme::title("Make it yours.", 44.0));

        section(ui, "01", "AI provider");
        let current = self.profile.settings.provider;
        ui.horizontal(|ui| {
            for p in [Provider::Claude, Provider::Gpt] {
                let on = current == Some(p);
                let b = if on { theme::cta(p.name(), BLUE, WHITE, 36.0) } else { theme::cta_ghost(p.name(), 36.0) }.min_size(vec2(120.0, 36.0));
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

        section(ui, "04", "Extensions");
        self.extensions_settings(ui);

        section(ui, "05", "Passwords & import");
        self.passwords_settings(ui);

        section(ui, "06", "VPN");
        let s = self.vpn.status.clone();
        ui.label(theme::body(if s.installed { "Mullvad is installed. Open the VPN pill at the foot of the sidebar to connect or pick a location." } else { "Install Mullvad VPN to enable the built-in VPN." }));
        if s.installed {
            ui.label(theme::muted(&format!("Status: {}{}", s.state, if s.connected() { format!(" · {}, {}", s.city, s.country) } else { String::new() })));
        }

        section(ui, "07", "Performance");
        if ui.checkbox(&mut self.profile.settings.memory_saver, "Memory saver: unload web tabs unused for 15 minutes (Inbox tabs stay awake)").changed() {
            self.dirty = true;
        }
        if ui.checkbox(&mut self.profile.settings.bookmarks_bar, "Show bookmarks bar").changed() {
            self.dirty = true;
        }
        if ui.checkbox(&mut self.profile.settings.sidebar, "Show the tab sidebar (Cmd+S)").changed() {
            self.dirty = true;
        }

        section(ui, "08", "About");
        ui.label(theme::body("Inert Browser 0.1 — Rust, egui + wgpu, Servo and WebKit for the unsecured web."));
        ui.label(theme::muted("inert:// protocol v1 · TLS 1.3 with X25519MLKEM768 · Ed25519-signed manifests."));
        ui.label(theme::muted("Typefaces: Geist Sans, Mono, Pixel and Serif by Vercel (SIL Open Font License)."));
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

    fn passwords_settings(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let b = ui.add(egui::Button::new(RichText::new("Import from Chrome…").font(FontId::new(13.0, sans()))).corner_radius(16).min_size(vec2(0.0, 32.0)));
            crate::drive::reg("settings:import", b.rect);
            if b.clicked() {
                self.add_tab("about:import", None, true, None);
            }
            if let Some(f) = &self.profile.imported_from {
                ui.label(theme::muted(&format!("Last import: {f}")));
            }
        });
        ui.add_space(8.0);
        let logins = self.profile.logins.clone();
        if logins.is_empty() {
            ui.label(theme::muted("No saved passwords. Import them from Chrome; they go into the macOS Keychain."));
            return;
        }
        let reveal_k = Id::unique("pw-reveal");
        let mut reveal: Option<(usize, String)> = ui.ctx().data(|d| d.get_temp(reveal_k));
        let mut forget = None;
        card(ui, |ui| {
            ui.label(theme::label(&format!("{} saved passwords", logins.len())));
            for (i, l) in logins.iter().enumerate() {
                ui.horizontal(|ui| {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                    icons::paint(ui.painter(), r, Icon::Key, MUTED);
                    ui.label(theme::mono_text(l.origin.trim_start_matches("https://"), 12.5));
                    ui.label(theme::body(&l.username));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("Delete").clicked() {
                            forget = Some(i);
                        }
                        let shown = reveal.as_ref().filter(|(j, _)| *j == i).map(|(_, p)| p.clone());
                        match shown {
                            Some(p) => {
                                if ui.small_button("Hide").clicked() {
                                    reveal = None;
                                }
                                ui.label(theme::mono_text(&p, 12.0));
                            }
                            None => {
                                if ui.small_button("Show").clicked() {
                                    reveal = crate::passwords::get(&l.origin, &l.username, self.demo).map(|p| (i, p));
                                }
                                ui.label(theme::mono_text("••••••••", 12.0).color(DIM));
                            }
                        }
                    });
                });
            }
        });
        ui.ctx().data_mut(|d| d.insert_temp(reveal_k, reveal));
        if let Some(i) = forget {
            let l = self.profile.logins.remove(i);
            crate::passwords::forget(&l.origin, &l.username, self.demo);
            ui.ctx().data_mut(|d| d.remove::<Option<(usize, String)>>(reveal_k));
            self.dirty = true;
        }
    }

    /// Built-in extensions: Shield and Cookies.
    fn extensions_settings(&mut self, ui: &mut Ui) {
        // Keep the cookie list fresh while the page is open.
        let k = Id::unique("cookies-all-at");
        let now = ui.input(|i| i.time);
        let last: f64 = ui.ctx().data(|d| d.get_temp(k)).unwrap_or(-100.0);
        if now - last > 5.0 {
            ui.ctx().data_mut(|d| d.insert_temp(k, now));
            self.cookies_refresh_all();
        }
        card(ui, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
                icons::paint(ui.painter(), r, Icon::Shield, OK);
                ui.label(theme::label_c("Shield", OK));
                ui.label(theme::muted("Ad & tracker blocking — EasyList + EasyPrivacy, both engines."));
            });
        });
        ui.add_space(10.0);
        card(ui, |ui| {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(18.0), Sense::hover());
                icons::paint(ui.painter(), r, Icon::Cookie, ACCENT);
                ui.label(theme::label_c("Cookies", ACCENT));
                ui.label(theme::muted("See, delete and refuse cookies; rejects cookie banners for you."));
            });
            ui.add_space(6.0);
            if ui.checkbox(&mut self.profile.settings.cookie_banners, "Reject cookie banners automatically (clicks “Reject all” / “Only necessary”)").changed() {
                self.dirty = true;
            }
            if !self.profile.settings.cookie_block.is_empty() {
                ui.add_space(4.0);
                ui.label(theme::label("Keep no cookies for"));
                let mut remove = None;
                for s in &self.profile.settings.cookie_block {
                    ui.horizontal(|ui| {
                        ui.label(theme::mono_text(s, 12.0));
                        if ui.small_button("Allow again").clicked() {
                            remove = Some(s.clone());
                        }
                    });
                }
                if let Some(s) = remove {
                    self.profile.settings.cookie_block.retain(|x| *x != s);
                    self.dirty = true;
                }
            }
            ui.add_space(8.0);
            // Sites with cookies (WebKit's store; values are never read).
            let mut sites: std::collections::BTreeMap<String, usize> = Default::default();
            for c in &self.cookie.all {
                *sites.entry(crate::cookies::site_of(&c.domain)).or_default() += 1;
            }
            ui.horizontal(|ui| {
                ui.label(theme::label(&format!("{} sites with cookies", sites.len())));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let b = ui.add(egui::Button::new(RichText::new("Delete all cookies").color(DANGER)).corner_radius(14));
                    crate::drive::reg("cookies:clear-all", b.rect);
                    if b.clicked() {
                        self.clear_all_cookies();
                        self.toast("Deleted all cookies and site data");
                    }
                });
            });
            let mut clear = None;
            egui::ScrollArea::vertical().max_height(240.0).id_salt("cookie-sites").show(ui, |ui| {
                for (site, n) in &sites {
                    ui.horizontal(|ui| {
                        ui.label(theme::mono_text(site, 12.5));
                        ui.label(theme::muted(&format!("{n} cookie{}", if *n == 1 { "" } else { "s" })));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.small_button("Delete").clicked() {
                                clear = Some(site.clone());
                            }
                        });
                    });
                }
            });
            if let Some(s) = clear {
                self.cookies_clear_site(&s);
            }
            ui.label(theme::muted("Servo tabs keep their own cookie jar; “Delete all” and per-site deletes clear both."));
        });
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
        ui.label(theme::title(&title, 40.0));
        ui.add_space(14.0);
        ui.label(RichText::new(body).font(FontId::new(13.5, mono())).color(FG));
        ui.add_space(28.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 12.0;
            if primary(ui, "Try again", 150.0).clicked() {
                self.reload();
            }
            if ghost(ui, "Back to safety", 170.0).clicked() {
                self.navigate(id, "about:newtab");
            }
            if let Failure::KeyChanged { .. } = f {
                if ui.add(theme::cta("Trust the new key", DANGER.gamma_multiply(0.14), DANGER, 46.0).min_size(vec2(190.0, 46.0))).on_hover_text("Re-pin to the key in the site's trust record").clicked() {
                    self.trust_new_key(id);
                }
            }
        });
    }
}
