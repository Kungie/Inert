//! Draws a verified `inert://` screen with the site's design tokens.
//! The renderer runs no site code: it walks the signed template and fills
//! slots with already-validated data.

use crate::inert_tab::{FormValue, Img, InertTab};
use crate::theme;
use eframe::egui::{self, Align, Color32, CornerRadius, FontFamily, FontId, Id, Layout, RichText, Sense, Stroke, Ui, vec2};
use inert_proto::*;
use std::collections::HashMap;

pub enum Act {
    Open(String, Fields),
    Submit { action: String, fields: Fields },
    External(String),
    Fetch(Hash),
    LocalError(String, String),
}

fn family(f: inert_proto::FontFamily) -> FontFamily {
    match f {
        inert_proto::FontFamily::GeistSerif => theme::serif(),
        inert_proto::FontFamily::GeistSans => theme::sans(),
        inert_proto::FontFamily::GeistMono => theme::mono(),
        inert_proto::FontFamily::GeistPixel => theme::pixel(),
    }
}

pub fn fmt_value(v: &Value) -> String {
    match v {
        Value::Text(s) => s.clone(),
        Value::Uint(_) | Value::Nint(_) => v.as_i64().map(|n| n.to_string()).unwrap_or_default(),
        Value::Array(_) => Decimal::from_value(v).map(|d| d.to_string()).unwrap_or_default(),
        Value::Bool(b) => if *b { "yes".into() } else { "no".into() },
        _ => String::new(),
    }
}

struct R<'a> {
    m: &'a Manifest,
    d: &'a Design,
    data: &'a Fields,
    forms: &'a mut HashMap<(String, String), FormValue>,
    errors: &'a HashMap<String, String>,
    images: &'a HashMap<Hash, Img>,
    acts: Vec<Act>,
    z: f32,
    busy: bool,
    in_row: bool,
    /// The width of the row being laid out, so fields in it can share it.
    row_w: f32,
}

impl R<'_> {
    fn c(&self, c: Rgb) -> Color32 {
        theme::rgb(c)
    }
    fn font(&self, s: TextStyle) -> FontId {
        FontId::new(self.d.size(s) as f32 * self.z, family(self.d.font(s)))
    }
    fn gap(&self, s: Spacing) -> f32 {
        self.d.space(s) as f32 * self.z
    }
    fn get(&self, path: &str, item: Option<&Value>) -> Option<Value> {
        if let Some(rest) = path.strip_prefix("item.") { item.and_then(|i| i.get(rest)).cloned() } else { self.data.get(path).cloned() }
    }
    fn arg(&self, a: &ArgSrc, item: Option<&Value>) -> Option<Value> {
        match a {
            ArgSrc::Lit(v) => Some(v.clone()),
            ArgSrc::Bind(p) => self.get(p, item),
        }
    }
}

/// Renders the tab's current screen. Returns what the user asked for.
pub fn render(ui: &mut Ui, tab: &mut InertTab) -> Vec<Act> {
    let (Some(m), Some(s)) = (tab.manifest.clone(), tab.screen.clone()) else { return Vec::new() };
    let Some(def) = m.screens.get(&s.screen) else { return Vec::new() };
    let d = &m.design;
    let bg = theme::rgb(d.colors.bg);
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, bg);
    let mut r = R { m: &m, d, data: &s.data, forms: &mut tab.forms, errors: &tab.field_errors, images: &tab.images, acts: Vec::new(), z: tab.zoom, busy: tab.busy, in_row: false, row_w: 0.0 };

    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let avail = ui.available_width();
        let content_w = (760.0 * r.z).min(avail - 48.0).max(280.0);
        let side = ((avail - content_w) / 2.0).max(24.0);
        ui.add_space(48.0 * r.z);
        ui.horizontal(|ui| {
            ui.add_space(side);
            ui.vertical(|ui| {
                ui.set_width(content_w);
                if let Some(b) = &tab.banner {
                    egui::Frame::new().stroke(Stroke::new(1.0, theme::DANGER)).inner_margin(12.0).corner_radius(2).show(ui, |ui| {
                        ui.set_width(content_w - 26.0);
                        ui.label(RichText::new(b).color(theme::DANGER).font(FontId::new(13.0, theme::mono())));
                    });
                    ui.add_space(16.0);
                }
                node(ui, &def.root, &mut r, None);
            });
        });
        ui.add_space(96.0 * r.z);
    });
    r.acts
}

