//! Glyph theme: the window is a cobalt-blue card with a live ASCII glyph map
//! (glyphblue); pages sit in dark inset cards and controls are soft glass
//! pills (Dia, Facet). Text colors pass 4.5:1 on the surface they are used on.

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId, RichText, Stroke, vec2};
use std::sync::Arc;

// The frame: the window itself.
pub const BLUE: Color32 = Color32::from_rgb(31, 77, 255);
pub const BLUE_DEEP: Color32 = Color32::from_rgb(20, 52, 204);
// Dark surfaces: content card, popovers, panels.
pub const BG: Color32 = Color32::from_rgb(9, 11, 18);
pub const PANEL: Color32 = Color32::from_rgb(14, 17, 26);
pub const RAISED: Color32 = Color32::from_rgb(22, 26, 38);
pub const HOVER: Color32 = Color32::from_rgb(31, 36, 52);
pub const LINE: Color32 = Color32::from_rgb(36, 42, 60);
pub const LINE_STRONG: Color32 = Color32::from_rgb(56, 64, 90);
pub const FG: Color32 = Color32::from_rgb(236, 238, 245);
pub const WHITE: Color32 = Color32::from_rgb(255, 255, 255);
pub const MUTED: Color32 = Color32::from_rgb(160, 167, 188);
pub const DIM: Color32 = Color32::from_rgb(132, 140, 165);
pub const ACCENT: Color32 = Color32::from_rgb(110, 144, 255);
pub const EDGE: Color32 = Color32::from_rgb(143, 168, 255);
pub const SECURE: Color32 = Color32::from_rgb(176, 196, 255);
pub const WARN: Color32 = Color32::from_rgb(255, 181, 71);
pub const DANGER: Color32 = Color32::from_rgb(255, 122, 122);
pub const OK: Color32 = Color32::from_rgb(93, 218, 165);

/// White glass over the blue frame: hover and active fills, and ink on blue.
pub fn glass(a: f32) -> Color32 {
    Color32::from_white_alpha((a.clamp(0.0, 1.0) * 255.0) as u8)
}

/// Deep navy glass: wells on the blue frame (address bar, badges).
pub fn well(a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(4, 8, 28, (a.clamp(0.0, 1.0) * 255.0) as u8)
}

/// The soft shadow under floating panels.
pub fn shadow() -> egui::epaint::Shadow {
    egui::epaint::Shadow { offset: [0, 14], blur: 36, spread: 0, color: Color32::from_black_alpha(120) }
}

/// A floating dark panel: popovers, dropdowns, dialogs.
pub fn panel_frame(radius: u8, margin: f32) -> egui::Frame {
    egui::Frame::new().fill(PANEL).stroke(Stroke::new(1.0, glass(0.10))).corner_radius(radius).inner_margin(margin).shadow(shadow())
}

/// A pill call-to-action in sentence case.
pub fn cta(text: &str, fill: Color32, ink: Color32, h: f32) -> egui::Button<'static> {
    egui::Button::new(RichText::new(text).font(FontId::new(13.5, sans_medium())).color(ink)).fill(fill).corner_radius((h / 2.0) as u8).min_size(vec2(0.0, h))
}

/// The quiet twin of `cta`: outline only.
pub fn cta_ghost(text: &str, h: f32) -> egui::Button<'static> {
    cta(text, Color32::TRANSPARENT, FG, h).stroke(Stroke::new(1.0, LINE_STRONG))
}

pub fn serif() -> FontFamily {
    FontFamily::Name("serif".into())
}
pub fn sans() -> FontFamily {
    FontFamily::Proportional
}
pub fn sans_medium() -> FontFamily {
    FontFamily::Name("sans-medium".into())
}
pub fn mono() -> FontFamily {
    FontFamily::Monospace
}
pub fn pixel() -> FontFamily {
    FontFamily::Name("pixel".into())
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut defs = FontDefinitions::default();
    let mut add = |name: &str, bytes: &'static [u8]| {
        defs.font_data.insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    };
    add("geist", include_bytes!("../assets/fonts/Geist-Regular.ttf"));
    add("geist-medium", include_bytes!("../assets/fonts/Geist-Medium.ttf"));
    add("geist-mono", include_bytes!("../assets/fonts/GeistMono-Regular.ttf"));
    add("geist-serif", include_bytes!("../assets/fonts/GeistSerif-Regular.ttf"));
    add("geist-pixel", include_bytes!("../assets/fonts/GeistPixel-Regular.ttf"));

    // Keep egui's bundled fonts as fallbacks for symbols the Geist fonts lack.
    let fallbacks: Vec<String> = defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    let with_fallback = |first: &str| {
        let mut v = vec![first.to_owned()];
        v.extend(fallbacks.iter().cloned());
        v
    };
    defs.families.insert(FontFamily::Proportional, with_fallback("geist"));
    defs.families.insert(FontFamily::Monospace, with_fallback("geist-mono"));
    defs.families.insert(serif(), with_fallback("geist-serif"));
    defs.families.insert(sans_medium(), with_fallback("geist-medium"));
    defs.families.insert(pixel(), with_fallback("geist-pixel"));
    ctx.set_fonts(defs);
}

