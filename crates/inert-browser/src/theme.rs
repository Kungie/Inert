//! Stencil-style dark theme: pure black, thin hairlines, mono uppercase labels,
//! Geist Serif headlines. Every color here passes 4.5:1 on black for text.

use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, FontId, RichText, Stroke};
use std::sync::Arc;

pub const BG: Color32 = Color32::from_rgb(0, 0, 0);
pub const PANEL: Color32 = Color32::from_rgb(8, 8, 8);
pub const RAISED: Color32 = Color32::from_rgb(16, 16, 16);
pub const HOVER: Color32 = Color32::from_rgb(24, 24, 24);
pub const LINE: Color32 = Color32::from_rgb(34, 34, 34);
pub const LINE_STRONG: Color32 = Color32::from_rgb(58, 58, 58);
pub const FG: Color32 = Color32::from_rgb(237, 237, 237);
pub const WHITE: Color32 = Color32::from_rgb(255, 255, 255);
pub const MUTED: Color32 = Color32::from_rgb(154, 154, 154);
pub const DIM: Color32 = Color32::from_rgb(138, 138, 138);
pub const ACCENT: Color32 = Color32::from_rgb(180, 140, 255);
pub const EDGE: Color32 = Color32::from_rgb(127, 156, 255);
pub const SECURE: Color32 = Color32::from_rgb(185, 198, 255);
pub const WARN: Color32 = Color32::from_rgb(255, 181, 71);
pub const DANGER: Color32 = Color32::from_rgb(255, 122, 122);
pub const OK: Color32 = Color32::from_rgb(93, 202, 165);

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
        v.window_stroke = Stroke::new(1.0, LINE_STRONG);
        v.window_corner_radius = egui::CornerRadius::same(14);
        v.menu_corner_radius = egui::CornerRadius::same(12);
        v.window_shadow = egui::epaint::Shadow::NONE;
        v.popup_shadow = egui::epaint::Shadow::NONE;
        v.selection.bg_fill = Color32::from_rgb(60, 48, 92);
        v.selection.stroke = Stroke::new(1.0, ACCENT);
        v.hyperlink_color = SECURE;
        v.override_text_color = Some(FG);
        for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active, &mut v.widgets.open] {
            w.corner_radius = egui::CornerRadius::same(8);
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
        style.spacing.button_padding = egui::vec2(10.0, 6.0);
        style.spacing.interact_size.y = 28.0;
        style.text_styles.insert(egui::TextStyle::Body, FontId::new(14.0, sans()));
        style.text_styles.insert(egui::TextStyle::Button, FontId::new(13.0, sans()));
        style.text_styles.insert(egui::TextStyle::Monospace, FontId::new(13.0, mono()));
        style.text_styles.insert(egui::TextStyle::Small, FontId::new(11.0, mono()));
        style.text_styles.insert(egui::TextStyle::Heading, FontId::new(28.0, serif()));
    });
}

/// `STENCIL STYLE LABEL` — mono, uppercase, wide tracking.
pub fn label(s: &str) -> RichText {
    RichText::new(s.to_uppercase()).font(FontId::new(11.0, mono())).color(MUTED).extra_letter_spacing(1.6)
}

pub fn label_c(s: &str, c: Color32) -> RichText {
    RichText::new(s.to_uppercase()).font(FontId::new(11.0, mono())).color(c).extra_letter_spacing(1.6)
}

pub fn serif_text(s: &str, size: f32) -> RichText {
    RichText::new(s).font(FontId::new(size, serif())).color(WHITE)
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

/// Group colors (Chrome-like palette, tuned for black).
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