fn node(ui: &mut Ui, n: &Node, r: &mut R, item: Option<&Value>) {
    match n {
        Node::Stack { dir, gap, align, children } => {
            let g = r.gap(*gap);
            let cross = match align {
                inert_proto::Align::Start | inert_proto::Align::Stretch => Align::Min,
                inert_proto::Align::Center => Align::Center,
                inert_proto::Align::End => Align::Max,
            };
            match dir {
                Dir::V => {
                    let layout = Layout::top_down(cross).with_cross_justify(*align == inert_proto::Align::Stretch);
                    ui.with_layout(layout, |ui| {
                        ui.spacing_mut().item_spacing.y = g;
                        for c in children {
                            node(ui, c, r, item);
                        }
                    });
                }
                Dir::H => {
                    let layout = Layout::left_to_right(cross).with_main_wrap(true);
                    ui.with_layout(layout, |ui| {
                        ui.spacing_mut().item_spacing.x = g;
                        ui.spacing_mut().item_spacing.y = g * 0.5;
                        let was = (r.in_row, r.row_w);
                        r.in_row = true;
                        r.row_w = ui.available_width();
                        for c in children {
                            node(ui, c, r, item);
                        }
                        (r.in_row, r.row_w) = was;
                    });
                }
            }
        }
        Node::Text { style, text } => {
            let s = match text {
                TextSrc::Lit(s) => s.clone(),
                TextSrc::Bind(p) => r.get(p, item).map(|v| fmt_value(&v)).unwrap_or_default(),
            };
            let color = match style {
                TextStyle::Label => r.c(r.d.colors.muted),
                _ => r.c(r.d.colors.fg),
            };
            let spacing = match style {
                TextStyle::Label => 1.6,
                TextStyle::Display => -1.5 * r.z,
                _ => 0.0,
            };
            // Find in page: matches get a marker behind them.
            let find = ui.ctx().data(|d| d.get_temp::<String>(Id::new(FIND))).filter(|q| !q.is_empty());
            let hits = find.as_ref().map(|q| matches(&s, q)).unwrap_or_default();
            if hits.is_empty() {
                ui.add(egui::Label::new(RichText::new(s).font(r.font(*style)).color(color).extra_letter_spacing(spacing)).wrap());
            } else {
                let mut job = egui::text::LayoutJob::default();
                let fmt = |bg: Color32| egui::TextFormat { font_id: r.font(*style), color, extra_letter_spacing: spacing, background: bg, ..Default::default() };
                let mut at = 0;
                for (a, b) in hits {
                    job.append(&s[at..a], 0.0, fmt(Color32::TRANSPARENT));
                    job.append(&s[a..b], 0.0, fmt(FIND_MARK));
                    at = b;
                }
                job.append(&s[at..], 0.0, fmt(Color32::TRANSPARENT));
                job.wrap.max_width = ui.available_width();
                ui.add(egui::Label::new(job).wrap());
            }
        }
        Node::Divider => {
            let w = ui.available_width();
            let (rect, _) = ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
            ui.painter().rect_filled(rect, 0.0, r.c(r.d.colors.line));
        }
        Node::Button { label, action, args, variant } => {
            let (fill, text_c, stroke) = match variant {
                Variant::Primary => (r.c(r.d.colors.accent), r.c(r.d.colors.on_accent), Stroke::NONE),
                Variant::Ghost => (Color32::TRANSPARENT, r.c(r.d.colors.fg), Stroke::new(1.0, theme::LINE_STRONG)),
            };
            let text = RichText::new(label.as_str()).font(FontId::new(14.0 * r.z, theme::sans_medium())).color(text_c);
            let b = egui::Button::new(text).fill(fill).stroke(stroke).corner_radius(CornerRadius::same(r.d.radius.min(20) as u8)).min_size(vec2(96.0 * r.z, 40.0 * r.z));
            let busy = r.busy;
            let resp = ui.allocate_ui_with_layout(vec2(0.0, 40.0 * r.z), Layout::left_to_right(Align::Center), |ui| ui.add_enabled(!busy, b)).inner;
            crate::drive::reg(format!("btn:{label}"), resp.rect);
            if resp.clicked() {
                submit(r, action, args, item);
            }
        }
        Node::Input { action, field, label, kind, placeholder, .. } => {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.label(RichText::new(label.to_uppercase()).font(FontId::new(11.0, theme::mono())).color(r.c(r.d.colors.muted)).extra_letter_spacing(1.6));
                let key = (action.clone(), field.clone());
                let fg = r.c(r.d.colors.fg);
                // In a row, fields share a narrow width (split view) instead of spilling out of it.
                let w = if r.in_row { (300.0 * r.z).min(r.row_w * 0.46) } else { ui.available_width().min(520.0 * r.z).max(200.0) };
                let font = FontId::new(r.d.size(TextStyle::Body) as f32 * r.z, family(r.d.font(TextStyle::Body)));
                let entry = r.forms.entry(key).or_insert(FormValue::Text(String::new()));
                if let FormValue::Text(t) = entry {
                    let mut te = match kind {
                        InputKind::Multiline => egui::TextEdit::multiline(t).desired_rows(5),
                        _ => egui::TextEdit::singleline(t),
                    };
                    te = te.font(font).desired_width(w).margin(vec2(10.0, 9.0)).text_color(fg);
                    if let Some(p) = placeholder {
                        te = te.hint_text(RichText::new(p).color(theme::DIM));
                    }
                    let resp = ui.add(te);
                    crate::drive::reg(format!("input:{field}"), resp.rect);
                }
                if let Some(e) = r.errors.get(field) {
                    ui.label(RichText::new(e).color(theme::DANGER).font(FontId::new(12.0, theme::mono())));
                }
            });
        }
        Node::Select { action, field, label, .. } => {
            let options: Vec<(String, String)> = match r.m.actions.get(action).and_then(|a| a.fields.get(field)) {
                Some(FieldType::Enum { options }) => options.iter().map(|o| (o.value.clone(), o.label.clone())).collect(),
                Some(FieldType::Choice { from }) => r
                    .data
                    .get(from)
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|it| Some((it.get("id")?.as_text()?.to_owned(), it.get("label")?.as_text()?.to_owned())))
                            .collect()
                    })
                    .unwrap_or_default(),
                _ => Vec::new(),
            };
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.label(RichText::new(label.to_uppercase()).font(FontId::new(11.0, theme::mono())).color(r.c(r.d.colors.muted)).extra_letter_spacing(1.6));
                let key = (action.clone(), field.clone());
                let first = options.first().map(|o| o.0.clone()).unwrap_or_default();
                let entry = r.forms.entry(key).or_insert(FormValue::Choice(first));
                if let FormValue::Choice(v) = entry {
                    let shown = options.iter().find(|o| &o.0 == v).map(|o| o.1.clone()).unwrap_or_else(|| "Choose…".into());
                    egui::ComboBox::from_id_salt((action, field))
                        .selected_text(RichText::new(shown).font(FontId::new(14.0 * r.z, theme::sans())))
                        .width(if r.in_row { (200.0 * r.z).min(r.row_w * 0.28) } else { ui.available_width().min(520.0 * r.z).max(200.0) })
                        .show_ui(ui, |ui| {
                            for (val, lab) in &options {
                                ui.selectable_value(v, val.clone(), lab);
                            }
                        });
                }
                if let Some(e) = r.errors.get(field) {
                    ui.label(RichText::new(e).color(theme::DANGER).font(FontId::new(12.0, theme::mono())));
                }
            });
        }
        Node::Toggle { action, field, label, .. } => {
            let key = (action.clone(), field.clone());
            let accent = r.c(r.d.colors.accent);
            let fg = r.c(r.d.colors.fg);
            let entry = r.forms.entry(key).or_insert(FormValue::Bool(false));
            if let FormValue::Bool(on) = entry {
                ui.horizontal(|ui| {
                    let (rect, resp) = ui.allocate_exact_size(vec2(38.0, 22.0), Sense::click());
                    crate::drive::reg(format!("toggle:{field}"), rect);
                    if resp.clicked() {
                        *on = !*on;
                    }
                    let p = ui.painter();
                    p.rect_stroke(rect, 11.0, Stroke::new(1.0, if *on { accent } else { theme::LINE_STRONG }), egui::StrokeKind::Inside);
                    if *on {
                        p.rect_filled(rect.shrink(3.0), 8.0, accent.gamma_multiply(0.25));
                    }
                    let x = if *on { rect.right() - 11.0 } else { rect.left() + 11.0 };
                    p.circle_filled(egui::pos2(x, rect.center().y), 7.0, if *on { accent } else { theme::MUTED });
                    let l = ui.add(egui::Label::new(RichText::new(label).font(FontId::new(15.0 * r.z, theme::sans())).color(fg)).sense(Sense::click()));
                    if l.clicked() {
                        *on = !*on;
                    }
                });
            }
            if let Some(e) = r.errors.get(field) {
                ui.label(RichText::new(e).color(theme::DANGER).font(FontId::new(12.0, theme::mono())));
            }
        }
        Node::List { bind, item: tpl } => {
            let items = r.data.get(bind).and_then(Value::as_array).map(<[Value]>::to_vec).unwrap_or_default();
            if items.is_empty() {
                ui.label(RichText::new("— empty —").font(FontId::new(12.0, theme::mono())).color(r.c(r.d.colors.muted)));
            }
            let line = r.c(r.d.colors.line);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 10.0 * r.z;
                for (i, it) in items.iter().enumerate() {
                    if i > 0 {
                        let w = ui.available_width();
                        let (rect, _) = ui.allocate_exact_size(vec2(w, 1.0), Sense::hover());
                        ui.painter().rect_filled(rect, 0.0, line);
                    }
                    ui.push_id(i, |ui| node(ui, tpl, r, Some(it)));
                }
            });
        }
        Node::Image { src, alt, width, height } => {
            let h = match src {
                ImageSrc::Asset(h) => Some(*h),
                ImageSrc::Bind(p) => r.get(p, item).and_then(|v| v.as_bytes().and_then(|b| b.try_into().ok())),
            };
            let size = vec2(*width as f32 * r.z, *height as f32 * r.z);
            let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
            resp.on_hover_text(alt);
            match h.and_then(|h| r.images.get(&h).map(|i| (h, i))) {
                Some((_, Img::Ready(t))) => {
                    ui.painter().image(t.id(), rect, egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)), Color32::WHITE);
                }
                Some((_, Img::Broken)) => {
                    ui.painter().rect_stroke(rect, 2.0, Stroke::new(1.0, theme::DANGER), egui::StrokeKind::Inside);
                }
                other => {
                    ui.painter().rect_filled(rect, 2.0, theme::RAISED);
                    ui.painter().rect_stroke(rect, 2.0, Stroke::new(1.0, r.c(r.d.colors.line)), egui::StrokeKind::Inside);
                    if other.is_none() {
                        if let Some(h) = h {
                            r.acts.push(Act::Fetch(h));
                        }
                    }
                }
            }
        }
        Node::Link { label, target, params } => {
            let accent = r.c(r.d.colors.accent);
            let rt = RichText::new(label).font(FontId::new(r.d.size(TextStyle::Body) as f32 * r.z, theme::sans())).color(accent);
            let resp = ui.add(egui::Label::new(rt).sense(Sense::click()));
            let resp = resp.on_hover_cursor(egui::CursorIcon::PointingHand);
            crate::drive::reg(format!("link:{label}"), resp.rect);
            if resp.hovered() {
                let rr = resp.rect;
                ui.painter().line_segment([rr.left_bottom(), rr.right_bottom()], Stroke::new(1.0, accent));
            }
            if resp.clicked() && !r.busy {
                match target {
                    LinkTarget::Screen(to) => {
                        let mut p = Fields::new();
                        for (k, a) in params {
                            if let Some(v) = r.arg(a, item) {
                                p.insert(k.clone(), v);
                            }
                        }
                        r.acts.push(Act::Open(to.clone(), p));
                    }
                    LinkTarget::Href(h) => r.acts.push(Act::External(h.clone())),
                }
            }
        }
    }
}

