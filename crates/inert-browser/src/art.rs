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
    /// A second ink: some glyphs take this color instead (two-tone maps).
    pub alt: Option<Color32>,
}

/// Glyph ink for the dark look: the chosen color, lifted toward white.
fn blue_ink(a: f32) -> Color32 {
    let c = theme::accent().lerp_to_gamma(Color32::WHITE, 0.28);
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a * 255.0) as u8)
}

impl Glyphs {
    /// The quiet map on the window frame.
    pub fn frame() -> Self {
        let color = if theme::is_dark() { blue_ink(0.30) } else { theme::glass(0.15) };
        Glyphs { color, size: 9.0, time: 0.0, lens: 0.7, sea: 0.56, alt: None }
    }

    /// The bright map on a hero card. In the dark look the letters are blue
    /// on near-black, with a few white ones.
    pub fn hero(time: f32) -> Self {
        if theme::is_dark() {
            Glyphs { color: blue_ink(0.85), size: 9.5, time, lens: 1.0, sea: 0.47, alt: Some(theme::glass(0.55)) }
        } else {
            Glyphs { color: theme::glass(0.62), size: 9.5, time, lens: 1.0, sea: 0.47, alt: None }
        }
    }

    /// The logo tile: white letters on the brand blue.
    pub fn logo(time: f32, size: f32) -> Self {
        Glyphs { color: theme::glass(0.62), size, time, lens: 0.0, sea: 0.40, alt: None }
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
    let mut row2 = String::with_capacity(((c1 - c0).max(0) * 2) as usize);
    for y in r0..r1 {
        row.clear();
        row2.clear();
        let mut any2 = false;
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
            // Two-tone maps send some letters to the second ink, in streaks
            // that drift with the map.
            let second = g.alt.is_some() && ch != b' ' && noise(x as f32 * 0.21 + t * 0.4, y as f32 * 0.5) > 0.62;
            if second {
                row.push(' ');
                row2.push(ch as char);
                any2 = true;
            } else {
                row.push(ch as char);
                row2.push(' ');
            }
            row.push(' ');
            row2.push(' ');
        }
        if any {
            p.text(pos2(c0 as f32 * cell, py), egui::Align2::LEFT_TOP, &row, font.clone(), g.color);
        }
        if let (true, Some(c2)) = (any2, g.alt) {
            p.text(pos2(c0 as f32 * cell, py), egui::Align2::LEFT_TOP, &row2, font.clone(), c2);
        }
    }
}

/// The Inert mark: a text caret held between two brackets — text that
/// stays put, code that never runs.
pub fn mark(p: &Painter, c: Pos2, r: f32, color: Color32) {
    // In a 100-unit box: brackets at x 24–37 and 63–76, y 22–78; the caret
    // in the middle. `r` is half the box.
    let k = r / 50.0;
    let at = |x: f32, y: f32| pos2((c.x + (x - 50.0) * k).round(), (c.y + (y - 50.0) * k).round());
    let w = (7.0 * k).max(1.5);
    let s = egui::Stroke::new(w, color);
    p.add(egui::Shape::line(vec![at(37.0, 22.0), at(24.0, 22.0), at(24.0, 78.0), at(37.0, 78.0)], s));
    p.add(egui::Shape::line(vec![at(63.0, 22.0), at(76.0, 22.0), at(76.0, 78.0), at(63.0, 78.0)], s));
    // Round line ends.
    for pt in [at(37.0, 22.0), at(37.0, 78.0), at(63.0, 22.0), at(63.0, 78.0)] {
        p.circle_filled(pt, w / 2.0, color);
    }
    p.rect_filled(Rect::from_min_max(at(46.5, 32.0), at(53.5, 68.0)), w / 2.0, color);
}

/// The logo: a rounded tile in the brand blue with the glyph map and the
/// held caret. `bg` is what the tile sits on (its corners are painted with it).
pub fn logo(p: &Painter, ctx: &egui::Context, r: Rect, time: f32, bg: Color32) {
    let rad = r.width() * 0.24;
    p.rect_filled(r, rad, theme::BLUE);
    let size = (r.width() / 9.0).clamp(4.0, 10.0);
    glyphs(p, ctx, r, None, &Glyphs::logo(time, size));
    let c = r.center();
    p.rect_filled(Rect::from_center_size(c, egui::vec2(r.width() * 0.72, r.height() * 0.58)), r.width() * 0.13, theme::BLUE);
    mark(p, c, r.width() * 0.34, theme::WHITE);
    crate::chrome::round_corners(p, r, rad, bg);
}