pub fn install_style(ctx: &egui::Context) {
    ctx.set_theme(egui::Theme::Dark);
    ctx.all_styles_mut(|style| {
        let v = &mut style.visuals;
        v.dark_mode = true;
        v.panel_fill = BG;
        v.window_fill = PANEL;
        v.extreme_bg_color = RAISED;
        v.faint_bg_color = PANEL;
        v.code_bg_color = RAISED;
        v.window_stroke = Stroke::new(1.0, glass(0.10));
        v.window_corner_radius = egui::CornerRadius::same(16);
        v.menu_corner_radius = egui::CornerRadius::same(14);
        v.window_shadow = shadow();
        v.popup_shadow = shadow();
        v.selection.bg_fill = Color32::from_rgb(38, 64, 168);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.hyperlink_color = SECURE;
        v.override_text_color = Some(FG);
        for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.corner_radius = egui::CornerRadius::same(10);
            w.fg_stroke = Stroke::new(1.0, FG);
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
        v.widgets.noninteractive.bg_fill = BG;
        v.widgets.inactive.bg_fill = RAISED;
        v.widgets.inactive.weak_bg_fill = RAISED;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, LINE);
        v.widgets.hovered.bg_fill = HOVER;
        v.widgets.hovered.weak_bg_fill = HOVER;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, LINE_STRONG);
        v.widgets.active.bg_fill = HOVER;
        v.widgets.active.weak_bg_fill = HOVER;
        v.widgets.active.bg_stroke = Stroke::new(1.0, FG);
        v.widgets.open.bg_fill = HOVER;
        v.widgets.open.weak_bg_fill = HOVER;
        style.spacing.item_spacing = egui::vec2(8.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.interact_size.y = 28.0;
        style.text_styles.insert(egui::TextStyle::Body, FontId::new(14.0, sans()));
        style.text_styles.insert(egui::TextStyle::Button, FontId::new(13.0, sans()));
        style.text_styles.insert(egui::TextStyle::Monospace, FontId::new(13.0, mono()));
        style.text_styles.insert(egui::TextStyle::Small, FontId::new(11.0, mono()));
        style.text_styles.insert(egui::TextStyle::Heading, FontId::new(28.0, sans()));
        style.animation_time = 0.14;
    });
}

/// `SMALL MONO LABEL` — uppercase, wide tracking (glyphblue's corner tag).
pub fn label(s: &str) -> RichText {
    RichText::new(s.to_uppercase()).font(FontId::new(11.0, mono())).color(MUTED).extra_letter_spacing(1.6)
}

pub fn label_c(s: &str, c: Color32) -> RichText {
    RichText::new(s.to_uppercase()).font(FontId::new(11.0, mono())).color(c).extra_letter_spacing(1.6)
}

/// Headlines: Geist Sans, slightly tightened (glyphblue's headline).
pub fn title(s: &str, size: f32) -> RichText {
    RichText::new(s).font(FontId::new(size, sans())).color(WHITE).extra_letter_spacing(-size * 0.022)
}

pub fn body(s: &str) -> RichText {
    RichText::new(s).font(FontId::new(14.0, sans())).color(FG)
}

pub fn muted(s: &str) -> RichText {
    RichText::new(s).font(FontId::new(13.0, sans())).color(MUTED)
}

pub fn mono_text(s: &str, size: f32) -> RichText {
    RichText::new(s).font(FontId::new(size, mono())).color(FG)
}

/// Group colors: shown as solid chips, so they read on the blue frame.
pub const GROUP_COLORS: [(&str, Color32); 8] = [
    ("grey", Color32::from_rgb(170, 170, 170)),
    ("blue", Color32::from_rgb(127, 156, 255)),
    ("purple", Color32::from_rgb(180, 140, 255)),
    ("pink", Color32::from_rgb(237, 147, 177)),
    ("red", Color32::from_rgb(240, 120, 110)),
    ("orange", Color32::from_rgb(255, 181, 71)),
    ("green", Color32::from_rgb(151, 196, 89)),
    ("teal", Color32::from_rgb(93, 202, 165)),
];

pub fn group_color(name: &str) -> Color32 {
    GROUP_COLORS.iter().find(|(n, _)| *n == name).map(|(_, c)| *c).unwrap_or(MUTED)
}

pub fn rgb(c: inert_proto::Rgb) -> Color32 {
    Color32::from_rgb(c.0, c.1, c.2)
}
