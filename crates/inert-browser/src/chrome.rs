//! Browser chrome. The window is one blue glyph card: a sidebar of tabs and
//! groups on the left, a toolbar with the address bar on top, and the page in
//! an inset rounded card. Also the popovers (site security, VPN, shield,
//! menu), the AI side panel and the content area.

use crate::app::{Browser, Kind, Page, Popover};
use crate::icons::{self, Icon};
use crate::inert_tab::Status;
use crate::omnibox::{self, Kind as SKind};
use crate::profile::{GroupKind, presets};
use crate::theme::{self, *};
use crate::{adblock, vpn};
use eframe::egui::{self, Align, Color32, FontId, Id, Layout, Rect, RichText, Sense, Stroke, Ui, Vec2, pos2, vec2};
use std::time::{Duration, Instant};

const SIDE_W: f32 = 252.0;
const TOP_H: f32 = 52.0;
const ROW_H: f32 = 34.0;
const MARKS_H: f32 = 30.0;
const LEFT_INSET: f32 = 80.0; // macOS traffic lights
const GAP: f32 = 8.0;
/// Corner radius of the page card (web views are rounded to match).
pub const CARD_R: f32 = 14.0;
/// Where the traffic lights sit: centered on the toolbar's line.
pub const LIGHTS: (f64, f64) = (18.0, TOP_H as f64 / 2.0);

pub fn group_icon(k: GroupKind) -> Option<Icon> {
    match k {
        GroupKind::Inbox => Some(Icon::Inbox),
        GroupKind::Homeserver => Some(Icon::Server),
        GroupKind::Dev => Some(Icon::Code),
        GroupKind::Normal => None,
    }
}

