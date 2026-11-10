//! The glyph map: a field of ASCII glyphs laid out like continents, drawn as
//! text with Geist Mono. It shifts under the cursor and, where asked, drifts
//! slowly. Nothing is downloaded and no texture is kept.

use crate::theme;
use eframe::egui::{self, Color32, FontId, Painter, Pos2, Rect, pos2};

/// Light to dense. The letters spell the name, the way the reference card's do.
const RAMP: &[u8] = b".:-=+INERT$8";

fn hash(x: i32, y: i32) -> f32 {
    let mut n = (x as u32).wrapping_mul(374_761_393) ^ (y as u32).wrapping_mul(668_265_263);
    n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    ((n ^ (n >> 16)) & 0xffff) as f32 / 65535.0
}

fn noise(x: f32, y: f32) -> f32 {
    let (xi, yi) = (x.floor(), y.floor());
    let (tx, ty) = (x - xi, y - yi);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (xi, yi) = (xi as i32, yi as i32);
    let top = hash(xi, yi) + (hash(xi + 1, yi) - hash(xi, yi)) * s(tx);
    let bot = hash(xi, yi + 1) + (hash(xi + 1, yi + 1) - hash(xi, yi + 1)) * s(tx);
    top + (bot - top) * s(ty)
}

fn field(x: f32, y: f32) -> f32 {
    0.62 * noise(x, y) + 0.28 * noise(x * 2.0 + 17.0, y * 2.0 + 5.0) + 0.10 * noise(x * 4.3, y * 4.3)
}

pub struct Glyphs {
    pub color: Color32,
    /// Font size in points.
    pub size: f32,
    /// Seconds; pass 0 for a still map.
    pub time: f32,
    /// How strongly the map reacts under the cursor (0 = not at all).
    pub lens: f32,
    /// Share of the field that stays empty (0..1).
    pub sea: f32,
}

impl Glyphs {
    /// The quiet map on the window frame.
    pub fn frame() -> Self {
        Glyphs { color: theme::glass(0.15), size: 9.0, time: 0.0, lens: 0.7, sea: 0.56 }
    }

    /// The bright map on a hero card.
    pub fn hero(time: f32) -> Self {
        Glyphs { color: theme::glass(0.62), size: 9.5, time, lens: 1.0, sea: 0.47 }
    }
}

/// Paints the map into `rect`. Glyphs sit one to a cell on a window-wide
/// grid (a cell is two characters wide), so separate rects line up.
pub fn glyphs(p: &Painter, ctx: &egui::Context, rect: Rect, pointer: Option<Pos2>, g: &Glyphs) {
    if rect.width() < 2.0 || rect.height() < 2.0 {
        return;
    }
    let font = FontId::new(g.size, theme::mono());
    let (cw, rh) = ctx.fonts_mut(|f| (f.glyph_width(&font, 'M'), f.row_height(&font)));
    if cw <= 0.0 || rh <= 0.0 {
        return;
    }
    let cell = cw * 2.0;
    let rh = (rh * 1.02).round();
    let p = p.with_clip_rect(rect.intersect(p.clip_rect()));
    let (c0, c1) = ((rect.left() / cell).floor() as i32, (rect.right() / cell).ceil() as i32);
    let (r0, r1) = ((rect.top() / rh).floor() as i32, (rect.bottom() / rh).ceil() as i32);
    // Drift in steps, so still frames reuse their laid-out rows.
    let t = (g.time * 6.0).floor() / 6.0;
    let reach = 150.0;
    // The cursor only matters inside (or just outside) this rect.
    let pointer = pointer.filter(|pt| g.lens > 0.0 && rect.expand(reach).contains(*pt));
    let mut row = String::with_capacity(((c1 - c0).max(0) * 2) as usize);
    for y in r0..r1 {
        row.clear();
        let py = y as f32 * rh;
        let near_row = pointer.is_some_and(|pt| (py - pt.y).abs() < reach);
        let mut any = false;
        for x in c0..c1 {
            let mut v = field(x as f32 * 0.085 + t * 0.05, y as f32 * 0.105 - t * 0.032);
            let mut pick = hash(x, y);
            if near_row {
                let pt = pointer.unwrap();
                let d = ((x as f32 * cell - pt.x).powi(2) + (py - pt.y).powi(2)).sqrt();
                let k = (1.0 - d / reach).max(0.0);
                if k > 0.0 {
                    // Under the cursor the land rises and the letters reshuffle.
                    v += k * k * 0.26 * g.lens;
                    pick = hash(x + (pt.x / 11.0) as i32, y + (pt.y / 11.0) as i32);
                }
            }
            let ch = if v < g.sea {
                b' '
            } else {
                // Shores are dots and dashes; the letters gather inland.
                let depth = ((v - g.sea) / (1.0 - g.sea) * 2.6).min(1.0);
                let i = (depth * 0.72 + pick * 0.28).powf(1.25) * (RAMP.len() - 1) as f32;
                any = true;
                RAMP[(i as usize).min(RAMP.len() - 1)]
            };
            row.push(ch as char);
            row.push(' ');
        }
        if any {
            p.text(pos2(c0 as f32 * cell, py), egui::Align2::LEFT_TOP, &row, font.clone(), g.color);
        }
    }
}

/// The Inert mark: the letter I, sealed in a ring.
pub fn mark(p: &Painter, c: Pos2, r: f32, color: Color32) {
    let s = egui::Stroke::new((r * 0.17).max(1.4), color);
    p.circle_stroke(c, r, s);
    p.line_segment([pos2(c.x, c.y - r * 0.48), pos2(c.x, c.y + r * 0.48)], s);
}

/// Placeholder pattern for images still loading.
pub fn dither_box(w: usize, h: usize, c: [u8; 3]) -> egui::ColorImage {
    const BAYER: [[f32; 4]; 4] = [[0., 8., 2., 10.], [12., 4., 14., 6.], [3., 11., 1., 9.], [15., 7., 13., 5.]];
    let mut px = vec![Color32::BLACK; w * h];
    for y in 0..h {
        for x in 0..w {
            let t = (BAYER[y % 4][x % 4] + 0.5) / 16.0;
            let v = 0.18 + 0.12 * ((x + y) as f32 / (w + h) as f32);
            if v > t {
                px[y * w + x] = Color32::from_rgb(c[0] / 3, c[1] / 3, c[2] / 3);
            }
        }
    }
    egui::ColorImage::new([w, h], px)
}