/// 5×7 pixel letters for the app icon (drawn without a font engine).
const PIX: [(u8, [&str; 7]); 10] = [
    (b'I', ["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "#####"]),
    (b'N', ["#...#", "##..#", "#.#.#", "#.#.#", "#..##", "#...#", "#...#"]),
    (b'E', ["#####", "#....", "#....", "####.", "#....", "#....", "#####"]),
    (b'R', ["####.", "#...#", "#...#", "####.", "#.#..", "#..#.", "#...#"]),
    (b'T', ["#####", "..#..", "..#..", "..#..", "..#..", "..#..", "..#.."]),
    (b'$', ["..#..", ".####", "#.#..", ".###.", "..#.#", "####.", "..#.."]),
    (b'+', [".....", "..#..", "..#..", "#####", "..#..", "..#..", "....."]),
    (b'=', [".....", ".....", "#####", ".....", "#####", ".....", "....."]),
    (b'-', [".....", ".....", ".....", "#####", ".....", ".....", "....."]),
    (b':', [".....", "..#..", ".....", ".....", ".....", "..#..", "....."]),
];

/// The app icon as an image: the logo tile on macOS's icon grid.
pub fn icon_image(n: usize) -> egui::ColorImage {
    let nf = n as f32;
    let mut px = vec![Color32::TRANSPARENT; n * n];
    // macOS icon grid: an 824/1024 rounded square, centered.
    let (lo, hi) = (nf * 100.0 / 1024.0, nf * 924.0 / 1024.0);
    let rad = nf * 185.0 / 1024.0;
    let inside = |x: f32, y: f32| {
        let cx = x.clamp(lo + rad, hi - rad);
        let cy = y.clamp(lo + rad, hi - rad);
        (x - cx).powi(2) + (y - cy).powi(2) <= rad * rad && x >= lo && x <= hi && y >= lo && y <= hi
    };
    let navy = [31u8, 77, 255];
    let blue = [150u8, 172, 255];
    // Letter grid: cells of 7×9 "pixels" of size `u`.
    let u = (nf / 240.0).max(1.0);
    let (cw, ch) = (7.0 * u, 9.0 * u);
    let c = nf / 2.0;
    // The letters stay clear of a box around the mark.
    let (clear_w, clear_h) = (nf * 0.215, nf * 0.225);
    for y in 0..n {
        for x in 0..n {
            // 4×4 samples per pixel for smooth edges.
            let mut acc = [0f32; 4];
            for sy in 0..4 {
                for sx in 0..4 {
                    let fx = x as f32 + (sx as f32 + 0.5) / 4.0;
                    let fy = y as f32 + (sy as f32 + 0.5) / 4.0;
                    if !inside(fx, fy) {
                        continue;
                    }
                    let mut col = navy;
                    let d = ((fx - c).powi(2) + (fy - c).powi(2)).sqrt();
                    let _ = d;
                    if (fx - c).abs() > clear_w || (fy - c).abs() > clear_h {
                        // Flowing letters: a diagonal band field.
                        let (gx, gy) = ((fx / cw).floor(), (fy / ch).floor());
                        let v = field(gx * 0.16 + gy * 0.05, gy * 0.20);
                        if v > 0.47 {
                            let pick = hash(gx as i32, gy as i32);
                            let depth = ((v - 0.47) / 0.53 * 2.4).min(1.0);
                            let k = ((depth * 0.7 + pick * 0.3) * (PIX.len() - 1) as f32) as usize;
                            let glyph = &PIX[k.min(PIX.len() - 1)].1;
                            let (lx, ly) = (((fx - gx * cw) / u) as usize, ((fy - gy * ch) / u) as usize);
                            if lx < 5 && ly < 7 && glyph[ly].as_bytes()[lx] == b'#' {
                                col = blue;
                            }
                        }
                    }
                    // The held caret: two brackets and a bar, white.
                    let (ux, uy) = ((fx - c) / (nf * 0.31) * 50.0 + 50.0, (fy - c) / (nf * 0.31) * 50.0 + 50.0);
                    let half = 3.5;
                    let near = |x0: f32, y0: f32, x1: f32, y1: f32| ux >= x0.min(x1) - half && ux <= x0.max(x1) + half && uy >= y0.min(y1) - half && uy <= y0.max(y1) + half;
                    let bracket = near(24.0, 22.0, 37.0, 22.0) || near(24.0, 78.0, 37.0, 78.0) || near(24.0, 22.0, 24.0, 78.0) || near(63.0, 22.0, 76.0, 22.0) || near(63.0, 78.0, 76.0, 78.0) || near(76.0, 22.0, 76.0, 78.0);
                    let caret = (ux - 50.0).abs() <= 3.5 && (32.0..=68.0).contains(&uy);
                    if bracket || caret {
                        col = [255, 255, 255];
                    }
                    acc[0] += col[0] as f32;
                    acc[1] += col[1] as f32;
                    acc[2] += col[2] as f32;
                    acc[3] += 1.0;
                }
            }
            if acc[3] > 0.0 {
                let a = acc[3] / 16.0;
                let k = 1.0 / acc[3];
                px[y * n + x] = Color32::from_rgba_unmultiplied((acc[0] * k) as u8, (acc[1] * k) as u8, (acc[2] * k) as u8, (a * 255.0) as u8);
            }
        }
    }
    egui::ColorImage::new([n, n], px)
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