/// Ease-out cubic.
pub fn ease(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// A tab that was just closed, drawn shrinking for a moment after the tab it followed.
pub struct Ghost {
    pub after: Option<u64>,
    pub h: f32,
    pub start: f64,
    pub title: String,
}

/// Paints the frame color over a rect's corners, so whatever was drawn inside
/// it ends up with rounded corners (egui clips to rectangles only).
pub fn round_corners(p: &egui::Painter, r: Rect, rad: f32, c: Color32) {
    for (corner, sx, sy) in [(r.left_top(), 1.0, 1.0), (r.right_top(), -1.0, 1.0), (r.right_bottom(), -1.0, -1.0), (r.left_bottom(), 1.0, -1.0)] {
        let centre = corner + vec2(sx * rad, sy * rad);
        let mut mesh = egui::Mesh::default();
        mesh.colored_vertex(corner, c);
        let n = 12u32;
        for i in 0..=n {
            let a = std::f32::consts::FRAC_PI_2 * i as f32 / n as f32;
            mesh.colored_vertex(centre - vec2(sx * rad * a.sin(), sy * rad * a.cos()), c);
        }
        for i in 1..=n {
            mesh.add_triangle(0, i, i + 1);
        }
        p.add(egui::Shape::mesh(mesh));
    }
}

fn tab_icon(k: &Kind) -> (Icon, Color32) {
    match k {
        Kind::Inert(it) if it.status == Status::Failed => (Icon::Unlock, Color32::from_rgb(255, 196, 196)),
        Kind::Inert(_) => (Icon::Lock, WHITE),
        Kind::Web(_) => (Icon::Globe, glass(0.72)),
        Kind::Page(Page::Settings) => (Icon::Gear, glass(0.72)),
        Kind::Page(Page::History) => (Icon::History, glass(0.72)),
        Kind::Page(Page::Bookmarks) => (Icon::Bookmark, glass(0.72)),
        Kind::Page(_) => (Icon::Spark, glass(0.72)),
    }
}

fn spinner(p: &egui::Painter, c: egui::Pos2, time: f32) {
    p.circle_stroke(c, 5.5, Stroke::new(1.2, glass(0.25)));
    let a = time * 6.0;
    let arc: Vec<egui::Pos2> = (0..=10).map(|i| {
        let t = a + i as f32 / 10.0 * 2.2;
        c + vec2(t.cos(), t.sin()) * 5.5
    }).collect();
    p.add(egui::Shape::line(arc, Stroke::new(1.6, WHITE)));
}

enum Security {
    Secured { dev: bool },
    Failed,
    Connecting,
    Unsecured(&'static str),
    Unencrypted(&'static str),
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
                let e = w.engine.unwrap_or(self.profile.settings.web_engine).name();
                if w.url.starts_with("https://") || w.url.starts_with("data:") { Security::Unsecured(e) } else { Security::Unencrypted(e) }
            }
            _ => Security::Internal,
        }
    }

    pub fn open_popover(&mut self, p: Popover, anchor: Rect) {
        let recently_closed = self.ctx.data(|d| d.get_temp::<(Instant, Popover)>(Id::unique("pop_closed_at"))).is_some_and(|(t, k)| k == p && t.elapsed() < Duration::from_millis(250));
        if self.popover == p || recently_closed {
            self.popover = Popover::None;
            return;
        }
        self.popover = p;
        self.popover_opened = Some(Instant::now());
        self.ctx.data_mut(|d| d.insert_temp(Id::unique("pop_anchor"), anchor));
    }

    pub fn toggle_sidebar(&mut self) {
        self.profile.settings.sidebar = !self.profile.settings.sidebar;
        self.dirty = true;
    }

    pub fn chrome(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let win = ui.max_rect();
        let side_t = ease(ctx.animate_bool_with_time(Id::unique("side-open"), self.profile.settings.sidebar, 0.28));
        let side_w = (SIDE_W * side_t).round();
        let marks = if self.profile.settings.bookmarks_bar { MARKS_H } else { 0.0 };

        // The frame is one blue card. Its glyph map shows on the sidebar and
        // the bars; the page card covers the rest.
        let p = ui.painter().clone();
        p.rect_filled(win, 0.0, theme::frame());
        let ptr = ctx.pointer_latest_pos();
        let g = crate::art::Glyphs::frame();
        crate::art::glyphs(&p, &ctx, Rect::from_min_max(win.min, pos2(win.left() + side_w, win.bottom())), ptr, &g);
        crate::art::glyphs(&p, &ctx, Rect::from_min_max(pos2(win.left() + side_w, win.top()), pos2(win.right(), win.top() + TOP_H + marks)), ptr, &g);

        if side_w >= 1.0 {
            egui::Panel::left("side").resizable(false).show_separator_line(false).exact_size(side_w).frame(egui::Frame::NONE).show(ui, |ui| self.sidebar(ui));
        }
        egui::Panel::top("toolbar").show_separator_line(false).exact_size(TOP_H).frame(egui::Frame::NONE).show(ui, |ui| self.toolbar(ui, side_t));
        if self.profile.settings.bookmarks_bar {
            let left = (4.0 + (GAP + 4.0) * (1.0 - side_t)) as i8;
            egui::Panel::top("marks").show_separator_line(false).exact_size(MARKS_H).frame(egui::Frame::new().inner_margin(egui::Margin { left, right: 14, top: 0, bottom: 6 })).show(ui, |ui| self.bookmarks_bar(ui));
        }
        if self.find.is_some() {
            egui::Panel::top("find").show_separator_line(false).exact_size(44.0).frame(egui::Frame::new().inner_margin(egui::Margin { left: (GAP * (1.0 - side_t)) as i8, right: GAP as i8, top: 0, bottom: 6 })).show(ui, |ui| self.find_bar(ui));
        }
        if self.ai.open {
            egui::Panel::right("ai").resizable(false).show_separator_line(false).exact_size(388.0).frame(egui::Frame::NONE).show(ui, |ui| {
                let mut c = side_card(ui);
                self.ai_panel(&mut c);
            });
        }
        if self.inspector {
            egui::Panel::right("inspector").resizable(false).show_separator_line(false).exact_size(348.0).frame(egui::Frame::NONE).show(ui, |ui| {
                let mut c = side_card(ui);
                self.inspector_panel(&mut c);
            });
        }
        egui::CentralPanel::no_frame().show(ui, |ui| {
            let r = ui.max_rect();
            let card = Rect::from_min_max(pos2(r.left() + GAP * (1.0 - side_t), r.top()), pos2(r.right() - GAP, r.bottom() - GAP));
            let p = ui.painter().clone();
            p.add(egui::epaint::Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(70) }.as_shape(card, CARD_R));
            p.rect_filled(card, CARD_R, BG);
            self.content_rect = card;
            let mut child = ui.new_child(egui::UiBuilder::new().max_rect(card).layout(Layout::top_down(Align::Min)));
            child.set_clip_rect(card);
            self.content(&mut child);
            round_corners(&p, card, CARD_R, theme::frame());
            p.rect_stroke(card, CARD_R, Stroke::new(1.0, glass(0.16)), egui::StrokeKind::Inside);
        });
        self.popovers(&ctx);
        self.omnibox_dropdown(&ctx);
        self.ntp_dropdown(&ctx);
        self.rename_modal(&ctx);
        self.toast_overlay(&ctx);
    }

    /// Empty frame areas move the window; a double click zooms it.
    fn frame_drag(&self, ui: &mut Ui, r: Rect, key: &'static str) {
        let drag = ui.interact(r, Id::unique(key), Sense::click_and_drag());
        if drag.drag_started() {
            if std::env::var_os("INERT_DEBUG_FRAME").is_some() {
                eprintln!("frame drag started on {key} at {:?}", ui.ctx().pointer_latest_pos());
            }
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if drag.double_clicked() {
            if std::env::var_os("INERT_DEBUG_FRAME").is_some() {
                eprintln!("frame double click on {key} at {:?}", ui.ctx().pointer_latest_pos());
            }
            let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }
    }

    // ------------------------------------------------------------------ sidebar

    fn sidebar(&mut self, ui: &mut Ui) {
        let panel = ui.max_rect();
        // Laid out at full width; it slides in from the left while the panel opens.
        let full = Rect::from_min_max(pos2(panel.right() - SIDE_W, panel.top()), panel.max);
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        self.frame_drag(ui, panel, "side-drag");
        ui.set_clip_rect(panel);

        // Top row: the traffic lights on the left, the sidebar toggle on the right.
        let top = Rect::from_min_max(pos2(full.left() + LEFT_INSET, full.top()), pos2(full.right() - 10.0, full.top() + TOP_H));
        let mut tu = ui.new_child(egui::UiBuilder::new().max_rect(top).layout(Layout::right_to_left(Align::Center)));
        let tg = icons::button(&mut tu, Icon::Sidebar, 30.0, "Hide sidebar  Cmd+S");
        crate::drive::reg("sidebar", tg.rect);
        if tg.clicked() {
            self.toggle_sidebar();
        }

        // Bottom row: VPN on the left, assistant and settings on the right.
        let bottom = Rect::from_min_max(pos2(full.left() + 10.0, full.bottom() - 46.0), pos2(full.right() - 10.0, full.bottom() - GAP));
        let mut bu = ui.new_child(egui::UiBuilder::new().max_rect(bottom).layout(Layout::left_to_right(Align::Center)));
        self.sidebar_footer(&mut bu);

        let list = Rect::from_min_max(pos2(full.left() + 8.0, top.bottom()), pos2(full.right() - 8.0, bottom.top() - 6.0));
        let mut lu = ui.new_child(egui::UiBuilder::new().max_rect(list).layout(Layout::top_down(Align::Min)));
        lu.set_clip_rect(list.intersect(panel));
        egui::ScrollArea::vertical().auto_shrink([false, false]).scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden).show(&mut lu, |ui| {
            ui.spacing_mut().item_spacing = Vec2::ZERO;
            // Tabs that appear after startup grow in; the ones restored at launch don't.
            let animate_new = ctx.cumulative_pass_nr() > 3;

            // Pinned tabs: a grid of glass tiles.
            let pinned: Vec<u64> = self.tabs.iter().filter(|t| t.pinned).map(|t| t.id).collect();
            if !pinned.is_empty() {
                let gap = 6.0;
                let tw = ((ui.available_width() - gap * 3.0) / 4.0).floor();
                for row in pinned.chunks(4) {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        for id in row {
                            self.pin_tile(ui, *id, tw);
                        }
                    });
                    ui.add_space(gap);
                }
                ui.add_space(4.0);
            }
            self.ghosts(ui, None, now);
            for id in pinned {
                self.ghosts(ui, Some(id), now);
            }

            // Groups: a folder row, then its tabs, which slide open and closed.
            let groups = self.profile.groups.clone();
            for g in &groups {
                let ids: Vec<u64> = self.tabs.iter().filter(|t| !t.pinned && t.group == Some(g.id)).map(|t| t.id).collect();
                let color = group_color(&g.color);
                let open = ease(ctx.animate_bool_with_time(Id::unique(("grp-open", g.id)), !g.collapsed, 0.26));
                self.group_row(ui, g, ids.len(), color, open);
                if open > 0.002 {
                    for id in ids {
                        self.tab_row(ui, id, open, Some(color), animate_new);
                        self.ghosts(ui, Some(id), now);
                    }
                }
            }
            if !groups.is_empty() {
                ui.add_space(6.0);
            }

            // Ungrouped tabs.
            let rest: Vec<u64> = self.tabs.iter().filter(|t| !t.pinned && t.group.is_none_or(|g| !groups.iter().any(|x| x.id == g))).map(|t| t.id).collect();
            for id in rest {
                self.tab_row(ui, id, 1.0, None, animate_new);
                self.ghosts(ui, Some(id), now);
            }

            // "+ New Tab", the last row.
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
            let resp = ui.interact(rect, Id::unique("newtab-row"), Sense::click());
            crate::drive::reg("newtab", rect);
            let hov = ctx.animate_bool_with_time(Id::unique("newtab-hov"), resp.hovered(), 0.14);
            let body = rect.shrink2(vec2(0.0, 1.0));
            let p = ui.painter();
            if hov > 0.0 {
                p.rect_filled(body, 10.0, glass(0.09 * hov));
            }
            let ink = glass(0.66 + 0.34 * hov);
            icons::paint(p, Rect::from_center_size(pos2(body.left() + 16.0, body.center().y), Vec2::splat(13.0)), Icon::Plus, ink);
            p.text(pos2(body.left() + 34.0, body.center().y), egui::Align2::LEFT_CENTER, "New Tab", FontId::new(13.0, sans()), ink);
            if resp.on_hover_text("New tab  Cmd+T").clicked() {
                self.add_tab("about:newtab", None, true, None);
            }
            ui.add_space(12.0);
        });
        self.closing.retain(|g| now - g.start < 0.3);
    }

    fn sidebar_footer(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        ui.spacing_mut().item_spacing.x = 2.0;
        let st = &self.vpn.status;
        let cc = self.vpn.relays.iter().find(|c| c.name == st.country).map(|c| c.code.to_uppercase());
        let on = st.connected();
        let txt = match (on, cc) {
            (true, Some(cc)) => format!("VPN · {cc}"),
            _ => st.short(),
        };
        let galley = ui.painter().layout_no_wrap(txt, FontId::new(10.5, mono()), WHITE);
        let (rect, resp) = ui.allocate_exact_size(vec2(galley.size().x + 36.0, 28.0), Sense::click());
        crate::drive::reg("vpn", rect);
        let hov = ctx.animate_bool_with_time(Id::unique("vpn-hov"), resp.hovered(), 0.14);
        let p = ui.painter();
        p.rect_filled(rect, 14.0, glass(0.10 + 0.08 * hov));
        let dot = pos2(rect.left() + 14.0, rect.center().y);
        if on {
            p.circle_filled(dot, 6.0, OK.gamma_multiply(0.28));
        }
        p.circle_filled(dot, 3.2, if on { OK } else { glass(0.5) });
        p.galley(pos2(rect.left() + 25.0, rect.center().y - galley.size().y / 2.0), galley, WHITE);
        if resp.on_hover_text("Mullvad VPN").clicked() {
            self.open_popover(Popover::Vpn, rect);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let air = icons::button_c(ui, Icon::Spark, 30.0, "Assistant  Cmd+J", if self.ai.open { WHITE } else { glass(0.8) }, true);
            crate::drive::reg("ai", air.rect);
            if self.ai.open {
                ui.painter().circle_filled(pos2(air.rect.center().x, air.rect.bottom() - 2.0), 1.6, WHITE);
            }
            if air.clicked() {
                self.ai.open = !self.ai.open;
            }
            if icons::button_c(ui, Icon::Gear, 30.0, "Settings  Cmd+,", glass(0.8), true).clicked() {
                self.add_tab("about:settings", None, true, None);
            }
            if icons::button_c(ui, Icon::History, 30.0, "History  Cmd+Y", glass(0.8), true).clicked() {
                self.add_tab("about:history", None, true, None);
            }
        });
    }

    /// Tabs that were just closed, folding away where they stood.
    fn ghosts(&mut self, ui: &mut Ui, after: Option<u64>, now: f64) {
        for g in self.closing.iter().filter(|g| g.after == after) {
            let f = 1.0 - ease(((now - g.start) / 0.2) as f32);
            if f <= 0.0 {
                continue;
            }
            ui.ctx().request_repaint();
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), g.h * f), Sense::hover());
            let p = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
            let body = Rect::from_min_size(pos2(rect.left(), rect.top() + 1.0), vec2(rect.width(), ROW_H - 2.0));
            p.rect_filled(body, 10.0, glass(0.10 * f));
            let mut job = egui::text::LayoutJob::single_section(g.title.clone(), egui::TextFormat::simple(FontId::new(13.0, sans()), glass(0.7 * f)));
            job.wrap = egui::text::TextWrapping::truncate_at_width((rect.width() - 50.0).max(0.0));
            let galley = ui.fonts_mut(|fo| fo.layout_job(job));
            p.galley(pos2(body.left() + 34.0, body.center().y - galley.size().y / 2.0), galley, WHITE);
        }
    }

    /// A group's folder row: color chip, name, tab count and a chevron that
    /// turns as the group opens.
    fn group_row(&mut self, ui: &mut Ui, g: &crate::profile::GroupDef, count: usize, color: Color32, open: f32) {
        // Rows get ids of their own: automatic ids shift when rows above
        // fold away, which would hand an open context menu to another row.
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::hover());
        let resp = ui.interact(rect, Id::unique(("group-row", g.id)), Sense::click());
        crate::drive::reg(format!("group:{}", g.name), rect);
        let hov = ui.ctx().animate_bool_with_time(Id::unique(("chip-hov", g.id)), resp.hovered(), 0.14);
        let body = rect.shrink2(vec2(0.0, 1.0));
        let p = ui.painter();
        if hov > 0.0 {
            p.rect_filled(body, 10.0, glass(0.09 * hov));
        }
        // The chip carries the group's color; it stays readable on the blue frame.
        let chip = Rect::from_center_size(pos2(body.left() + 16.0, body.center().y), Vec2::splat(18.0));
        p.rect_filled(chip, 6.0, color);
        icons::paint(p, chip.shrink(3.5), group_icon(g.kind).unwrap_or(Icon::Folder), BG);
        let mut job = egui::text::LayoutJob::single_section(g.name.clone(), egui::TextFormat::simple(FontId::new(13.0, sans_medium()), WHITE));
        job.wrap = egui::text::TextWrapping::truncate_at_width((body.width() - 90.0).max(0.0));
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        p.galley(pos2(body.left() + 34.0, body.center().y - galley.size().y / 2.0), galley, WHITE);
        // Chevron: points right when closed, down when open.
        let c = pos2(body.right() - 14.0, body.center().y);
        let a = open * std::f32::consts::FRAC_PI_2;
        let rot = |v: Vec2| vec2(v.x * a.cos() - v.y * a.sin(), v.x * a.sin() + v.y * a.cos());
        p.add(egui::Shape::line(vec![c + rot(vec2(-1.5, -3.5)), c + rot(vec2(2.0, 0.0)), c + rot(vec2(-1.5, 3.5))], Stroke::new(1.4, glass(0.7))));
        if count > 0 {
            p.text(pos2(body.right() - 28.0, body.center().y), egui::Align2::RIGHT_CENTER, count.to_string(), FontId::new(10.5, mono()), glass(0.7));
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

    /// A pinned tab: a square glass tile with just its icon.
    fn pin_tile(&mut self, ui: &mut Ui, id: u64, w: f32) {
        let Some(t) = self.tab(id) else { return };
        let ctx = ui.ctx().clone();
        let active = id == self.active;
        let title = t.title();
        let loading = t.loading();
        let (icon, icon_c) = tab_icon(&t.kind);
        let (rect, _) = ui.allocate_exact_size(vec2(w, 40.0), Sense::hover());
        let resp = ui.interact(rect, Id::unique(("tab-row", id)), Sense::click());
        crate::drive::reg(format!("tab:{title}"), rect);
        let hov = ctx.animate_bool_with_time(Id::unique(("tab-hov", id)), resp.hovered(), 0.14);
        let act = ctx.animate_bool_with_time(Id::unique(("tab-act", id)), active, 0.18);
        let down = ctx.animate_bool_with_time(Id::unique(("tab-down", id)), resp.is_pointer_button_down_on(), 0.08);
        let body = rect.shrink(down);
        let p = ui.painter();
        p.rect_filled(body, 12.0, glass(0.10 + 0.06 * hov + 0.14 * act));
        if act > 0.0 {
            p.rect_stroke(body, 12.0, Stroke::new(1.0, glass(0.22 * act)), egui::StrokeKind::Inside);
        }
        if loading {
            spinner(p, body.center(), ctx.input(|i| i.time) as f32);
        } else {
            icons::paint(p, Rect::from_center_size(body.center(), Vec2::splat(16.0)), icon, icon_c);
        }
        let resp = resp.on_hover_text(&title);
        if resp.clicked() {
            self.activate(id);
        }
        if resp.middle_clicked() {
            self.close_tab(id);
            return;
        }
        self.tab_menu(&resp, id, true);
    }

    /// One tab row. `scale` folds it while its group opens or closes; new
    /// tabs unfold from nothing.
    fn tab_row(&mut self, ui: &mut Ui, id: u64, scale: f32, group: Option<Color32>, animate_new: bool) {
        let Some(t) = self.tab(id) else { return };
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        let active = id == self.active;
        let title = t.title();
        let loading = t.loading();
        let (icon, icon_c) = tab_icon(&t.kind);
        let t0 = ctx.data_mut(|d| *d.get_temp_mut_or_insert_with(Id::unique(("tab-born", id)), || if animate_new { now } else { now - 10.0 }));
        let grow = ease(((now - t0) / 0.22) as f32);
        if grow < 1.0 {
            ctx.request_repaint();
        }
        let k = (scale * grow).clamp(0.0, 1.0);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H * k), Sense::hover());
        ctx.data_mut(|d| d.insert_temp(Id::unique(("tab-h", id)), ROW_H));
        if rect.height() < 1.0 {
            return;
        }
        let resp = ui.interact(rect, Id::unique(("tab-row", id)), Sense::click());
        crate::drive::reg(format!("tab:{title}"), rect);
        let indent = if group.is_some() { 16.0 } else { 0.0 };
        // The row keeps its full height while it unfolds, clipped to what has opened.
        let body = Rect::from_min_size(pos2(rect.left() + indent, rect.top() + 1.0), vec2(rect.width() - indent, ROW_H - 2.0));
        let p = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
        let hov = ctx.animate_bool_with_time(Id::unique(("tab-hov", id)), resp.hovered(), 0.14);
        let act = ctx.animate_bool_with_time(Id::unique(("tab-act", id)), active, 0.18);
        let fill = (0.09 * hov * (1.0 - act) + 0.20 * act) * k;
        if fill > 0.0 {
            p.rect_filled(body, 10.0, glass(fill));
        }
        if act > 0.0 {
            p.rect_stroke(body, 10.0, Stroke::new(1.0, glass(0.20 * act * k)), egui::StrokeKind::Inside);
        }
        if let Some(gc) = group {
            // A guide in the group's color ties the tabs to their folder.
            p.line_segment([pos2(rect.left() + 16.0, rect.top()), pos2(rect.left() + 16.0, rect.bottom())], Stroke::new(1.5, gc.gamma_multiply(0.9 * k)));
        }
        let ic = pos2(body.left() + 16.0 + if group.is_some() { 6.0 } else { 0.0 }, body.center().y);
        if loading {
            spinner(&p, ic, now as f32);
        } else {
            icons::paint(&p, Rect::from_center_size(ic, Vec2::splat(14.0)), icon, icon_c.gamma_multiply(k));
        }
        let show_close = (active || resp.hovered() || ui.rect_contains_pointer(rect)) && k > 0.99;
        let text_left = ic.x + 18.0;
        let avail = (body.right() - if show_close { 30.0 } else { 12.0 } - text_left).max(0.0);
        let ink = glass((0.80 + 0.20 * act.max(hov)) * k);
        let mut job = egui::text::LayoutJob::single_section(title.clone(), egui::TextFormat::simple(FontId::new(13.0, sans()), ink));
        job.wrap = egui::text::TextWrapping::truncate_at_width(avail);
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        p.galley(pos2(text_left, body.center().y - galley.size().y / 2.0), galley, WHITE);
        if show_close {
            let cr = Rect::from_center_size(pos2(body.right() - 16.0, body.center().y), Vec2::splat(20.0));
            let cresp = ui.interact(cr, Id::unique(("close", id)), Sense::click());
            let ch = ctx.animate_bool_with_time(Id::unique(("close-hov", id)), cresp.hovered(), 0.12);
            if ch > 0.0 {
                p.circle_filled(cr.center(), 10.0, glass(0.18 * ch));
            }
            icons::paint(&p, cr.shrink(6.0), Icon::Close, glass(0.7 + 0.3 * ch));
            if cresp.clicked() {
                self.close_tab(id);
                return;
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
        self.tab_menu(&resp, id, false);
    }

    fn tab_menu(&mut self, resp: &egui::Response, id: u64, pinned: bool) {
        let groups = self.profile.groups.clone();
        resp.context_menu(|ui| {
            ui.set_min_width(230.0);
            if ui.button("New tab below").clicked() {
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
            let pb = ui.button(if pinned { "Unpin" } else { "Pin" });
            crate::drive::reg(if pinned { "ctx:Unpin" } else { "ctx:Pin" }, pb.rect);
            if pb.clicked() {
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

    fn toolbar(&mut self, ui: &mut Ui, side_t: f32) {
        let full = ui.max_rect();
        self.frame_drag(ui, full, "bar-drag");
        // With the sidebar hidden, the traffic lights sit at the bar's left end.
        let inset = 4.0 + (LEFT_INSET - 4.0) * (1.0 - side_t);
        let inner = Rect::from_min_max(pos2(full.left() + inset, full.top()), pos2(full.right() - 12.0, full.bottom()));
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner).layout(Layout::left_to_right(Align::Center)));
        let ui = &mut child;
        ui.spacing_mut().item_spacing.x = 2.0;
        if side_t < 0.5 {
            let tg = icons::button(ui, Icon::Sidebar, 30.0, "Show sidebar  Cmd+S");
            crate::drive::reg("sidebar", tg.rect);
            if tg.clicked() {
                self.toggle_sidebar();
            }
        }
        let rb = icons::button_c(ui, Icon::Back, 30.0, "Back  Cmd+[", WHITE, self.can_go(true));
        crate::drive::reg("back", rb.rect);
        if rb.clicked() {
            self.go_back();
        }
        let rf = icons::button_c(ui, Icon::Forward, 30.0, "Forward  Cmd+]", WHITE, self.can_go(false));
        crate::drive::reg("forward", rf.rect);
        if rf.clicked() {
            self.go_forward();
        }
        let loading = self.active_tab().is_some_and(|t| t.loading());
        if loading {
            if icons::button(ui, Icon::Stop, 30.0, "Stop").clicked() {
                self.stop();
            }
        } else if icons::button(ui, Icon::Reload, 30.0, "Reload  Cmd+R").clicked() {
            self.reload();
        }
        ui.add_space(8.0);
        // The address bar: centered in what is left, never wider than it reads well.
        let right_w = 4.0 * 32.0 + 8.0;
        let avail = ui.available_width() - right_w;
        let w = avail.clamp(200.0, 820.0);
        let pad = ((avail - w) / 2.0).max(0.0);
        ui.add_space(pad);
        self.omnibox(ui, w);
        ui.add_space(pad + 8.0);
        let url = self.active_tab().map(|t| t.url()).unwrap_or_default();
        let marked = self.profile.is_bookmarked(&url);
        let rs = icons::button_c(ui, if marked { Icon::StarFilled } else { Icon::Star }, 30.0, "Bookmark this tab  Cmd+D", WHITE, !url.starts_with("about:"));
        crate::drive::reg("star", rs.rect);
        if rs.clicked() {
            let title = self.active_tab().map(|t| t.title()).unwrap_or_default();
            self.profile.toggle_bookmark(&url, &title);
            self.dirty = true;
        }
        let rc = icons::button_c(ui, Icon::Cookie, 30.0, "Cookies — see and delete this site's cookies", if self.profile.settings.cookie_banners { WHITE } else { glass(0.55) }, true);
        crate::drive::reg("cookies", rc.rect);
        if rc.clicked() {
            self.cookies_refresh();
            self.open_popover(Popover::Cookies, rc.rect);
        }
        let (shield_c, tip) = match (&self.ad.state, self.profile.settings.adblock) {
            (_, false) => (glass(0.55), "Shield is off".to_owned()),
            (adblock::State::Ready(n), _) => (WHITE, format!("Shield on — {n} ad & tracker rules")),
            (adblock::State::Failed(e), _) => (Color32::from_rgb(255, 196, 196), format!("Shield failed: {e}")),
            _ => (WARN, "Shield is preparing filter lists…".to_owned()),
        };
        let r = icons::button_c(ui, Icon::Shield, 30.0, &tip, shield_c, true);
        crate::drive::reg("shield", r.rect);
        if matches!((&self.ad.state, self.profile.settings.adblock), (adblock::State::Ready(_), true)) {
            // A lit dot says the shield is up.
            ui.painter().circle_filled(pos2(r.rect.right() - 7.0, r.rect.top() + 8.0), 2.6, OK);
        }
        if r.clicked() {
            self.open_popover(Popover::Shield, r.rect);
        }
        let r = icons::button(ui, Icon::Menu, 30.0, "Menu");
        crate::drive::reg("menu", r.rect);
        if r.clicked() {
            self.open_popover(Popover::Menu, r.rect);
        }
    }

    fn omnibox(&mut self, ui: &mut Ui, width: f32) {
        let id = Id::unique("omnibox");
        let (rect, bg) = ui.allocate_exact_size(vec2(width, 36.0), Sense::click());
        let focused = ui.memory(|m| m.has_focus(id));
        let ctx = ui.ctx().clone();
        let f = ctx.animate_bool_with_time(Id::unique("omni-focus"), focused, 0.16);
        let hov = ctx.animate_bool_with_time(Id::unique("omni-hover"), bg.hovered() || ui.rect_contains_pointer(rect), 0.12);
        let rad = rect.height() / 2.0;
        let p = ui.painter();
        // A navy glass well in the blue frame: deepens on hover, and on focus
        // goes dark with a soft white halo.
        if f > 0.0 {
            p.rect_stroke(rect.expand(1.5), rad + 1.5, Stroke::new(3.0, glass(0.16 * f)), egui::StrokeKind::Outside);
        }
        p.rect_filled(rect, rad, well(0.38 + 0.08 * hov + 0.34 * f));
        p.rect_stroke(rect, rad, Stroke::new(1.0, glass(0.10 + 0.08 * hov + 0.26 * f)), egui::StrokeKind::Inside);
        // While the page loads, a light runs along the bottom edge.
        if self.active_tab().is_some_and(|t| t.loading()) {
            let tm = ctx.input(|i| i.time) as f32;
            let (x0, x1) = (rect.left() + rad, rect.right() - rad);
            let seg = (x1 - x0) * 0.22;
            let x = x0 + ((tm * 0.9).fract()) * (x1 - x0 + seg) - seg;
            let y = rect.bottom() - 1.5;
            p.line_segment([pos2(x.max(x0), y), pos2((x + seg).min(x1), y)], Stroke::new(2.0, glass(0.85)));
            ctx.request_repaint();
        }
        if bg.clicked() {
            self.omni.focus = true;
        }

        // Security badge. While the user types something else, it stops
        // describing the current page (like Chrome's search icon).
        let current = crate::app::display_url(&self.active_tab().map(|t| t.url()).unwrap_or_default());
        let typing_other = self.omni.editing && !self.omni.text.is_empty() && self.omni.text != current;
        let (label, color, icon) = if typing_other { ("SEARCH", glass(0.75), Icon::Search) } else { match self.security() {
            Security::Secured { dev: true } => ("SECURED · DEV", SECURE, Icon::Lock),
            Security::Secured { dev: false } => ("SECURED", SECURE, Icon::Lock),
            Security::Connecting => ("VERIFYING", glass(0.75), Icon::Lock),
            Security::Failed => ("BLOCKED", DANGER, Icon::Unlock),
            Security::Unsecured("Servo") => ("UNSECURED · SERVO", WARN, Icon::Unlock),
            Security::Unsecured(_) => ("UNSECURED · WEBKIT", WARN, Icon::Unlock),
            Security::Unencrypted("Servo") => ("NOT ENCRYPTED · SERVO", DANGER, Icon::Unlock),
            Security::Unencrypted(_) => ("NOT ENCRYPTED · WEBKIT", DANGER, Icon::Unlock),
            Security::Internal => ("INERT", glass(0.75), Icon::Spark),
        } };
        let galley = ui.painter().layout_no_wrap(label.to_owned(), FontId::new(10.0, mono()), color);
        let bw = galley.size().x + 32.0;
        let badge = Rect::from_min_size(rect.min + vec2(5.0, 5.0), vec2(bw, rect.height() - 10.0));
        let bresp = ui.interact(badge, Id::unique("sec-badge"), Sense::click()).on_hover_text("Site security");
        crate::drive::reg("badge", badge);
        let bh = ctx.animate_bool_with_time(Id::unique("badge-hover"), bresp.hovered(), 0.12);
        ui.painter().rect_filled(badge, badge.height() / 2.0, color.gamma_multiply(0.12 + 0.10 * bh));
        icons::paint(ui.painter(), Rect::from_center_size(pos2(badge.left() + 14.0, badge.center().y), Vec2::splat(12.0)), icon, color);
        ui.painter().galley(pos2(badge.left() + 24.0, badge.center().y - galley.size().y / 2.0), galley, color);
        if bresp.clicked() {
            self.open_popover(Popover::Security, bresp.rect);
        }

        // Text field, vertically centered on the pill: a rect exactly one
        // text row tall, with no inner margin.
        let font = FontId::new(13.5, sans());
        let row_h = ui.fonts_mut(|fo| fo.row_height(&font));
        // Saved passwords for this site: a key at the right end (Chrome's).
        let here = self.logins_here().len();
        let mut right_pad = 16.0;
        if here > 0 && !typing_other {
            let kr = Rect::from_center_size(pos2(rect.right() - 20.0, rect.center().y), Vec2::splat(26.0));
            let k = ui.interact(kr, Id::unique("omni-key"), Sense::click()).on_hover_text("Saved passwords for this site");
            crate::drive::reg("omni:key", kr);
            if k.hovered() {
                ui.painter().circle_filled(kr.center(), 13.0, glass(0.14));
            }
            icons::paint(ui.painter(), kr.shrink(6.0), Icon::Key, WHITE);
            if k.clicked() {
                self.open_popover(Popover::Passwords, kr);
            }
            right_pad = 40.0;
        }
        let text_rect = Rect::from_min_max(pos2(badge.right() + 10.0, rect.center().y - row_h / 2.0), pos2(rect.right() - right_pad, rect.center().y + row_h / 2.0));
        crate::drive::reg("omnibox", Rect::from_min_max(pos2(text_rect.left(), rect.top()), pos2(text_rect.right(), rect.bottom())));
        if self.omni.focus {
            ui.memory_mut(|m| m.request_focus(id));
            self.omni.focus = false;
            self.omni.select_all = true;
        }
        let before = self.omni.text.clone();
        // Unfocused, the address reads like Chrome's: the host stands out.
        let mut layouter = move |ui: &Ui, text: &dyn egui::TextBuffer, _w: f32| -> std::sync::Arc<egui::Galley> {
            let s = text.as_str();
            let mut job = egui::text::LayoutJob::default();
            let fmt = |c: Color32| egui::TextFormat::simple(FontId::new(13.5, sans()), c);
            match (focused, s.split_once("://")) {
                (false, Some((scheme, rest))) => {
                    let host_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
                    job.append(&format!("{scheme}://"), 0.0, fmt(glass(0.5)));
                    job.append(&rest[..host_end], 0.0, fmt(WHITE));
                    job.append(&rest[host_end..], 0.0, fmt(glass(0.7)));
                }
                _ => job.append(s, 0.0, fmt(WHITE)),
            }
            ui.fonts_mut(|fo| fo.layout_job(job))
        };
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(text_rect).layout(Layout::left_to_right(Align::Center)));
        let out = egui::TextEdit::singleline(&mut self.omni.text)
            .id(id)
            .frame(egui::Frame::NONE)
            .margin(egui::Margin::ZERO)
            .font(font)
            .text_color(WHITE)
            .layouter(&mut layouter)
            .hint_text(RichText::new("Search or enter an address").color(glass(0.5)))
            .desired_width(text_rect.width())
            .show(&mut child);
        let resp = out.response.clone();
        if resp.gained_focus() {
            self.omni.editing = true;
            self.omni.select_all = true;
        }
        // Select everything once the click that focused the box is over
        // (the text box moves the cursor on press and release).
        let pointer_busy = ui.input(|i| i.pointer.any_down() || i.pointer.any_released());
        if self.omni.select_all && ui.memory(|m| m.has_focus(id)) && !pointer_busy {
            let mut st = out.state.clone();
            st.cursor.set_char_range(Some(egui::text::CCursorRange::two(egui::text::CCursor::new(0), egui::text::CCursor::new(self.omni.text.chars().count()))));
            st.store(ui.ctx(), id);
            self.omni.select_all = false;
        }
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
        if ui.memory(|m| m.has_focus(id)) {
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
        if self.omni.text != before && ui.memory(|m| m.has_focus(id)) {
            self.on_omni_typed(ui, id, &before, &out);
        }
        if escape && self.omni.editing {
            self.omni.editing = false;
            self.omni.sugg.clear();
            self.omni.text = crate::app::display_url(&self.active_tab().map(|t| t.url()).unwrap_or_default());
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

    pub fn accept_suggestion(&mut self, s: omnibox::Suggestion) {
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
            ctx.animate_bool_with_time(Id::unique("omni-dd-anim"), false, 0.0);
            return;
        }
        let t = ctx.animate_bool_with_time(Id::unique("omni-dd-anim"), true, 0.14);
        let Some(r) = ctx.data(|d| d.get_temp::<Rect>(Id::unique("omni-rect"))) else { return };
        let rows = rows_of(&self.omni);
        let (area, clicked) = suggestion_popup(ctx, "omni-dropdown", r, &rows, self.omni.sel, t);
        ctx.data_mut(|d| d.insert_temp(Id::unique("omni-dd"), area));
        self.overlay_rects.push(area);
        if let Some(s) = clicked {
            self.omni.editing = false;
            self.omni.sugg.clear();
            self.omni.remote.clear();
            ctx.memory_mut(|m| m.surrender_focus(Id::unique("omnibox")));
            self.accept_suggestion(s);
        }
    }

    /// Suggestions under the new tab page's own search box.
    pub fn ntp_dropdown(&mut self, ctx: &egui::Context) {
        let on_ntp = matches!(self.active_tab().map(|t| &t.kind), Some(Kind::Page(Page::NewTab)));
        if !on_ntp || !self.ntp.editing || (self.ntp.sugg.is_empty() && self.ntp.remote.is_empty()) {
            ctx.data_mut(|d| d.remove::<Rect>(Id::unique("ntp-dd")));
            ctx.animate_bool_with_time(Id::unique("ntp-dd-anim"), false, 0.0);
            return;
        }
        let t = ctx.animate_bool_with_time(Id::unique("ntp-dd-anim"), true, 0.14);
        let Some(r) = ctx.data(|d| d.get_temp::<Rect>(Id::unique("ntp-rect"))) else { return };
        let rows = rows_of(&self.ntp);
        let (area, clicked) = suggestion_popup(ctx, "ntp-dropdown", r, &rows, self.ntp.sel, t);
        ctx.data_mut(|d| d.insert_temp(Id::unique("ntp-dd"), area));
        if let Some(s) = clicked {
            self.ntp = Default::default();
            ctx.memory_mut(|m| m.surrender_focus(Id::unique("ntp-search")));
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
                let icon_c = if inert { WHITE } else { glass(0.6) };
                let (rect, resp) = {
                    let g = ui.painter().layout_no_wrap(b.title.clone(), FontId::new(12.0, sans()), WHITE);
                    let w = g.size().x.min(150.0) + 30.0;
                    ui.allocate_exact_size(vec2(w, 22.0), Sense::click())
                };
                let bh = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.12);
                if bh > 0.0 {
                    ui.painter().rect_filled(rect, 11.0, glass(0.12 * bh));
                }
                icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), Vec2::splat(11.0)), if inert { Icon::Lock } else { Icon::Globe }, icon_c);
                let mut job = egui::text::LayoutJob::single_section(b.title.clone(), egui::TextFormat::simple(FontId::new(12.0, sans()), glass(0.82 + 0.18 * bh)));
                job.wrap = egui::text::TextWrapping::truncate_at_width(150.0);
                let g = ui.fonts_mut(|f| f.layout_job(job));
                ui.painter().galley(pos2(rect.left() + 22.0, rect.center().y - g.size().y / 2.0), g, WHITE);
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
                if ui.add(egui::Label::new(theme::label_c("All bookmarks", glass(0.78))).sense(Sense::click())).clicked() {
                    self.add_tab("about:bookmarks", None, true, None);
                }
            });
        });
    }

    fn find_bar(&mut self, ui: &mut Ui) {
        egui::Frame::new().fill(well(0.55)).stroke(Stroke::new(1.0, glass(0.12))).corner_radius(19).inner_margin(egui::Margin::symmetric(14, 4)).show(ui, |ui| {
          ui.set_width(ui.available_width());
          ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
            ui.label(theme::label_c("Find in page", glass(0.8)));
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
                ui.label(RichText::new(format!("{n} matches on this screen")).font(FontId::new(13.0, sans())).color(glass(0.8)));
            }
            self.find = Some(q);
            if ui.button("Done").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.find = None;
            }
          });
        });
    }

    // ------------------------------------------------------------------ popovers

    fn popovers(&mut self, ctx: &egui::Context) {
        if self.popover == Popover::None {
            ctx.animate_bool_with_time(Id::unique("popover-anim"), false, 0.0);
            return;
        }
        let t = ctx.animate_bool_with_time(Id::unique("popover-anim"), true, 0.14);
        let anchor = ctx.data(|d| d.get_temp::<Rect>(Id::unique("pop_anchor"))).unwrap_or(Rect::from_min_size(pos2(100.0, 80.0), Vec2::ZERO));
        let width = match self.popover {
            Popover::Vpn => 340.0,
            Popover::Menu => 240.0,
            _ => 360.0,
        };
        let screen = ctx.content_rect();
        // Open toward the middle of the window: right-aligned from the
        // toolbar's right end, upward from the sidebar's foot.
        let from_left = anchor.center().x < screen.center().x;
        let x = if from_left { anchor.left() } else { anchor.right() - width }.clamp(8.0, (screen.right() - width - 8.0).max(8.0));
        let up = anchor.center().y > screen.center().y;
        let slide = 8.0 * (1.0 - ease(t));
        let (pos, pivot) = if up { (pos2(x, anchor.top() - 8.0 + slide), egui::Align2::LEFT_BOTTOM) } else { (pos2(x, anchor.bottom() + 8.0 - slide), egui::Align2::LEFT_TOP) };
        let which = self.popover;
        let area = egui::Area::new(Id::unique("popover")).pivot(pivot).fixed_pos(pos).order(egui::Order::Foreground).show(ctx, |ui| {
            ui.multiply_opacity(t);
            theme::panel_frame(18, 16.0).show(ui, |ui| {
                ui.set_width(width - 32.0);
                match which {
                    Popover::Security => self.pop_security(ui),
                    Popover::Vpn => self.pop_vpn(ui),
                    Popover::Shield => self.pop_shield(ui),
                    Popover::Cookies => self.pop_cookies(ui),
                    Popover::Passwords => self.pop_passwords(ui),
                    Popover::Menu => self.pop_menu(ui),
                    Popover::GroupAdd(g) => self.pop_group_add(ui, g),
                    Popover::None => {}
                }
            });
        });
        // A popover that opens upward is placed by its size from the frame
        // before. When its content grows or shrinks, lay this frame out again
        // so it never jumps.
        let size = area.response.rect.size();
        let last: Option<Vec2> = ctx.data(|d| d.get_temp(Id::unique("pop_size")));
        ctx.data_mut(|d| d.insert_temp(Id::unique("pop_size"), size));
        if up && last.is_some_and(|l| (l.y - size.y).abs() > 0.5) {
            ctx.request_discard("popover height changed");
        }
        self.overlay_rects.push(area.response.rect);
        let outside = ctx.input(|i| i.pointer.any_pressed()) && ctx.pointer_latest_pos().is_some_and(|p| !area.response.rect.contains(p));
        let fresh = self.popover_opened.is_some_and(|t| t.elapsed() < Duration::from_millis(150));
        let in_menu = egui::Popup::is_any_open(ctx);
        if (outside && !fresh && !in_menu) || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            ctx.data_mut(|d| d.insert_temp(Id::unique("pop_closed_at"), (Instant::now(), self.popover)));
            self.popover = Popover::None;
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
                    ui.label(theme::title(&host, 24.0));
                    ui.add_space(8.0);
                    for line in [
                        "The manifest is signed and matches the pinned key.",
                        "Every screen was checked against the signed template.",
                        "No code from this site runs on your device.",
                        "TLS 1.3 with post-quantum hybrid key exchange.",
                    ] {
                        ui.horizontal(|ui| {
                            let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                            icons::paint(ui.painter(), r, Icon::Check, OK);
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
                Security::Unencrypted(_) => {
                    ui.label(theme::label_c("○ Not encrypted", DANGER));
                    ui.add_space(4.0);
                    ui.label(theme::body("This page is plain HTTP: anyone on the network can read or change it, and it runs code from the site."));
                    self.engine_switch(ui);
                }
                _ => {
                    ui.label(theme::label_c("○ Unsecured", WARN));
                    ui.add_space(6.0);
                    ui.label(theme::title("Ordinary web page", 22.0));
                    ui.add_space(6.0);
                    ui.label(theme::body("The connection uses HTTPS, but the page runs the site's own code inside WebKit's sandbox. Inert cannot verify what it shows."));
                    ui.add_space(6.0);
                    let shield = match &self.ad.state {
                        adblock::State::Ready(n) if self.profile.settings.adblock => format!("Shield is blocking ads & trackers ({n} rules)."),
                        _ => "Shield is not active on this page.".into(),
                    };
                    ui.label(theme::muted(&shield));
                    ui.label(theme::muted("The assistant treats this page's text as untrusted."));
                    self.engine_switch(ui);
                }
            },
        }
    }

    fn engine_switch(&mut self, ui: &mut Ui) {
        let id = self.active;
        let Some(Kind::Web(w)) = self.tab(id).map(|t| &t.kind) else { return };
        let cur = w.engine.unwrap_or(self.profile.settings.web_engine);
        ui.add_space(8.0);
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(theme::label("Engine"));
            ui.label(theme::mono_text(if cur == crate::profile::Engine::Servo { "Servo (Rust)" } else { "WebKit (system)" }, 12.0));
        });
        let other = cur.other();
        let b = ui.button(format!("Open this page with {}", other.name()));
        crate::drive::reg("engine:switch", b.rect);
        if b.clicked() {
            if let Some(Kind::Web(w)) = self.tab_mut(id).map(|t| &mut t.kind) {
                w.discard();
                w.engine = Some(other);
            }
            self.popover = Popover::None;
        }
    }

    fn pop_vpn(&mut self, ui: &mut Ui) {
        let st = self.vpn.status.clone();
        // Header: the icon sits on the label's line (it used to float above it).
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
            icons::paint(ui.painter(), r, Icon::Vpn, if st.connected() { OK } else { MUTED });
            ui.label(theme::label("Native VPN · Mullvad"));
        });
        ui.add_space(8.0);
        if !st.installed {
            self.vpn_setup(ui);
            return;
        }
        if st.logged_in == Some(false) {
            self.vpn_login(ui);
            return;
        }
        let headline = if st.connected() { format!("{}, {}", st.city, st.country) } else if st.busy() { "Connecting…".into() } else { "Not connected".into() };
        ui.label(theme::title(&headline, 26.0));
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
        let b = if st.connected() { theme::cta_ghost("Disconnect", 42.0) } else { theme::cta("Connect", BLUE, WHITE, 42.0) }.min_size(vec2(ui.available_width(), 42.0));
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
        ui.add_space(6.0);
        ui.separator();
        if self.vpn.confirm_logout {
            ui.label(theme::body("Log out of Mullvad? The VPN disconnects and this Mac is removed from your account. You'll need your account number to sign back in."));
            ui.horizontal(|ui| {
                let b = ui.add(theme::cta("Log out", DANGER.gamma_multiply(0.14), DANGER, 34.0).min_size(vec2(110.0, 34.0)));
                crate::drive::reg("vpn:logout-confirm", b.rect);
                if b.clicked() {
                    self.vpn.confirm_logout = false;
                    self.vpn.status.state = "disconnecting".into();
                    vpn::exec(vpn::Cmd::Logout, self.demo, self.tx.clone(), self.ctx.clone());
                }
                if ui.add(theme::cta_ghost("Cancel", 34.0)).clicked() {
                    self.vpn.confirm_logout = false;
                }
            });
        } else {
            let r = ui.horizontal(|ui| {
                let (r, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 28.0), Sense::click());
                if resp.hovered() {
                    ui.painter().rect_filled(r, 8.0, HOVER);
                }
                icons::paint(ui.painter(), Rect::from_center_size(pos2(r.left() + 12.0, r.center().y), Vec2::splat(13.0)), Icon::Logout, MUTED);
                ui.painter().text(pos2(r.left() + 28.0, r.center().y), egui::Align2::LEFT_CENTER, "Log out of Mullvad", FontId::new(13.0, sans()), FG);
                resp
            }).inner;
            crate::drive::reg("vpn:logout", r.rect);
            if r.clicked() {
                self.vpn.confirm_logout = true;
            }
        }
    }

    /// Mullvad isn't installed: how to set it up.
    fn vpn_setup(&mut self, ui: &mut Ui) {
        ui.label(theme::title("Set up the VPN", 24.0));
        ui.label(theme::body("Inert's VPN is Mullvad's own WireGuard tunnel, dressed in Inert. Three steps:"));
        ui.add_space(6.0);
        for (n, t) in [("1", "Install the Mullvad app from mullvad.net."), ("2", "Open it once so its background service starts."), ("3", "Come back and type your account number.")] {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(Vec2::splat(20.0), Sense::hover());
                ui.painter().circle_filled(r.center(), 10.0, BLUE);
                ui.painter().text(r.center(), egui::Align2::CENTER_CENTER, n, FontId::new(11.0, mono()), WHITE);
                ui.label(theme::body(t));
            });
        }
        ui.add_space(8.0);
        let b = ui.add(theme::cta("Download Mullvad", BLUE, WHITE, 42.0).min_size(vec2(ui.available_width(), 42.0)));
        if b.clicked() {
            self.add_tab("https://mullvad.net/download", None, true, None);
            self.popover = Popover::None;
        }
    }

    /// Mullvad is installed but signed out: the account number is all it takes.
    fn vpn_login(&mut self, ui: &mut Ui) {
        ui.label(theme::title("Sign in to Mullvad", 24.0));
        ui.label(theme::body("Type your 16-digit Mullvad account number. That's all — no email, no password."));
        ui.add_space(8.0);
        ui.label(theme::label("Account number"));
        let busy = self.vpn.login_busy;
        let r = ui.add_enabled(
            !busy,
            egui::TextEdit::singleline(&mut self.vpn.account_input)
                .password(!self.vpn.show_account)
                .hint_text("0000 0000 0000 0000")
                .font(FontId::new(16.0, mono()))
                .desired_width(ui.available_width())
                .margin(vec2(12.0, 10.0)),
        );
        crate::drive::reg("vpn:account", r.rect);
        let mut show = self.vpn.show_account;
        if ui.checkbox(&mut show, "Show number").changed() {
            self.vpn.show_account = show;
        }
        if let Some(e) = &self.vpn.login_error {
            ui.label(RichText::new(e).font(FontId::new(12.0, mono())).color(DANGER));
        }
        ui.add_space(6.0);
        let label = if busy { "Signing in…" } else { "Log in" };
        let b = ui.add_enabled(!busy, theme::cta(label, BLUE, WHITE, 42.0).min_size(vec2(ui.available_width(), 42.0)));
        crate::drive::reg("vpn:login", b.rect);
        let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (b.clicked() || enter) && !busy {
            match vpn::clean_account(&self.vpn.account_input) {
                Some(n) => {
                    self.vpn.account_input.clear();
                    self.vpn.login_error = None;
                    self.vpn.login_busy = true;
                    vpn::exec(vpn::Cmd::Login(n), self.demo, self.tx.clone(), self.ctx.clone());
                }
                None => self.vpn.login_error = Some("An account number is 16 digits.".into()),
            }
        }
        ui.add_space(8.0);
        ui.label(theme::muted("The number goes straight to the Mullvad app on this Mac. Inert never shows or stores it."));
        if ui.add(egui::Label::new(theme::label_c("No account? Get one at mullvad.net →", SECURE)).sense(Sense::click())).clicked() {
            self.add_tab("https://mullvad.net/account/create", None, true, None);
            self.popover = Popover::None;
        }
    }

    /// Saved logins for the active page's origin (exact origin, like Chrome).
    pub fn logins_here(&self) -> Vec<crate::profile::LoginRef> {
        let Some(Kind::Web(w)) = self.active_tab().map(|t| &t.kind) else { return Vec::new() };
        let Some(o) = crate::passwords::origin_of(&w.url) else { return Vec::new() };
        self.profile.logins.iter().filter(|l| l.origin == o).cloned().collect()
    }

    fn pop_passwords(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
            icons::paint(ui.painter(), r, Icon::Key, ACCENT);
            ui.label(theme::label("Saved passwords"));
        });
        ui.add_space(6.0);
        let list = self.logins_here();
        if let Some(l) = list.first() {
            ui.label(theme::title(l.origin.trim_start_matches("https://"), 22.0));
        }
        ui.add_space(6.0);
        for (i, l) in list.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(theme::body(if l.username.is_empty() { "(no username)" } else { &l.username }));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let b = ui.add(theme::cta("Fill", BLUE, WHITE, 30.0).min_size(vec2(64.0, 30.0)));
                    crate::drive::reg(format!("pw:fill:{i}"), b.rect);
                    if b.clicked() {
                        match crate::passwords::get(&l.origin, &l.username, self.demo) {
                            Some(pw) => {
                                if let Some(Kind::Web(w)) = self.active_tab().map(|t| &t.kind) {
                                    crate::web::js(w, &crate::passwords::fill_js(&l.username, &pw));
                                }
                                self.popover = Popover::None;
                            }
                            None => self.toast("That password is no longer in the Keychain"),
                        }
                    }
                });
            });
        }
        ui.add_space(6.0);
        ui.label(theme::muted("Filled only on this exact site, only when you click. Passwords stay in the macOS Keychain."));
    }

    fn pop_cookies(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
            icons::paint(ui.painter(), r, Icon::Cookie, ACCENT);
            ui.label(theme::label("Cookies · extension"));
        });
        ui.add_space(6.0);
        let id = self.active;
        let web = matches!(self.tab(id).map(|t| &t.kind), Some(Kind::Web(_)));
        if !web {
            ui.label(theme::title("No cookies here", 24.0));
            ui.label(theme::body("inert:// sites and Inert's own pages don't use cookies: there is no script to set them, and sessions are part of the signed protocol."));
        } else {
            let host = self.cookie.host.clone();
            let site = crate::cookies::site_of(&host);
            ui.label(theme::title(&host, 24.0));
            let n = self.cookie.list.len();
            let consent = self.cookie.consent.get(&id).cloned().unwrap_or_default();
            ui.label(theme::muted(&if self.cookie.loading { "Reading cookies…".to_owned() } else { format!("{n} cookie{} for this site", if n == 1 { "" } else { "s" }) }));
            if consent == "rejected" {
                ui.label(theme::label_c("✓ Cookie banner rejected automatically", OK));
            }
            ui.add_space(6.0);
            let servo = matches!(self.tab(id).map(|t| &t.kind), Some(Kind::Web(w)) if w.sv.is_some());
            let list = self.cookie.list.clone();
            let mut del = None;
            egui::ScrollArea::vertical().max_height(210.0).show(ui, |ui| {
                for c in &list {
                    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 38.0), Sense::hover());
                    if resp.hovered() {
                        ui.painter().rect_filled(rect, 8.0, HOVER);
                    }
                    let mut job = egui::text::LayoutJob::single_section(c.name.clone(), egui::TextFormat::simple(FontId::new(12.0, mono()), FG));
                    job.wrap = egui::text::TextWrapping::truncate_at_width(rect.width() - 40.0);
                    let g = ui.fonts_mut(|f| f.layout_job(job));
                    ui.painter().galley(pos2(rect.left() + 8.0, rect.top() + 4.0), g, FG);
                    let exp = match c.expires {
                        None => "session".to_owned(),
                        Some(t) => format!("until {}", crate::when::date(t.max(0) as u64)),
                    };
                    let flags = format!("{} · {exp}{}{}", c.domain, if c.secure { " · Secure" } else { "" }, if c.http_only { " · HttpOnly" } else { "" });
                    ui.painter().text(pos2(rect.left() + 8.0, rect.bottom() - 5.0), egui::Align2::LEFT_BOTTOM, flags, FontId::new(10.5, mono()), DIM);
                    if !servo && resp.hovered() {
                        let cr = Rect::from_center_size(pos2(rect.right() - 16.0, rect.center().y), Vec2::splat(20.0));
                        let x = ui.interact(cr, Id::unique(("cookie-x", &c.name, &c.domain)), Sense::click()).on_hover_text("Delete this cookie");
                        icons::paint(ui.painter(), cr.shrink(5.0), Icon::Close, if x.hovered() { WHITE } else { MUTED });
                        if x.clicked() {
                            del = Some(c.clone());
                        }
                    }
                }
            });
            if let Some(c) = del {
                self.cookie_delete(&c);
            }
            ui.add_space(8.0);
            let b = ui.add(theme::cta_ghost(&format!("Delete cookies for {site}"), 38.0).min_size(vec2(ui.available_width(), 38.0)));
            crate::drive::reg("cookies:clear-site", b.rect);
            if b.clicked() {
                self.cookies_clear_site(&site);
                self.toast(format!("Deleted cookies and site data for {site}"));
            }
            ui.add_space(4.0);
            let mut keep_none = self.profile.settings.cookie_block.contains(&site);
            let r = ui.checkbox(&mut keep_none, format!("Keep no cookies for {site}"));
            crate::drive::reg("cookies:block", r.rect);
            if r.changed() {
                if keep_none {
                    self.profile.settings.cookie_block.push(site.clone());
                    self.cookies_clear_site(&site);
                } else {
                    self.profile.settings.cookie_block.retain(|s| *s != site);
                }
                self.dirty = true;
            }
        }
        ui.add_space(4.0);
        let mut on = self.profile.settings.cookie_banners;
        let r = ui.checkbox(&mut on, "Reject cookie banners automatically");
        crate::drive::reg("cookies:banners", r.rect);
        if r.changed() {
            self.profile.settings.cookie_banners = on;
            self.dirty = true;
        }
        ui.label(theme::muted("Values are never shown. Manage every site's cookies in Settings → Extensions."));
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
        ui.label(theme::title(&h, 24.0));
        ui.label(theme::body(&d));
        if let Some(h) = &self.servo {
            let (n, ready) = h.shield();
            ui.label(theme::body(&if ready { format!("Servo tabs: {n} requests blocked this session.") } else { "Servo tabs: loading the blocking engine…".to_owned() }));
        }
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
        let item = |ui: &mut Ui, icon: Icon, text: &str, key: &str| -> bool {
            let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 32.0), Sense::click());
            crate::drive::reg(format!("menu:{text}"), rect);
            let h = ui.ctx().animate_bool_with_time(resp.id.with("h"), resp.hovered(), 0.10);
            if h > 0.0 {
                ui.painter().rect_filled(rect, 10.0, HOVER.gamma_multiply(h));
            }
            icons::paint(ui.painter(), Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), Vec2::splat(13.0)), icon, MUTED);
            ui.painter().text(pos2(rect.left() + 32.0, rect.center().y), egui::Align2::LEFT_CENTER, text, FontId::new(13.0, sans()), FG);
            ui.painter().text(pos2(rect.right() - 8.0, rect.center().y), egui::Align2::RIGHT_CENTER, key, FontId::new(11.0, mono()), DIM);
            resp.clicked()
        };
        if item(ui, Icon::Tab, "New tab", "Cmd+T") {
            self.add_tab("about:newtab", None, true, None);
            self.popover = Popover::None;
        }
        if item(ui, Icon::Layers, "New tab group", "") {
            let id = self.add_tab("about:newtab", None, true, None);
            self.group_new(id, "New group");
            self.popover = Popover::None;
        }
        if item(ui, Icon::History, "History", "Cmd+Y") {
            self.add_tab("about:history", None, true, None);
            self.popover = Popover::None;
        }
        if item(ui, Icon::Bookmark, "Bookmarks", "Opt+Cmd+B") {
            self.add_tab("about:bookmarks", None, true, None);
            self.popover = Popover::None;
        }
        if item(ui, Icon::Import, "Import from Chrome…", "") {
            self.add_tab("about:import", None, true, None);
            self.popover = Popover::None;
        }
        if item(ui, Icon::Folder, "Downloads", "") {
            self.add_tab("about:downloads", None, true, None);
            self.popover = Popover::None;
        }
        if item(ui, Icon::Spark, "Assistant", "Cmd+J") {
            self.ai.open = !self.ai.open;
            self.popover = Popover::None;
        }
        if item(ui, Icon::Search, "Find in page", "Cmd+F") {
            self.find = Some(String::new());
            self.popover = Popover::None;
        }
        if item(ui, Icon::Code, "Developer tools", "Opt+Cmd+I") {
            self.devtools();
            self.popover = Popover::None;
        }
        if item(ui, Icon::Gear, "Settings", "Cmd+,") {
            self.add_tab("about:settings", None, true, None);
            self.popover = Popover::None;
        }
    }

    pub fn devtools(&mut self) {
        match self.active_tab().map(|t| &t.kind) {
            Some(Kind::Web(w)) => {
                if let Some(v) = &w.view {
                    v.open_devtools();
                } else if w.sv.is_some() {
                    self.toast("Servo has no built-in inspector yet. Open the page with WebKit (site security panel) to inspect it.");
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
            theme::panel_frame(18, 16.0).show(ui, |ui| {
                ui.set_width(300.0);
                ui.label(theme::label("Name this group"));
                let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(290.0).font(FontId::new(15.0, sans())));
                // Check Enter before re-requesting focus: requesting focus clears `lost_focus`.
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    done = true;
                } else if !ui.memory(|m| m.has_focus(r.id)) && !ui.input(|i| i.pointer.any_down()) {
                    r.request_focus();
                }
                ui.horizontal(|ui| {
                    if ui.add(theme::cta("Save", BLUE, WHITE, 34.0)).clicked() {
                        done = true;
                    }
                    if ui.add(theme::cta_ghost("Cancel", 34.0)).clicked() {
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
        let Some((t, at)) = &self.toast else {
            ctx.animate_bool_with_time(Id::unique("toast-in"), false, 0.0);
            return;
        };
        // Rises in, and fades out before it is dropped.
        let left = 4.0 - at.elapsed().as_secs_f32();
        let a = ease(ctx.animate_bool_with_time(Id::unique("toast-in"), true, 0.22)) * (left / 0.3).clamp(0.0, 1.0);
        let r = self.content_rect;
        egui::Area::new(Id::unique("toast")).pivot(egui::Align2::CENTER_BOTTOM).fixed_pos(pos2(r.center().x, r.bottom() - 18.0 + 10.0 * (1.0 - a))).order(egui::Order::Tooltip).interactable(false).show(ctx, |ui| {
            ui.multiply_opacity(a);
            egui::Frame::new().fill(RAISED).stroke(Stroke::new(1.0, glass(0.14))).corner_radius(20).inner_margin(egui::Margin::symmetric(18, 10)).shadow(theme::shadow()).show(ui, |ui| {
                ui.label(RichText::new(t).font(FontId::new(13.0, sans())).color(FG));
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
        egui::Frame::new().fill(ctx_color.gamma_multiply(0.10)).corner_radius(12).inner_margin(egui::Margin::symmetric(10, 5)).show(ui, |ui| {
            ui.label(RichText::new(ctx_label).font(FontId::new(11.0, mono())).color(ctx_color));
        });
        ui.add_space(8.0);
        let input_h = 96.0;
        let avail = ui.available_height() - input_h;
        egui::ScrollArea::vertical().max_height(avail).stick_to_bottom(true).auto_shrink([false, false]).show(ui, |ui| {
            if self.ai.msgs.is_empty() {
                ui.add_space(20.0);
                ui.label(theme::title("Ask about this page.", 26.0));
                ui.add_space(6.0);
                ui.label(theme::muted("Inert pages are read from their signed structure. Web pages are read as untrusted text — instructions hidden in them are ignored."));
                ui.add_space(10.0);
                for s in ["Summarize this page", "What can I do here?", "Is anything on this page unusual?"] {
                    if ui.add(theme::cta_ghost(s, 32.0)).clicked() {
                        self.ask_ai(s.into());
                    }
                }
            }
            for m in &self.ai.msgs {
                if m.user {
                    ui.with_layout(Layout::top_down(Align::Max), |ui| {
                        egui::Frame::new().fill(BLUE).corner_radius(egui::CornerRadius { nw: 16, ne: 16, sw: 16, se: 4 }).inner_margin(egui::Margin::symmetric(12, 9)).show(ui, |ui| {
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
        crate::drive::reg("ai:input", r.rect);
        let send = ui.memory(|m| m.has_focus(r.id)) && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
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
            ui.label(theme::muted("Open an inert:// tab to inspect its manifest. Web tabs use WebKit's developer tools (Opt+Cmd+I)."));
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
        // A page fades in when its tab comes forward.
        let ctx = ui.ctx().clone();
        let now = ctx.input(|i| i.time);
        if ctx.data(|d| d.get_temp::<u64>(Id::unique("shown-tab"))) != Some(id) {
            ctx.data_mut(|d| {
                d.insert_temp(Id::unique("shown-tab"), id);
                d.insert_temp(Id::unique("shown-at"), now);
            });
        }
        let k = ease(((now - ctx.data(|d| d.get_temp::<f64>(Id::unique("shown-at"))).unwrap_or(0.0)) / 0.22) as f32);
        if k < 1.0 {
            ui.multiply_opacity(0.25 + 0.75 * k);
            ctx.request_repaint();
        }
        let kind_tag = match self.tab(id).map(|t| &t.kind) {
            Some(Kind::Page(p)) => Some(*p),
            _ => None,
        };
        if let Some(p) = kind_tag {
            self.page(ui, p);
            return;
        }
        // Servo-backed web pages are drawn by Inert itself.
        let overlay = self.popover != Popover::None || (self.omni.editing && !self.omni.sugg.is_empty()) || egui::Popup::is_any_open(ui.ctx()) || self.rename.is_some();
        if let (Some(Kind::Web(w)), Some(host)) = (self.tabs.iter().find(|t| t.id == id).map(|t| &t.kind), &self.servo) {
            if let Some(sv) = &w.sv {
                crate::drive::reg("servo:surface", ui.max_rect());
                crate::servo_web::show(ui, host, id, sv, overlay);
                return;
            }
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
                    // The native WebKit view covers this area.
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
            theme::panel_frame(18, 18.0).show(ui, |ui| {
                ui.set_width(380.0);
                let web = href.starts_with("https://");
                ui.label(theme::label_c(if web { "Leaving inert:// for the web" } else { "Opening another inert:// site" }, if web { WARN } else { SECURE }));
                ui.add_space(6.0);
                ui.label(theme::mono_text(href, 12.0));
                ui.add_space(6.0);
                ui.label(theme::body(if web { "The destination is an ordinary web page. It will run its own code and is shown as unsecured." } else { "That site is verified separately with its own pinned key." }));
                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    if ui.add(theme::cta("Open in new tab", BLUE, WHITE, 36.0)).clicked() {
                        choice = Some(true);
                    }
                    if ui.add(theme::cta_ghost("Cancel", 36.0)).clicked() {
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
                (shift, Key::T, "reopen"),
                (shift, Key::B, "bookmarks-bar"),
                (alt, Key::B, "bookmarks"),
                (alt, Key::I, "devtools"),
                (Modifiers::CTRL | Modifiers::SHIFT, Key::Tab, "prev-tab"),
                (alt, Key::ArrowRight, "next-tab"),
                (alt, Key::ArrowLeft, "prev-tab"),
                (cmd, Key::S, "sidebar"),
                (cmd, Key::T, "new-tab"),
                (cmd, Key::W, "close-tab"),
                (cmd, Key::L, "focus-omnibox"),
                (cmd, Key::R, "reload"),
                (cmd, Key::OpenBracket, "back"),
                (cmd, Key::CloseBracket, "forward"),
                (cmd, Key::D, "bookmark"),
                (cmd, Key::Y, "history"),
                (cmd, Key::J, "assistant"),
                (cmd, Key::F, "find"),
                (cmd, Key::Comma, "settings"),
                (cmd, Key::Equals, "zoom-in"),
                (cmd, Key::Plus, "zoom-in"),
                (cmd, Key::Minus, "zoom-out"),
                (cmd, Key::Num0, "zoom-reset"),
                (Modifiers::CTRL, Key::Tab, "next-tab"),
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

    /// Cut / copy / paste / select all / undo / redo from the Edit menu
    /// (and their shortcuts). A focused WebKit page gets the native action;
    /// everything else, egui's fields and Servo pages included, gets egui
    /// input on the next frame.
    fn edit_command(&mut self, what: &str) {
        let egui_focus = self.ctx.memory(|m| m.focused().is_some());
        #[cfg(target_os = "macos")]
        if !egui_focus {
            if let Some(Kind::Web(w)) = self.active_tab().map(|t| &t.kind) {
                if let Some(v) = &w.view {
                    crate::web::mac::edit_action(v, what);
                    return;
                }
            }
        }
        let _ = egui_focus;
        let cmd = egui::Modifiers::COMMAND;
        let key = |k: egui::Key, m: egui::Modifiers| {
            [true, false].map(|pressed| egui::Event::Key { key: k, physical_key: Some(k), pressed, repeat: false, modifiers: m })
        };
        match what {
            "copy" => self.pending_input.push(egui::Event::Copy),
            "cut" => self.pending_input.push(egui::Event::Cut),
            "paste" => {
                #[cfg(target_os = "macos")]
                if let Some(t) = crate::web::mac::clipboard_text() {
                    self.pending_input.push(egui::Event::Paste(t));
                }
            }
            "select-all" => self.pending_input.extend(key(egui::Key::A, cmd)),
            "undo" => self.pending_input.extend(key(egui::Key::Z, cmd)),
            "redo" => self.pending_input.extend(key(egui::Key::Z, cmd | egui::Modifiers::SHIFT)),
            _ => {}
        }
        self.ctx.request_repaint();
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
            "sidebar" => self.toggle_sidebar(),
            s if s.starts_with("edit-") => self.edit_command(&s[5..]),
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
                let foot = self.ctx.content_rect().bottom() - 40.0;
                self.open_popover(Popover::Vpn, Rect::from_min_size(pos2(14.0, foot), vec2(80.0, 28.0)));
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

/// Local suggestions followed by the search engine's.
pub fn rows_of(o: &crate::app::Omni) -> Vec<omnibox::Suggestion> {
    let mut rows: Vec<omnibox::Suggestion> = o.sugg.clone();
    rows.extend(o.remote.iter().map(|q| omnibox::Suggestion { kind: SKind::Remote, title: q.clone(), target: q.clone() }));
    rows
}

/// The suggestion list under a search field (omnibox or new tab page),
/// fading and sliding in. Returns its rect and the row that was clicked.
pub fn suggestion_popup(ctx: &egui::Context, key: &str, r: Rect, rows: &[omnibox::Suggestion], selected: usize, t: f32) -> (Rect, Option<omnibox::Suggestion>) {
    let mut clicked = None;
    let area = egui::Area::new(Id::unique(key)).fixed_pos(pos2(r.left(), r.bottom() + 6.0 - 6.0 * (1.0 - ease(t)))).order(egui::Order::Foreground).show(ctx, |ui| {
        ui.multiply_opacity(t);
        theme::panel_frame(18, 6.0).show(ui, |ui| {
            ui.set_width(r.width() - 12.0);
            ui.spacing_mut().item_spacing.y = 0.0;
            for (i, s) in rows.iter().enumerate() {
                let sel = i == selected;
                let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), 36.0), Sense::click());
                crate::drive::reg(format!("{key}:{i}"), rect);
                let h = ui.ctx().animate_bool_with_time(resp.id.with("h"), sel || resp.hovered(), 0.10);
                if h > 0.0 {
                    ui.painter().rect_filled(rect, 12.0, if sel { HOVER } else { RAISED }.gamma_multiply(h));
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
                ui.painter().text(pos2(rect.right() - 10.0, rect.center().y), egui::Align2::RIGHT_CENTER, tag, FontId::new(9.5, mono()), DIM);
                if resp.clicked() {
                    clicked = Some(s.clone());
                }
            }
        });
    });
    (area.response.rect, clicked)
}

/// The rounded dark card a right-hand panel (assistant, inspector) lives in.
fn side_card(ui: &mut Ui) -> Ui {
    let r = ui.max_rect();
    let card = Rect::from_min_max(r.min, pos2(r.right() - GAP, r.bottom() - GAP));
    ui.painter().rect_filled(card, CARD_R, PANEL);
    ui.painter().rect_stroke(card, CARD_R, Stroke::new(1.0, glass(0.16)), egui::StrokeKind::Inside);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(card.shrink(16.0)).layout(Layout::top_down(Align::Min)));
    child.set_clip_rect(card);
    child
}
