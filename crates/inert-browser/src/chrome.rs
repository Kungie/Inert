//! Browser chrome: tab strip with groups, toolbar + omnibox, bookmarks bar,
//! popovers (site security, VPN, shield, menu), the AI side panel and the
//! content area.

use crate::app::{Browser, Kind, Page, Popover};
use crate::icons::{self, Icon};
use crate::inert_tab::Status;
use crate::omnibox::{self, Kind as SKind};
use crate::profile::{GroupKind, presets};
use crate::theme::{self, *};
use crate::{adblock, vpn};
use eframe::egui::{self, Align, Color32, FontId, Id, Layout, Rect, RichText, Sense, Stroke, Ui, Vec2, pos2, vec2};
use std::time::{Duration, Instant};

const STRIP_H: f32 = 40.0;
const BAR_H: f32 = 46.0;
const MARKS_H: f32 = 30.0;
const LEFT_INSET: f32 = 80.0; // macOS traffic lights

pub fn group_icon(k: GroupKind) -> Option<Icon> {
    match k {
        GroupKind::Inbox => Some(Icon::Inbox),
        GroupKind::Homeserver => Some(Icon::Server),
        GroupKind::Dev => Some(Icon::Code),
        GroupKind::Normal => None,
    }
}

enum Security {
    Secured { dev: bool },
    Failed,
    Connecting,
    Unsecured,
    Unencrypted,
    Internal,
}

impl Browser {
    fn security(&self) -> Security {
        match self.active_tab().map(|t| &t.kind) {
            Some(Kind::Inert(it)) => match it.status {
                Status::Ready => Security::Secured { dev: it.dev },
                Status::Connecting => Security::Connecting,
                Status::Failed => Security::Failed,
            },
            Some(Kind::Web(w)) => {
                if w.url.starts_with("https://") { Security::Unsecured } else { Security::Unencrypted }
            }
            _ => Security::Internal,
        }
    }

    pub fn open_popover(&mut self, p: Popover, anchor: Rect) {
        let recently_closed = self.ctx.data(|d| d.get_temp::<Instant>(Id::unique("pop_closed_at"))).is_some_and(|t| t.elapsed() < Duration::from_millis(250));
        if self.popover == p || recently_closed {
            self.popover = Popover::None;
            return;
        }
        self.popover = p;
        self.popover_opened = Some(Instant::now());
        self.ctx.data_mut(|d| d.insert_temp(Id::unique("pop_anchor"), anchor));
    }