/// Collects form fields + args for `action`, converting text to typed values.
fn submit(r: &mut R, action: &str, args: &std::collections::BTreeMap<String, ArgSrc>, item: Option<&Value>) {
    let Some(def) = r.m.actions.get(action) else { return };
    let mut fields = Fields::new();
    let mut ok = true;
    for ((a, f), v) in r.forms.iter() {
        if a != action {
            continue;
        }
        let Some(t) = def.fields.get(f) else { continue };
        let val = match (v, t) {
            (FormValue::Text(s), FieldType::Int { .. }) => match s.trim().parse::<i64>() {
                Ok(n) => Value::int(n),
                Err(_) => {
                    r.acts.push(Act::LocalError(f.clone(), "Enter a whole number".into()));
                    ok = false;
                    continue;
                }
            },
            (FormValue::Text(s), FieldType::Decimal { scale, .. }) => match Decimal::parse(&s.trim().replace(',', ".")) {
                Some(d) => d.rescale(*scale).unwrap_or(d).to_value(),
                None => {
                    r.acts.push(Act::LocalError(f.clone(), "Enter a number like 12.50".into()));
                    ok = false;
                    continue;
                }
            },
            (FormValue::Text(s), _) => Value::text(s.clone()),
            (FormValue::Choice(s), _) => Value::text(s.clone()),
            (FormValue::Bool(b), _) => Value::Bool(*b),
        };
        fields.insert(f.clone(), val);
    }
    for (k, a) in args {
        if let Some(v) = r.arg(a, item) {
            fields.insert(k.clone(), v);
        }
    }
    if ok {
        r.acts.push(Act::Submit { action: action.to_owned(), fields });
    }
}

/// Human message for a field error code.
pub fn field_message(code: ErrorCode) -> &'static str {
    match code {
        ErrorCode::FieldInvalid => "This value is not accepted by the site's signed schema.",
        ErrorCode::Rejected => "The site rejected this value.",
        _ => "Something went wrong with this field.",
    }
}

/// Where the find bar leaves its query for this frame (egui temp data).
pub const FIND: &str = "inert-find-query";
const FIND_MARK: Color32 = Color32::from_rgba_premultiplied(120, 100, 0, 150);

/// Byte ranges of `q` in `s`, ignoring case.
fn matches(s: &str, q: &str) -> Vec<(usize, usize)> {
    let lower = s.to_lowercase();
    let q = q.to_lowercase();
    // Lowercasing can change byte lengths (rarely); then no ranges.
    if lower.len() != s.len() || q.is_empty() {
        return Vec::new();
    }
    lower.match_indices(&q).map(|(i, m)| (i, i + m.len())).filter(|(a, b)| s.is_char_boundary(*a) && s.is_char_boundary(*b)).collect()
}