    pub fn chrome(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        egui::Panel::top("strip").exact_size(STRIP_H).frame(egui::Frame::new().fill(BG)).show(ui, |ui| self.tab_strip(ui));
        egui::Panel::top("toolbar").exact_size(BAR_H).frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::symmetric(10, 0))).show(ui, |ui| self.toolbar(ui));
        if self.profile.settings.bookmarks_bar {
            egui::Panel::top("marks").exact_size(MARKS_H).frame(egui::Frame::new().fill(PANEL).inner_margin(egui::Margin::symmetric(12, 0))).show(ui, |ui| self.bookmarks_bar(ui));
        }
        // Hairline under the chrome.
        if self.find.is_some() {
            egui::Panel::top("find").exact_size(38.0).frame(egui::Frame::new().fill(RAISED).inner_margin(egui::Margin::symmetric(12, 0))).show(ui, |ui| self.find_bar(ui));
        }
        if self.ai.open {
            egui::Panel::right("ai").exact_size(380.0).frame(egui::Frame::new().fill(PANEL).stroke(Stroke::new(1.0, LINE)).inner_margin(16.0)).show(ui, |ui| self.ai_panel(ui));
        }
        if self.inspector {
            egui::Panel::right("inspector").exact_size(340.0).frame(egui::Frame::new().fill(PANEL).stroke(Stroke::new(1.0, LINE)).inner_margin(16.0)).show(ui, |ui| self.inspector_panel(ui));
        }
        egui::CentralPanel::no_frame().show(ui, |ui| {
            let r = ui.max_rect();
            ui.painter().line_segment([r.left_top(), r.right_top()], Stroke::new(1.0, LINE));
            self.content_rect = Rect::from_min_max(pos2(r.left(), r.top() + 1.0), r.max);
            self.content(ui);
        });
        self.popovers(&ctx);
        self.omnibox_dropdown(&ctx);
        self.rename_modal(&ctx);
        self.toast_overlay(&ctx);
    }

    // ------------------------------------------------------------------ tab strip

    fn tab_strip(&mut self, ui: &mut Ui) {
        let full = ui.max_rect();
        ui.painter().line_segment([full.left_bottom(), full.right_bottom()], Stroke::new(1.0, LINE));
        // Window drag on the empty strip.
        let drag = ui.interact(full, Id::unique("strip-drag"), Sense::click_and_drag());
        if drag.drag_started() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if drag.double_clicked() {
            let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }
        // Right side: VPN chip + assistant.
        let right = Rect::from_min_max(pos2(full.right() - 205.0, full.top() + 6.0), pos2(full.right() - 10.0, full.bottom()));
        let mut r = ui.new_child(egui::UiBuilder::new().max_rect(right).layout(Layout::right_to_left(Align::Center)));
        let ai_color = if self.ai.open { ACCENT } else { FG };
        if icons::button_c(&mut r, Icon::Spark, 28.0, "Assistant  ⌘J", ai_color, true).clicked() {
            self.ai.open = !self.ai.open;
        }
        let st = &self.vpn.status;
        let cc = self.vpn.relays.iter().find(|c| c.name == st.country).map(|c| c.code.to_uppercase());
        let txt = match (st.connected(), cc) {
            (true, Some(cc)) => format!("VPN · {cc}"),
            _ => st.short(),
        };
        let col = if st.connected() { OK } else { MUTED };
        let chip = egui::Button::new(RichText::new(format!("● {txt}")).font(FontId::new(10.5, mono())).color(col).extra_letter_spacing(1.2)).fill(Color32::TRANSPARENT).stroke(Stroke::new(1.0, if st.connected() { OK.gamma_multiply(0.6) } else { LINE_STRONG })).corner_radius(2).min_size(vec2(0.0, 24.0));
        let resp = r.add(chip).on_hover_text("Mullvad VPN");
        if resp.clicked() {
            self.open_popover(Popover::Vpn, resp.rect);
        }

        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(Rect::from_min_max(pos2(full.left() + LEFT_INSET, full.top() + 6.0), pos2(full.right() - 210.0, full.bottom()))).layout(Layout::left_to_right(Align::Center)));
        let ui = &mut child;
        let clip = ui.max_rect();
        ui.set_clip_rect(clip);
        ui.spacing_mut().item_spacing.x = 4.0;

        let order = self.visible_order();
        let n_visible = order
            .iter()
            .filter(|id| {
                let t = self.tab(**id).unwrap();
                t.pinned || t.group.and_then(|g| self.profile.groups.iter().find(|x| x.id == g)).is_none_or(|g| !g.collapsed)
            })
            .count()
            .max(1);
        let chip_space: f32 = self.profile.groups.iter().map(|g| g.name.len() as f32 * 7.5 + 46.0).sum();
        let tab_w = ((ui.available_width() - chip_space - 40.0) / n_visible as f32 - 4.0).clamp(30.0, 210.0);

        // Pinned tabs.
        let pinned: Vec<u64> = self.tabs.iter().filter(|t| t.pinned).map(|t| t.id).collect();
        for id in pinned {
            self.tab_item(ui, id, 40.0, None);
        }
        // Groups.
        let groups = self.profile.groups.clone();
        for g in &groups {
            let ids: Vec<u64> = self.tabs.iter().filter(|t| !t.pinned && t.group == Some(g.id)).map(|t| t.id).collect();
            let color = group_color(&g.color);
            self.group_chip(ui, g, ids.len(), color);
            if !g.collapsed {
                for id in ids {
                    self.tab_item(ui, id, tab_w, Some(color));
                }
            }
        }
        // Ungrouped.
        let rest: Vec<u64> = self.tabs.iter().filter(|t| !t.pinned && t.group.is_none_or(|g| !groups.iter().any(|x| x.id == g))).map(|t| t.id).collect();
        for id in rest {
            self.tab_item(ui, id, tab_w, None);
        }
        if icons::button(ui, Icon::Plus, 26.0, "New tab  ⌘T").clicked() {
            self.add_tab("about:newtab", None, true, None);
        }

    }

    fn group_chip(&mut self, ui: &mut Ui, g: &crate::profile::GroupDef, count: usize, color: Color32) {
        let icon = group_icon(g.kind);
        let label = g.name.to_uppercase();
        let galley = ui.painter().layout_no_wrap(label.clone(), FontId::new(10.5, mono()), color);
        let w = galley.size().x + if icon.is_some() { 30.0 } else { 16.0 } + if count > 0 { 16.0 } else { 0.0 };
        let (rect, resp) = ui.allocate_exact_size(vec2(w, 24.0), Sense::click());
        let p = ui.painter();
        let fill = if resp.hovered() { color.gamma_multiply(0.18) } else { color.gamma_multiply(0.1) };
        p.rect_filled(rect, 3.0, fill);
        p.rect_stroke(rect, 3.0, Stroke::new(1.0, color.gamma_multiply(0.55)), egui::StrokeKind::Inside);
        let mut x = rect.left() + 8.0;
        if let Some(ic) = icon {
            icons::paint(p, Rect::from_center_size(pos2(x + 6.0, rect.center().y), Vec2::splat(13.0)), ic, color);
            x += 16.0;
        }
        p.galley(pos2(x, rect.center().y - galley.size().y / 2.0), galley.clone(), color);
        if count > 0 {
            p.text(pos2(rect.right() - 9.0, rect.center().y), egui::Align2::CENTER_CENTER, count.to_string(), FontId::new(10.0, mono()), color.gamma_multiply(0.8));
        }
        let gid = g.id;
        let resp = resp.on_hover_text(if g.collapsed { "Expand group" } else { "Collapse group" });
        if resp.clicked() {
            if count == 0 {
                self.open_popover(Popover::GroupAdd(gid), resp.rect);
            } else if let Some(gd) = self.profile.groups.iter_mut().find(|x| x.id == gid) {
                gd.collapsed = !gd.collapsed;
                self.dirty = true;
            }
        }
        let kind = g.kind;
        resp.context_menu(|ui| {
            ui.set_min_width(220.0);
            if ui.button("New tab in group").clicked() {
                self.add_tab("about:newtab", Some(gid), true, None);
                ui.close();
            }
            if kind != GroupKind::Normal || !presets(kind).is_empty() {
                ui.menu_button("Add app…", |ui| {
                    for (name, url) in presets(kind) {
                        if ui.button(*name).clicked() {
                            self.add_tab(url, Some(gid), true, Some(name));
                            ui.close();
                        }
                    }
                });
            }
            if ui.button("Rename group").clicked() {
                let name = self.profile.groups.iter().find(|x| x.id == gid).map(|x| x.name.clone()).unwrap_or_default();
                self.rename = Some((gid, name));
                ui.close();
            }
            ui.menu_button("Color", |ui| {
                for (name, c) in theme::GROUP_COLORS {
                    if ui.button(RichText::new(format!("● {name}")).color(c)).clicked() {
                        if let Some(gd) = self.profile.groups.iter_mut().find(|x| x.id == gid) {
                            gd.color = name.into();
                            self.dirty = true;
                        }
                        ui.close();
                    }
                }
            });
            ui.separator();
            if ui.button("Close all tabs in group").clicked() {
                let ids: Vec<u64> = self.tabs.iter().filter(|t| t.group == Some(gid)).map(|t| t.id).collect();
                for id in ids {
                    self.close_tab(id);
                }
                ui.close();
            }
            if kind == GroupKind::Normal && ui.button("Ungroup").clicked() {
                for t in &mut self.tabs {
                    if t.group == Some(gid) {
                        t.group = None;
                    }
                }
                self.profile.groups.retain(|x| x.id != gid);
                self.dirty = true;
                ui.close();
            }
        });
    }

    fn tab_item(&mut self, ui: &mut Ui, id: u64, w: f32, group: Option<Color32>) {
        let Some(t) = self.tab(id) else { return };
        let active = id == self.active;
        let title = t.title();
        let pinned = t.pinned;
        let loading = t.loading();
        let (icon, icon_c) = match &t.kind {
            Kind::Inert(it) if it.status == Status::Failed => (Icon::Unlock, DANGER),
            Kind::Inert(_) => (Icon::Lock, SECURE),
            Kind::Web(_) => (Icon::Globe, MUTED),
            Kind::Page(Page::Settings) => (Icon::Gear, MUTED),
            Kind::Page(Page::History) => (Icon::History, MUTED),
            Kind::Page(Page::Bookmarks) => (Icon::Bookmark, MUTED),
            Kind::Page(_) => (Icon::Spark, MUTED),
        };
        let (rect, resp) = ui.allocate_exact_size(vec2(w, 30.0), Sense::click());
        let p = ui.painter().clone();
        if active {
            p.rect_filled(rect, egui::CornerRadius { nw: 4, ne: 4, sw: 0, se: 0 }, PANEL);
            p.line_segment([rect.left_top() + vec2(4.0, 0.0), rect.right_top() - vec2(4.0, 0.0)], Stroke::new(1.0, LINE_STRONG));
        } else if resp.hovered() {
            p.rect_filled(rect.shrink(1.0), 4.0, RAISED);
        }
        if let Some(gc) = group {
            p.line_segment([rect.left_bottom() + vec2(6.0, -1.0), rect.right_bottom() + vec2(-6.0, -1.0)], Stroke::new(2.0, gc));
        }
        let ic_x = if w < 64.0 { rect.center().x } else { rect.left() + 16.0 };
        let ic_rect = Rect::from_center_size(pos2(ic_x, rect.center().y), Vec2::splat(14.0));
        if loading {
            let tm = ui.input(|i| i.time) as f32;
            let a = tm * 6.0;
            let c = ic_rect.center();
            p.circle_stroke(c, 5.0, Stroke::new(1.0, LINE_STRONG));
            p.line_segment([c, c + vec2(a.cos(), a.sin()) * 5.0], Stroke::new(1.5, ACCENT));
        } else {
            icons::paint(&p, ic_rect, icon, icon_c);
        }
        let compact = w < 64.0;
        if !pinned && !compact {
            let show_close = active || resp.hovered();
            let text_right = rect.right() - if show_close { 26.0 } else { 8.0 };
            let avail = (text_right - (rect.left() + 30.0)).max(0.0);
            let color = if active { WHITE } else { MUTED };
            let mut job = egui::text::LayoutJob::single_section(title.clone(), egui::TextFormat::simple(FontId::new(12.5, sans()), color));
            job.wrap = egui::text::TextWrapping::truncate_at_width(avail);
            let galley = ui.fonts_mut(|f| f.layout_job(job));
            p.galley(pos2(rect.left() + 30.0, rect.center().y - galley.size().y / 2.0), galley, color);
            if show_close {
                let cr = Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), Vec2::splat(18.0));
                let cresp = ui.interact(cr, Id::unique(("close", id)), Sense::click());
                if cresp.hovered() {
                    p.rect_filled(cr, 3.0, HOVER);
                }
                icons::paint(&p, cr.shrink(5.0), Icon::Close, if cresp.hovered() { WHITE } else { MUTED });
                if cresp.clicked() {
                    self.close_tab(id);
                    return;
                }
            }
        }
        let resp = resp.on_hover_text(&title);
        if resp.clicked() {
            self.activate(id);
        }
        if resp.middle_clicked() {
            self.close_tab(id);
            return;
        }
        let groups = self.profile.groups.clone();
        resp.context_menu(|ui| {
            ui.set_min_width(230.0);
            if ui.button("New tab to the right").clicked() {
                self.active = id;
                let g = self.tab(id).and_then(|t| t.group);
                self.add_tab("about:newtab", g, true, None);
                ui.close();
            }
            ui.menu_button("Add tab to group", |ui| {
                if ui.button("New group").clicked() {
                    self.group_new(id, "New group");
                    ui.close();
                }
                ui.separator();
                for g in &groups {
                    let c = group_color(&g.color);
                    if ui.button(RichText::new(format!("● {}", g.name)).color(c)).clicked() {
                        if let Some(t) = self.tab_mut(id) {
                            t.group = Some(g.id);
                        }
                        self.dirty = true;
                        ui.close();
                    }
                }
            });
            if self.tab(id).is_some_and(|t| t.group.is_some()) && ui.button("Remove from group").clicked() {
                if let Some(t) = self.tab_mut(id) {
                    t.group = None;
                }
                self.dirty = true;
                ui.close();
            }
            ui.separator();
            if ui.button(if pinned { "Unpin" } else { "Pin" }).clicked() {
                if let Some(t) = self.tab_mut(id) {
                    t.pinned = !t.pinned;
                }
                self.dirty = true;
                ui.close();
            }
            if ui.button("Duplicate").clicked() {
                let (u, g) = self.tab(id).map(|t| (t.url(), t.group)).unwrap_or_default();
                self.add_tab(&u, g, true, None);
                ui.close();
            }
            if ui.button("Bookmark tab").clicked() {
                let (u, ti) = self.tab(id).map(|t| (t.url(), t.title())).unwrap_or_default();
                self.profile.toggle_bookmark(&u, &ti);
                self.dirty = true;
                ui.close();
            }
            ui.separator();
            if ui.button("Close").clicked() {
                self.close_tab(id);
                ui.close();
            }
            if ui.button("Close other tabs").clicked() {
                let others: Vec<u64> = self.tabs.iter().filter(|t| t.id != id && !t.pinned).map(|t| t.id).collect();
                for o in others {
                    self.close_tab(o);
                }
                ui.close();
            }
        });
    }

    // ------------------------------------------------------------------ toolbar

    fn toolbar(&mut self, ui: &mut Ui) {
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if icons::button_c(ui, Icon::Back, 30.0, "Back  ⌘[", FG, self.can_go(true)).clicked() {
                self.go_back();
            }
            if icons::button_c(ui, Icon::Forward, 30.0, "Forward  ⌘]", FG, self.can_go(false)).clicked() {
                self.go_forward();
            }
            let loading = self.active_tab().is_some_and(|t| t.loading());
            if loading {
                if icons::button(ui, Icon::Stop, 30.0, "Stop").clicked() {
                    self.stop();
                }
            } else if icons::button(ui, Icon::Reload, 30.0, "Reload  ⌘R").clicked() {
                self.reload();
            }
            ui.add_space(6.0);
            let right_w = 4.0 * 32.0 + 8.0;
            let w = (ui.available_width() - right_w).max(200.0);
            self.omnibox(ui, w);
            ui.add_space(6.0);
            let url = self.active_tab().map(|t| t.url()).unwrap_or_default();
            let marked = self.profile.is_bookmarked(&url);
            if icons::button_c(ui, if marked { Icon::StarFilled } else { Icon::Star }, 30.0, "Bookmark this tab  ⌘D", if marked { ACCENT } else { FG }, !url.starts_with("about:")).clicked() {
                let title = self.active_tab().map(|t| t.title()).unwrap_or_default();
                self.profile.toggle_bookmark(&url, &title);
                self.dirty = true;
            }
            let (shield_c, tip) = match (&self.ad.state, self.profile.settings.adblock) {
                (_, false) => (MUTED, "Shield is off".to_owned()),
                (adblock::State::Ready(n), _) => (OK, format!("Shield on — {n} ad & tracker rules")),
                (adblock::State::Failed(e), _) => (DANGER, format!("Shield failed: {e}")),
                _ => (WARN, "Shield is preparing filter lists…".to_owned()),
            };
            let r = icons::button_c(ui, Icon::Shield, 30.0, &tip, shield_c, true);
            if r.clicked() {
                self.open_popover(Popover::Shield, r.rect);
            }
            let r = icons::button(ui, Icon::Menu, 30.0, "Menu");
            if r.clicked() {
                self.open_popover(Popover::Menu, r.rect);
            }
        });
    }

    fn omnibox(&mut self, ui: &mut Ui, width: f32) {
        let id = Id::unique("omnibox");
        let (rect, _) = ui.allocate_exact_size(vec2(width, 32.0), Sense::hover());
        let focused = ui.memory(|m| m.has_focus(id));
        let p = ui.painter();
        p.rect_filled(rect, 4.0, if focused { BG } else { RAISED });
        p.rect_stroke(rect, 4.0, Stroke::new(1.0, if focused { LINE_STRONG } else { LINE }), egui::StrokeKind::Inside);

        // Security badge.
        let (label, color, icon) = match self.security() {
            Security::Secured { dev: true } => ("SECURED · DEV", SECURE, Icon::Lock),
            Security::Secured { dev: false } => ("SECURED", SECURE, Icon::Lock),
            Security::Connecting => ("VERIFYING", MUTED, Icon::Lock),
            Security::Failed => ("BLOCKED", DANGER, Icon::Unlock),
            Security::Unsecured => ("UNSECURED", WARN, Icon::Unlock),
            Security::Unencrypted => ("NOT ENCRYPTED", DANGER, Icon::Unlock),
            Security::Internal => ("INERT", MUTED, Icon::Spark),
        };
        let galley = ui.painter().layout_no_wrap(label.to_owned(), FontId::new(10.0, mono()), color);
        let bw = galley.size().x + 30.0;
        let badge = Rect::from_min_size(rect.min + vec2(4.0, 4.0), vec2(bw, rect.height() - 8.0));
        let bresp = ui.interact(badge, Id::unique("sec-badge"), Sense::click()).on_hover_text("Site security");
        ui.painter().rect_filled(badge, 3.0, if bresp.hovered() { HOVER } else { color.gamma_multiply(0.1) });
        icons::paint(ui.painter(), Rect::from_center_size(pos2(badge.left() + 12.0, badge.center().y), Vec2::splat(12.0)), icon, color);
        ui.painter().galley(pos2(badge.left() + 22.0, badge.center().y - galley.size().y / 2.0), galley, color);
        if bresp.clicked() {
            self.open_popover(Popover::Security, bresp.rect);
        }

        // Text field.
        let text_rect = Rect::from_min_max(pos2(badge.right() + 8.0, rect.top()), pos2(rect.right() - 8.0, rect.bottom()));
        if self.omni.focus {
            ui.memory_mut(|m| m.request_focus(id));
            self.omni.focus = false;
            self.omni.select_all = true;
        }
        let before = self.omni.text.clone();
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(text_rect).layout(Layout::left_to_right(Align::Center)));
        let out = egui::TextEdit::singleline(&mut self.omni.text)
            .id(id)
            .frame(egui::Frame::NONE)
            .font(FontId::new(13.5, sans()))
            .text_color(FG)
            .hint_text(RichText::new("Search or enter an address — inert:// sites are verified").color(DIM))
            .desired_width(text_rect.width())
            .show(&mut child);
        let resp = out.response.clone();
        if resp.gained_focus() {
            self.omni.editing = true;
            self.omni.select_all = true;
        }
        if self.omni.select_all && resp.has_focus() {
            let mut st = out.state.clone();
            st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(self.omni.text.chars().count()))));
            st.store(ui.ctx(), id);
            self.omni.select_all = false;
        }
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if resp.has_focus() {
            let (down, up, tab) = ui.input_mut(|i| (i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown), i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp), i.consume_key(egui::Modifiers::NONE, egui::Key::Tab)));
            let n = self.omni.sugg.len() + self.omni.remote.len();
            if down && n > 0 {
                self.omni.sel = (self.omni.sel + 1).min(n - 1);
            }
            if up {
                self.omni.sel = self.omni.sel.saturating_sub(1);
            }
            if tab {
                // Accept the inline completion.
                let len = self.omni.text.chars().count();
                let mut st = out.state.clone();
                st.cursor.set_char_range(Some(egui::text::CCursorRange::one(egui::text::CCursor::new(len))));
                st.store(ui.ctx(), id);
            }
        }
        if self.omni.text != before && resp.has_focus() {
            self.on_omni_typed(ui, id, &before, &out);
        }
        if escape && self.omni.editing {
            self.omni.editing = false;
            self.omni.sugg.clear();
            self.omni.text = self.active_tab().map(|t| t.url()).unwrap_or_default();
            ui.memory_mut(|m| m.surrender_focus(id));
        }
        if enter {
            let choice = self.selected_suggestion();
            self.omni.editing = false;
            self.omni.sugg.clear();
            self.omni.remote.clear();
            match choice {
                Some(s) => self.accept_suggestion(s),
                None => {
                    let t = self.omni.text.clone();
                    if !t.trim().is_empty() {
                        self.open_url_from_omnibox(&t);
                    }
                }
            }
        } else if resp.lost_focus() && !ui.ctx().memory(|m| m.has_focus(id)) {
            // Delay clearing so clicks on the dropdown still land.
            let pointer_in_dropdown = ui.ctx().data(|d| d.get_temp::<Rect>(Id::unique("omni-dd"))).zip(ui.ctx().pointer_latest_pos()).is_some_and(|(r, p)| r.contains(p));
            if !pointer_in_dropdown {
                self.omni.editing = false;
                self.omni.sugg.clear();
            }
        }
        ui.ctx().data_mut(|d| d.insert_temp(Id::unique("omni-rect"), rect));
    }

    fn on_omni_typed(&mut self, ui: &mut Ui, id: Id, before: &str, out: &egui::text_edit::TextEditOutput) {
        self.omni.editing = true;
        self.omni.sel = 0;
        // Only complete when the user added characters at the end.
        let cursor_end = out.cursor_range.map(|c| c.primary.index.0).unwrap_or(0) == self.omni.text.chars().count();
        let grew = self.omni.text.len() > before.len() && self.omni.text.starts_with(before.get(..before.len().min(self.omni.text.len())).unwrap_or(""));
        let typed = self.omni.text.clone();
        self.omni.typed = typed.clone();
        if grew && cursor_end {
            if let Some(full) = omnibox::inline_completion(&self.profile, &typed) {
                let tl = typed.chars().count();
                let fl = full.chars().count();
                self.omni.text = full;
                let mut st = out.state.clone();
                st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(fl), egui::text::CCursor::new(tl))));
                st.store(ui.ctx(), id);
            }
        }
        let tabs: Vec<(u64, String, String)> = self.tabs.iter().filter(|t| t.id != self.active).map(|t| (t.id, t.title(), t.url())).collect();
        let ai = self.profile.settings.provider.map(|p| p.name());
        self.omni.sugg = omnibox::local(&self.profile, &tabs, &self.sites, &typed, ai);
        self.omni.remote.clear();
        if self.profile.settings.remote_suggestions && typed.trim().len() >= 2 && !typed.contains("://") && !self.demo {
            omnibox::remote(self.profile.settings.search, typed, self.tx.clone(), self.ctx.clone());
        }
    }

    fn selected_suggestion(&self) -> Option<omnibox::Suggestion> {
        if self.omni.sel == 0 {
            return None; // Enter on the first row = what's in the box.
        }
        let i = self.omni.sel;
        if i < self.omni.sugg.len() {
            return self.omni.sugg.get(i).cloned();
        }
        self.omni.remote.get(i - self.omni.sugg.len()).map(|q| omnibox::Suggestion { kind: SKind::Remote, title: q.clone(), target: q.clone() })
    }

    fn accept_suggestion(&mut self, s: omnibox::Suggestion) {
        match s.kind {
            SKind::OpenTab => {
                if let Ok(id) = s.target.parse::<u64>() {
                    self.activate(id);
                }
            }
            SKind::Search | SKind::Remote => {
                let u = self.profile.settings.search.search_url(&s.target);
                let id = self.active;
                self.navigate(id, &u);
            }
            SKind::Ai => self.ask_ai(s.target),
            _ => {
                let id = self.active;
                self.navigate(id, &s.target);
            }
        }
    }

    fn omnibox_dropdown(&mut self, ctx: &egui::Context) {
        if !self.omni.editing || (self.omni.sugg.is_empty() && self.omni.remote.is_empty()) {
            ctx.data_mut(|d| d.remove::<Rect>(Id::unique("omni-dd")));
            return;
        }
        let Some(r) = ctx.data(|d| d.get_temp::<Rect>(Id::unique("omni-rect"))) else { return };
        let mut rows: Vec<omnibox::Suggestion> = self.omni.sugg.clone();
        rows.extend(self.omni.remote.iter().map(|q| omnibox::Suggestion { kind: SKind::Remote, title: q.clone(), target: q.clone() }));
        let mut clicked = None;
        let area = egui::Area::new(Id::unique("omni-dropdown")).fixed_pos(pos2(r.left(), r.bottom() + 4.0)).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::new().fill(PANEL).stroke(Stroke::new(1.0, LINE_STRONG)).corner_radius(4).inner_margin(6.0).show(ui, |ui| {
                ui.set_width(r.width() - 12.0);
                ui.spacing_mut().item_spacing.y = 0.0;
                for (i, s) in rows.iter().enumerate() {
                    let sel = i == self.omni.sel;
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
                    if sel || resp.hovered() {
                        ui.painter().rect_filled(rect, 3.0, if sel { HOVER } else { RAISED });
                    }
                    let (icon, c) = match s.kind {
                        SKind::Url => (Icon::Globe, FG),
                        SKind::Search | SKind::Remote => (Icon::Search, MUTED),
                        SKind::History => (Icon::History, MUTED),
                        SKind::Bookmark => (Icon::Star, ACCENT),
                        SKind::OpenTab => (Icon::Tab, EDGE),
                        SKind::Inert => (Icon::Lock, SECURE),
                        SKind::Ai => (Icon::Spark, ACCENT),
                    };
                    icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.left() + 16.0, rect.center().y), Vec2::splat(14.0)), icon, c);
                    let tag = match s.kind {
                        SKind::Url => "",
                        SKind::Search => "SEARCH",
                        SKind::Remote => "SUGGESTED",
                        SKind::History => "HISTORY",
                        SKind::Bookmark => "BOOKMARK",
                        SKind::OpenTab => "OPEN TAB",
                        SKind::Inert => "SECURED",
                        SKind::Ai => "ASSISTANT",
                    };
                    let mut job = egui::text::LayoutJob::single_section(s.title.clone(), egui::TextFormat::simple(FontId::new(13.0, sans()), FG));
                    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - 160.0);
                    let g = ui.fonts_mut(|f| f.layout_job(job));
                    ui.painter().galley(pos2(rect.left() + 34.0, rect.center().y - g.size().y / 2.0), g, FG);
                    if matches!(s.kind, SKind::History | SKind::Bookmark | SKind::Inert) {
                        let short = omnibox::strip(&s.target);
                        let gw = ui.painter().layout_no_wrap(short.to_owned(), FontId::new(11.0, mono()), DIM);
                        let _ = gw;
                    }
                    ui.painter().text(pos2(rect.right() - 10.0, rect.center().y), egui::Align2::RIGHT_CENTER, tag, FontId::new(9.5, mono()), DIM);
                    if resp.clicked() {
                        clicked = Some(s.clone());
                    }
                }
            });
        });
        ctx.data_mut(|d| d.insert_temp(Id::unique("omni-dd"), area.response.rect));
        self.overlay_rects.push(area.response.rect);
        if let Some(s) = clicked {
            self.omni.editing = false;
            self.omni.sugg.clear();
            self.omni.remote.clear();
            ctx.memory_mut(|m| m.surrender_focus(Id::unique("omnibox")));
            self.accept_suggestion(s);
        }
    }

    // ------------------------------------------------------------------ bookmarks bar

    fn bookmarks_bar(&mut self, ui: &mut Ui) {
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let marks = self.profile.bookmarks.clone();
            for b in marks.iter().take(14) {
                let inert = b.url.starts_with("inert://");
                let icon_c = if inert { SECURE } else { DIM };
                let (rect, resp) = {
                    let g = ui.painter().layout_no_wrap(b.title.clone(), FontId::new(12.0, sans()), MUTED);
                    let w = g.size().x.min(150.0) + 30.0;
                    ui.allocate_exact_size(vec2(w, 22.0), Sense::click())
                };
                if resp.hovered() {
                    ui.painter().rect_filled(rect, 3.0, HOVER);
                }
                icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), Vec2::splat(11.0)), if inert { Icon::Lock } else { Icon::Globe }, icon_c);
                let mut job = egui::text::LayoutJob::single_section(b.title.clone(), egui::TextFormat::simple(FontId::new(12.0, sans()), if resp.hovered() { FG } else { MUTED }));
                job.wrap = egui::text::TextWrapping::truncate_at_width(150.0);
                let g = ui.fonts_mut(|f| f.layout_job(job));
                ui.painter().galley(pos2(rect.left() + 22.0, rect.center().y - g.size().y / 2.0), g, MUTED);
                let resp = resp.on_hover_text(&b.url);
                if resp.clicked() {
                    let cmd = ui.input(|i| i.modifiers.command);
                    if cmd {
                        self.add_tab(&b.url, None, false, Some(&b.title));
                    } else {
                        let id = self.active;
                        self.navigate(id, &b.url);
                    }
                }
                if resp.middle_clicked() {
                    self.add_tab(&b.url, None, false, Some(&b.title));
                }
                let url = b.url.clone();
                resp.context_menu(|ui| {
                    if ui.button("Open in new tab").clicked() {
                        self.add_tab(&url, None, true, None);
                        ui.close();
                    }
                    if ui.button("Delete bookmark").clicked() {
                        self.profile.bookmarks.retain(|x| x.url != url);
                        self.dirty = true;
                        ui.close();
                    }
                });
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.add(egui::Label::new(theme::label("All bookmarks")).sense(Sense::click())).clicked() {
                    self.add_tab("about:bookmarks", None, true, None);
                }
            });
        });
    }

    fn find_bar(&mut self, ui: &mut Ui) {
        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.label(theme::label("Find in page"));
            let mut q = self.find.clone().unwrap_or_default();
            let r = ui.add(egui::TextEdit::singleline(&mut q).id(Id::unique("find")).desired_width(280.0).font(FontId::new(13.0, sans())));
            if ui.memory(|m| m.focused().is_none()) {
                r.request_focus();
            }
            let go = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) || ui.button("Next").clicked();
            if go && !q.is_empty() {
                if let Some(Kind::Web(w)) = self.active_tab().map(|t| &t.kind) {
                    let lit = serde_json::to_string(&q).unwrap_or_default();
                    crate::web::js(w, &format!("window.find({lit}, false, false, true)"));
                }
                r.request_focus();
            }
            if let Some(Kind::Inert(it)) = self.active_tab().map(|t| &t.kind) {
                let n = if q.is_empty() { 0 } else { it.semantic_text().to_lowercase().matches(&q.to_lowercase()).count() };
                ui.label(theme::muted(&format!("{n} matches on this screen")));
            }
            self.find = Some(q);
            if ui.button("Done").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.find = None;
            }
        });
    }

    // ------------------------------------------------------------------ popovers

    fn popovers(&mut self, ctx: &egui::Context) {
        if self.popover == Popover::None {
            return;
        }
        let anchor = ctx.data(|d| d.get_temp::<Rect>(Id::unique("pop_anchor"))).unwrap_or(Rect::from_min_size(pos2(100.0, 80.0), Vec2::ZERO));
        let width = match self.popover {
            Popover::Vpn => 340.0,
            Popover::Menu => 240.0,
            _ => 360.0,
        };
        let screen = ctx.content_rect();
        let x = (anchor.right() - width).clamp(8.0, screen.right() - width - 8.0);
        let x = if matches!(self.popover, Popover::Security | Popover::GroupAdd(_)) { anchor.left().min(screen.right() - width - 8.0) } else { x };
        let pos = pos2(x, anchor.bottom() + 6.0);
        let which = self.popover;
        let area = egui::Area::new(Id::unique("popover")).fixed_pos(pos).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::new().fill(PANEL).stroke(Stroke::new(1.0, LINE_STRONG)).corner_radius(4).inner_margin(16.0).show(ui, |ui| {
                ui.set_width(width - 32.0);
                match which {
                    Popover::Security => self.pop_security(ui),
                    Popover::Vpn => self.pop_vpn(ui),
                    Popover::Shield => self.pop_shield(ui),
                    Popover::Menu => self.pop_menu(ui),
                    Popover::GroupAdd(g) => self.pop_group_add(ui, g),
                    Popover::None => {}
                }
            });
        });
        self.overlay_rects.push(area.response.rect);
        let outside = ctx.input(|i| i.pointer.any_pressed()) && ctx.pointer_latest_pos().is_some_and(|p| !area.response.rect.contains(p));
        let fresh = self.popover_opened.is_some_and(|t| t.elapsed() < Duration::from_millis(150));
        let in_menu = egui::Popup::is_any_open(ctx);
        if (outside && !fresh && !in_menu) || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.popover = Popover::None;
            ctx.data_mut(|d| d.insert_temp(Id::unique("pop_closed_at"), Instant::now()));
        }
    }

    fn pop_security(&mut self, ui: &mut Ui) {
        let host_pin = self.active_tab().and_then(|t| match &t.kind {
            Kind::Inert(it) => Some((it.host.clone(), it.key_id.clone(), it.manifest.as_ref().map(|m| (m.version, m.screens.len(), m.actions.len())), it.status, it.dev)),
            _ => None,
        });
        match host_pin {
            Some((host, key, m, status, dev)) => {
                if status == Status::Ready {
                    ui.label(theme::label_c("● Connection is secured", SECURE));
                    ui.add_space(6.0);
                    ui.label(theme::serif_text(&host, 24.0));
                    ui.add_space(8.0);
                    for line in [
                        "The manifest is signed and matches the pinned key.",
                        "Every screen was checked against the signed template.",
                        "No code from this site runs on your device.",
                        "TLS 1.3 with post-quantum hybrid key exchange.",
                    ] {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("✓").color(OK));
                            ui.label(theme::body(line));
                        });
                    }
                    ui.add_space(8.0);
                    ui.separator();
                    egui::Grid::new("secgrid").num_columns(2).spacing([16.0, 6.0]).show(ui, |ui| {
                        ui.label(theme::label("Key"));
                        ui.label(theme::mono_text(&key, 12.0));
                        ui.end_row();
                        if let Some((v, s, a)) = m {
                            ui.label(theme::label("Manifest"));
                            ui.label(theme::mono_text(&format!("v{v} · {s} screens · {a} actions"), 12.0));
                            ui.end_row();
                        }
                        ui.label(theme::label("Trust"));
                        ui.label(theme::mono_text(if dev { "dev trust file (local)" } else { "DNS + pin" }, 12.0));
                        ui.end_row();
                    });
                    ui.add_space(6.0);
                    if ui.button("Inspect manifest").clicked() {
                        self.inspector = true;
                        self.popover = Popover::None;
                    }
                } else {
                    ui.label(theme::label_c("● Not verified", DANGER));
                    ui.label(theme::body(&format!("Inert refused to show {host} because it did not pass verification.")));
                }
            }
            None => match self.security() {
                Security::Internal => {
                    ui.label(theme::label("Inert page"));
                    ui.label(theme::body("Built into the browser."));
                }
                Security::Unencrypted => {
                    ui.label(theme::label_c("○ Not encrypted", DANGER));
                    ui.add_space(4.0);
                    ui.label(theme::body("This page is plain HTTP: anyone on the network can read or change it, and it runs code from the site."));
                }
                _ => {
                    ui.label(theme::label_c("○ Unsecured", WARN));
                    ui.add_space(6.0);
                    ui.label(theme::serif_text("Ordinary web page", 22.0));
                    ui.add_space(6.0);
                    ui.label(theme::body("The connection uses HTTPS, but the page runs the site's own code inside WebKit's sandbox. Inert cannot verify what it shows."));
                    ui.add_space(6.0);
                    let shield = match &self.ad.state {
                        adblock::State::Ready(n) if self.profile.settings.adblock => format!("Shield is blocking ads & trackers ({n} rules)."),
                        _ => "Shield is not active on this page.".into(),
                    };
                    ui.label(theme::muted(&shield));
                    ui.label(theme::muted("The assistant treats this page's text as untrusted."));
                }
            },
        }
    }

    fn pop_vpn(&mut self, ui: &mut Ui) {
        let st = self.vpn.status.clone();
        ui.horizontal(|ui| {
            icons::paint(ui.painter(), Rect::from_min_size(ui.cursor().min, Vec2::splat(16.0)), Icon::Vpn, if st.connected() { OK } else { MUTED });
            ui.add_space(20.0);
            ui.label(theme::label("Native VPN · Mullvad"));
        });
        ui.add_space(8.0);
        if !st.installed {
            ui.label(theme::serif_text("Mullvad isn't installed", 22.0));
            ui.label(theme::body("Inert's VPN uses the Mullvad app's WireGuard tunnel. Install it from mullvad.net, sign in there once, then come back."));
            if ui.button("Open mullvad.net").clicked() {
                self.add_tab("https://mullvad.net/download", None, true, None);
                self.popover = Popover::None;
            }
            return;
        }
        let headline = if st.connected() { format!("{}, {}", st.city, st.country) } else if st.busy() { "Connecting…".into() } else { "Not connected".into() };
        ui.label(theme::serif_text(&headline, 26.0));
        ui.add_space(4.0);
        let ip = if self.vpn.reveal_ip || self.demo { st.ip.clone() } else { "•••.•••.•••.•••".into() };
        ui.horizontal(|ui| {
            ui.label(theme::label(if st.connected() { "Exit IP" } else { "Your IP" }));
            if ui.add(egui::Label::new(theme::mono_text(&ip, 12.0)).sense(Sense::click())).on_hover_text("Click to reveal").clicked() {
                self.vpn.reveal_ip = !self.vpn.reveal_ip;
            }
        });
        if st.connected() && !st.hostname.is_empty() {
            ui.horizontal(|ui| {
                ui.label(theme::label("Server"));
                ui.label(theme::mono_text(&st.hostname, 12.0));
            });
        }
        ui.add_space(10.0);
        let (txt, fill, tc) = if st.connected() { ("DISCONNECT", Color32::TRANSPARENT, FG) } else { ("CONNECT →", OK, BG) };
        let b = egui::Button::new(RichText::new(txt).font(FontId::new(12.0, mono())).color(tc).extra_letter_spacing(1.4)).fill(fill).stroke(Stroke::new(1.0, if st.connected() { LINE_STRONG } else { OK })).min_size(vec2(ui.available_width(), 40.0));
        if ui.add_enabled(!st.busy(), b).clicked() {
            let cmd = if st.connected() { vpn::Cmd::Disconnect } else { vpn::Cmd::Connect };
            self.vpn.status.state = if st.connected() { "disconnecting".into() } else { "connecting".into() };
            vpn::exec(cmd, self.demo, self.tx.clone(), self.ctx.clone());
        }
        ui.add_space(10.0);
        ui.label(theme::label("Location"));
        ui.add(egui::TextEdit::singleline(&mut self.vpn.filter).hint_text("Filter countries…").desired_width(ui.available_width()));
        let filter = self.vpn.filter.to_lowercase();
        let relays = self.vpn.relays.clone();
        egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
            for c in relays.iter().filter(|c| filter.is_empty() || c.name.to_lowercase().contains(&filter)) {
                ui.horizontal(|ui| {
                    let here = st.connected() && st.country == c.name;
                    if ui.add(egui::Button::new(RichText::new(&c.name).color(if here { OK } else { FG })).frame(false)).clicked() {
                        self.profile.settings.vpn_last_location = Some(c.code.clone());
                        self.vpn.status.state = "connecting".into();
                        vpn::exec(vpn::Cmd::Location(c.code.clone(), None), self.demo, self.tx.clone(), self.ctx.clone());
                    }
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        for (city, code) in c.cities.iter().rev().take(3) {
                            if ui.add(egui::Button::new(RichText::new(city).font(FontId::new(11.0, mono())).color(MUTED)).frame(false)).clicked() {
                                self.vpn.status.state = "connecting".into();
                                vpn::exec(vpn::Cmd::Location(c.code.clone(), Some(code.clone())), self.demo, self.tx.clone(), self.ctx.clone());
                            }
                        }
                    });
                });
            }
        });
        ui.add_space(6.0);
        if !st.expiry.is_empty() {
            ui.label(theme::muted(&format!("Mullvad account active until {}", st.expiry.split(' ').next().unwrap_or(""))));
        }
        ui.label(theme::muted("Tunnels all traffic on this Mac through Mullvad (WireGuard)."));
    }

    fn pop_shield(&mut self, ui: &mut Ui) {
        ui.label(theme::label("Shield · ad & tracker blocking"));
        ui.add_space(6.0);
        let (h, d) = match (&self.ad.state, self.profile.settings.adblock) {
            (_, false) => ("Shield is off".to_owned(), "Ads and trackers load normally on web pages.".to_owned()),
            (adblock::State::Ready(n), _) => ("Shield is on".to_owned(), format!("{n} EasyList + EasyPrivacy rules, compiled into WebKit's native content blocker.")),
            (adblock::State::Compiling(n), _) => ("Compiling…".to_owned(), format!("Handing {n} rules to WebKit.")),
            (adblock::State::Failed(e), _) => ("Shield failed".to_owned(), e.clone()),
            _ => ("Preparing…".to_owned(), "Fetching and converting filter lists.".to_owned()),
        };
        ui.label(theme::serif_text(&h, 24.0));
        ui.label(theme::body(&d));
        ui.add_space(6.0);
        ui.label(theme::muted("inert:// sites never need it — they can't load third-party code."));
        ui.add_space(8.0);
        let mut on = self.profile.settings.adblock;
        if ui.checkbox(&mut on, "Block ads & trackers on web pages").changed() {
            self.profile.settings.adblock = on;
            self.dirty = true;
            if on && !matches!(self.ad.state, adblock::State::Ready(_)) {
                self.ad.state = adblock::State::Preparing;
                adblock::prepare(self.tx.clone(), self.ctx.clone());
            }
        }
    }

    fn pop_menu(&mut self, ui: &mut Ui) {
        ui.spacing_mut().item_spacing.y = 2.0;
        let mut item = |ui: &mut Ui, icon: Icon, text: &str, key: &str| -> bool {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(rect, 3.0, HOVER);
            }
            icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), Vec2::splat(13.0)), icon, MUTED);
            ui.painter().text(pos2(rect.left() + 32.0, rect.center().y), egui::Align2::LEFT_CENTER, text, FontId::new(13.0, sans()), FG);
            ui.painter().text(pos2(rect.right() - 8.0, rect.center().y), egui::Align2::RIGHT_CENTER, key, FontId::new(11.0, mono()), DIM);
            resp.clicked()
        };
        if item(ui, Icon::Tab, "New tab", "⌘T") {
            self.add_tab("about:newtab", None, true, None);
            self.popover = Popover::None;
        }
        if item(ui, Icon::Layers, "New tab group", "") {
            let id = self.add_tab("about:newtab", None, true, None);
            self.group_new(id, "New group");
            self.popover = Popover::None;
        }
        if item(ui, Icon::History, "History", "⌘Y") {
            self.add_tab("about:history", None, true, None);
            self.popover = Popover::None;
        }
        if item(ui, Icon::Bookmark, "Bookmarks", "⌥⌘B") {
            self.add_tab("about:bookmarks", None, true, None);
            self.popover = Popover::None;
        }
        if item(ui, Icon::Folder, "Downloads", "") {
            self.add_tab("about:downloads", None, true, None);
            self.popover = Popover::None;
        }
        if item(ui, Icon::Spark, "Assistant", "⌘J") {
            self.ai.open = !self.ai.open;
            self.popover = Popover::None;
        }
        if item(ui, Icon::Search, "Find in page", "⌘F") {
            self.find = Some(String::new());
            self.popover = Popover::None;
        }
        if item(ui, Icon::Code, "Developer tools", "⌥⌘I") {
            self.devtools();
            self.popover = Popover::None;
        }
        if item(ui, Icon::Gear, "Settings", "⌘,") {
            self.add_tab("about:settings", None, true, None);
            self.popover = Popover::None;
        }
    }

    pub fn devtools(&mut self) {
        match self.active_tab().map(|t| &t.kind) {
            Some(Kind::Web(w)) => {
                if let Some(v) = &w.view {
                    v.open_devtools();
                }
            }
            Some(Kind::Inert(_)) => self.inspector = !self.inspector,
            _ => {}
        }
    }

    fn pop_group_add(&mut self, ui: &mut Ui, gid: u64) {
        let Some(g) = self.profile.groups.iter().find(|x| x.id == gid).cloned() else { return };
        ui.horizontal(|ui| {
            if let Some(ic) = group_icon(g.kind) {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                icons::paint(ui.painter(), r, ic, group_color(&g.color));
            }
            ui.label(theme::label_c(&g.name, group_color(&g.color)));
        });
        ui.add_space(4.0);
        ui.label(theme::body(match g.kind {
            GroupKind::Inbox => "Keep mail and chats in one place. Inbox tabs are never unloaded, so notifications keep arriving.",
            GroupKind::Homeserver => "Dashboards for the services you self-host.",
            GroupKind::Dev => "Local dev servers, docs and repos — plus developer tools on every tab.",
            GroupKind::Normal => "Add a tab to this group.",
        }));
        ui.add_space(8.0);
        egui::Grid::new("presets").num_columns(2).spacing([8.0, 6.0]).show(ui, |ui| {
            for (i, (name, url)) in presets(g.kind).iter().enumerate() {
                let b = egui::Button::new(RichText::new(*name).font(FontId::new(12.5, sans()))).min_size(vec2(150.0, 30.0));
                if ui.add(b).on_hover_text(*url).clicked() {
                    self.add_tab(url, Some(gid), true, Some(name));
                    self.popover = Popover::None;
                }
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        ui.add_space(8.0);
        let key = Id::unique(("custom-url", gid));
        let mut s: String = ui.ctx().data(|d| d.get_temp(key)).unwrap_or_default();
        ui.horizontal(|ui| {
            let r = ui.add(egui::TextEdit::singleline(&mut s).hint_text("Custom URL…").desired_width(220.0));
            let add = ui.button("Add").clicked() || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            if add && !s.trim().is_empty() {
                let u = match omnibox::classify(&s) {
                    omnibox::Nav::Url(u) => u,
                    omnibox::Nav::Search(q) => self.profile.settings.search.search_url(&q),
                };
                self.add_tab(&u, Some(gid), true, None);
                s.clear();
                self.popover = Popover::None;
            }
        });
        ui.ctx().data_mut(|d| d.insert_temp(key, s));
    }

    fn rename_modal(&mut self, ctx: &egui::Context) {
        let Some((gid, mut name)) = self.rename.clone() else { return };
        let mut done = false;
        let mut cancel = false;
        egui::Area::new(Id::unique("rename")).fixed_pos(pos2(ctx.content_rect().center().x - 160.0, 90.0)).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::new().fill(PANEL).stroke(Stroke::new(1.0, LINE_STRONG)).corner_radius(4).inner_margin(16.0).show(ui, |ui| {
                ui.set_width(300.0);
                ui.label(theme::label("Name this group"));
                let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(290.0).font(FontId::new(15.0, sans())));
                if !r.has_focus() && !ui.input(|i| i.pointer.any_down()) {
                    r.request_focus();
                }
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = true;
                }
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        done = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        });
        if done {
            if let Some(g) = self.profile.groups.iter_mut().find(|g| g.id == gid) {
                g.name = name.trim().chars().take(32).collect();
                if g.name.is_empty() {
                    g.name = "Group".into();
                }
            }
            self.dirty = true;
            self.rename = None;
        } else if cancel || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.rename = None;
        } else {
            self.rename = Some((gid, name));
        }
    }

    fn toast_overlay(&mut self, ctx: &egui::Context) {
        let Some((t, _)) = &self.toast else { return };
        let r = self.content_rect;
        egui::Area::new(Id::unique("toast")).fixed_pos(pos2(r.left() + 16.0, r.bottom() - 52.0)).order(egui::Order::Tooltip).interactable(false).show(ctx, |ui| {
            egui::Frame::new().fill(RAISED).stroke(Stroke::new(1.0, LINE_STRONG)).corner_radius(3).inner_margin(egui::Margin::symmetric(12, 8)).show(ui, |ui| {
                ui.label(theme::mono_text(t, 12.0));
            });
        });
    }

    // ------------------------------------------------------------------ AI panel

    fn ai_panel(&mut self, ui: &mut Ui) {
        let provider = self.profile.settings.provider;
        ui.horizontal(|ui| {
            ui.label(theme::label_c(&format!("Assistant · {}", provider.map(|p| p.name()).unwrap_or("—")), ACCENT));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icons::button(ui, Icon::Close, 22.0, "Close").clicked() {
                    self.ai.open = false;
                }
                if !self.ai.msgs.is_empty() && ui.add(egui::Label::new(theme::label("Clear")).sense(Sense::click())).clicked() {
                    self.ai.msgs.clear();
                }
            });
        });
        ui.add_space(6.0);
        let (ctx_label, ctx_color) = match self.active_tab().map(|t| &t.kind) {
            Some(Kind::Inert(it)) if it.status == Status::Ready => (format!("Reading {} — signed tree", it.host), SECURE),
            Some(Kind::Web(w)) => (format!("Reading {} — untrusted text", omnibox::strip(&w.url).split('/').next().unwrap_or("")), WARN),
            _ => ("No page context".into(), MUTED),
        };
        egui::Frame::new().stroke(Stroke::new(1.0, ctx_color.gamma_multiply(0.5))).corner_radius(3).inner_margin(egui::Margin::symmetric(8, 5)).show(ui, |ui| {
            ui.label(RichText::new(ctx_label).font(FontId::new(11.0, mono())).color(ctx_color));
        });
        ui.add_space(8.0);
        let input_h = 96.0;
        let avail = ui.available_height() - input_h;
        egui::ScrollArea::vertical().max_height(avail).stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            if self.ai.msgs.is_empty() {
                ui.add_space(20.0);
                ui.label(theme::serif_text("Ask about this page.", 26.0));
                ui.add_space(6.0);
                ui.label(theme::muted("Inert pages are read from their signed structure. Web pages are read as untrusted text — instructions hidden in them are ignored."));
                ui.add_space(10.0);
                for s in ["Summarize this page", "What can I do here?", "Is anything on this page unusual?"] {
                    if ui.button(s).clicked() {
                        self.ask_ai(s.into());
                    }
                }
            }
            for m in &self.ai.msgs {
                if m.user {
                    ui.with_layout(Layout::top_down(Align::Max), |ui| {
                        egui::Frame::new().fill(RAISED).corner_radius(4).inner_margin(10.0).show(ui, |ui| {
                            ui.set_max_width(290.0);
                            ui.label(theme::body(&m.text));
                        });
                    });
                } else {
                    ui.label(RichText::new(&m.text).font(FontId::new(14.0, sans())).color(FG));
                }
                ui.add_space(10.0);
            }
            if self.ai.waiting {
                let dots = ".".repeat(((ui.input(|i| i.time) * 3.0) as usize % 3) + 1);
                ui.label(theme::label(&format!("Thinking{dots}")));
            }
        });
        ui.separator();
        if provider.is_none() {
            ui.label(theme::muted("Choose Claude or GPT in Settings to use the assistant."));
            return;
        }
        let r = ui.add(egui::TextEdit::multiline(&mut self.ai.input).desired_rows(2).desired_width(ui.available_width()).hint_text("Ask anything…  Enter sends · Shift+Enter for a new line").font(FontId::new(14.0, sans())));
        let send = r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
        if send && !self.ai.waiting {
            let q = self.ai.input.trim().to_owned();
            self.ai.input.clear();
            if !q.is_empty() {
                self.ai.msgs.retain(|_| true);
                self.ask_ai(q);
            }
        }
    }

    fn inspector_panel(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label(theme::label_c("Inspector · signed manifest", SECURE));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icons::button(ui, Icon::Close, 22.0, "Close").clicked() {
                    self.inspector = false;
                }
            });
        });
        let Some(Kind::Inert(it)) = self.active_tab().map(|t| &t.kind) else {
            ui.label(theme::muted("Open an inert:// tab to inspect its manifest. Web tabs use WebKit's developer tools (⌥⌘I)."));
            return;
        };
        let Some(m) = it.manifest.clone() else { return };
        egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("insp").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                for (k, v) in [("App", m.app.clone()), ("Name", m.name.clone()), ("Version", m.version.to_string()), ("Key", it.key_id.clone()), ("Entry", m.entry.clone()), ("Assets", m.assets.len().to_string())] {
                    ui.label(theme::label(k));
                    ui.label(theme::mono_text(&v, 12.0));
                    ui.end_row();
                }
            });
            ui.add_space(8.0);
            ui.label(theme::label("Screens"));
            for (name, s) in &m.screens {
                ui.collapsing(RichText::new(format!("{name} — {}", s.title)).font(FontId::new(12.5, mono())), |ui| {
                    for (k, t) in &s.slots {
                        ui.label(theme::mono_text(&format!("slot {k}: {}", t.name()), 11.5));
                    }
                    for (k, t) in &s.params {
                        ui.label(theme::mono_text(&format!("param {k}: {}", t.name()), 11.5));
                    }
                });
            }
            ui.add_space(8.0);
            ui.label(theme::label("Actions"));
            for (name, a) in &m.actions {
                let fields: Vec<String> = a.fields.iter().map(|(k, t)| format!("{k}: {}", t.name())).collect();
                ui.label(theme::mono_text(&format!("{name}({})", fields.join(", ")), 11.5));
            }
            if let Some(s) = &it.screen {
                ui.add_space(8.0);
                ui.label(theme::label("Current screen data (verified)"));
                for (k, v) in &s.data {
                    let txt = format!("{k} = {v:?}");
                    ui.label(theme::mono_text(&txt.chars().take(400).collect::<String>(), 11.0));
                }
            }
        });
    }

    // ------------------------------------------------------------------ content

    fn content(&mut self, ui: &mut Ui) {
        let id = self.active;
        let kind_tag = match self.tab(id).map(|t| &t.kind) {
            Some(Kind::Page(p)) => Some(*p),
            _ => None,
        };
        if let Some(p) = kind_tag {
            self.page(ui, p);
            return;
        }
        let mut acts = None;
        let mut failure = None;
        let mut confirm = None;
        if let Some(t) = self.tab_mut(id) {
            match &mut t.kind {
                Kind::Inert(it) => match it.status {
                    Status::Ready if it.screen.is_some() => {
                        acts = Some(crate::render::render(ui, it));
                        confirm = it.confirm_href.clone();
                    }
                    Status::Failed => failure = it.failure.clone().map(|f| (f, it.host.clone())),
                    _ => {
                        let r = ui.max_rect();
                        ui.painter().rect_filled(r, 0.0, BG);
                        ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, "VERIFYING SIGNED MANIFEST…", FontId::new(11.0, mono()), MUTED);
                    }
                },
                Kind::Web(_) => {
                    // The native web view covers this area.
                    let r = ui.max_rect();
                    ui.painter().rect_filled(r, 0.0, BG);
                }
                Kind::Page(_) => {}
            }
        }
        if let Some(a) = acts {
            self.apply_acts(id, a);
        }
        if let Some((f, host)) = failure {
            self.failure_page(ui, id, &host, &f);
        }
        if let Some(h) = confirm {
            self.confirm_external(ui.ctx(), id, &h);
        }
    }

    fn confirm_external(&mut self, ctx: &egui::Context, id: u64, href: &str) {
        let mut choice = None;
        let r = self.content_rect;
        egui::Area::new(Id::unique("confirm-href")).fixed_pos(pos2(r.center().x - 200.0, r.top() + 60.0)).order(egui::Order::Foreground).show(ctx, |ui| {
            egui::Frame::new().fill(PANEL).stroke(Stroke::new(1.0, LINE_STRONG)).corner_radius(4).inner_margin(18.0).show(ui, |ui| {
                ui.set_width(380.0);
                let web = href.starts_with("https://");
                ui.label(theme::label_c(if web { "Leaving inert:// for the web" } else { "Opening another inert:// site" }, if web { WARN } else { SECURE }));
                ui.add_space(6.0);
                ui.label(theme::mono_text(href, 12.0));
                ui.add_space(6.0);
                ui.label(theme::body(if web { "The destination is an ordinary web page. It will run its own code and is shown as unsecured." } else { "That site is verified separately with its own pinned key." }));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.button("Open in new tab").clicked() {
                        choice = Some(true);
                    }
                    if ui.button("Cancel").clicked() {
                        choice = Some(false);
                    }
                });
            });
        });
        if let Some(c) = choice {
            if let Some(Kind::Inert(it)) = self.tab_mut(id).map(|t| &mut t.kind) {
                it.confirm_href = None;
            }
            if c {
                self.add_tab(href, None, true, None);
            }
        }
    }

    // ------------------------------------------------------------------ shortcuts

    pub fn shortcuts(&mut self, ctx: &egui::Context) {
        use egui::{Key, Modifiers};
        let cmd = Modifiers::COMMAND;
        let shift = Modifiers::COMMAND | Modifiers::SHIFT;
        let alt = Modifiers::COMMAND | Modifiers::ALT;
        let mut hits = Vec::new();
        ctx.input_mut(|i| {
            for (m, k, name) in [
                (cmd, Key::T, "new-tab"),
                (cmd, Key::W, "close-tab"),
                (shift, Key::T, "reopen"),
                (cmd, Key::L, "focus-omnibox"),
                (cmd, Key::R, "reload"),
                (cmd, Key::OpenBracket, "back"),
                (cmd, Key::CloseBracket, "forward"),
                (cmd, Key::D, "bookmark"),
                (cmd, Key::Y, "history"),
                (cmd, Key::J, "assistant"),
                (cmd, Key::F, "find"),
                (cmd, Key::Comma, "settings"),
                (shift, Key::B, "bookmarks-bar"),
                (alt, Key::B, "bookmarks"),
                (alt, Key::I, "devtools"),
                (cmd, Key::Equals, "zoom-in"),
                (cmd, Key::Plus, "zoom-in"),
                (cmd, Key::Minus, "zoom-out"),
                (cmd, Key::Num0, "zoom-reset"),
                (Modifiers::CTRL, Key::Tab, "next-tab"),
                (Modifiers::CTRL | Modifiers::SHIFT, Key::Tab, "prev-tab"),
                (alt, Key::ArrowRight, "next-tab"),
                (alt, Key::ArrowLeft, "prev-tab"),
            ] {
                if i.consume_key(m, k) {
                    hits.push(name.to_owned());
                }
            }
            for (n, k) in [Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9].iter().enumerate() {
                if i.consume_key(cmd, *k) {
                    hits.push(format!("tab-{}", n + 1));
                }
            }
        });
        for h in hits {
            self.on_menu(&h);
        }
    }

    pub fn on_menu(&mut self, id: &str) {
        match id {
            "new-tab" => {
                self.add_tab("about:newtab", None, true, None);
                self.omni.focus = true;
            }
            "close-tab" => {
                let a = self.active;
                self.close_tab(a);
            }
            "reopen" => self.reopen_closed(),
            "focus-omnibox" => self.omni.focus = true,
            "reload" => self.reload(),
            "back" => self.go_back(),
            "forward" => self.go_forward(),
            "bookmark" => {
                let (u, t) = self.active_tab().map(|t| (t.url(), t.title())).unwrap_or_default();
                self.profile.toggle_bookmark(&u, &t);
                self.dirty = true;
            }
            "history" => {
                self.add_tab("about:history", None, true, None);
            }
            "bookmarks" => {
                self.add_tab("about:bookmarks", None, true, None);
            }
            "assistant" => self.ai.open = !self.ai.open,
            "find" => self.find = if self.find.is_some() { None } else { Some(String::new()) },
            "settings" => {
                self.add_tab("about:settings", None, true, None);
            }
            "bookmarks-bar" => {
                self.profile.settings.bookmarks_bar = !self.profile.settings.bookmarks_bar;
                self.dirty = true;
            }
            "devtools" => self.devtools(),
            "zoom-in" => self.zoom(0.1),
            "zoom-out" => self.zoom(-0.1),
            "zoom-reset" => self.zoom(0.0),
            "next-tab" => self.cycle(1),
            "prev-tab" => self.cycle(-1),
            "vpn" => {
                let r = Rect::from_min_size(pos2(self.content_rect.right() - 60.0, 30.0), Vec2::ZERO);
                self.open_popover(Popover::Vpn, r);
            }
            s if s.starts_with("tab-") => {
                let n: usize = s[4..].parse().unwrap_or(1);
                let order = self.visible_order();
                let target = if n == 9 { order.last().copied() } else { order.get(n - 1).copied() };
                if let Some(t) = target {
                    self.activate(t);
                }
            }
            _ => {}
        }
    }
}


