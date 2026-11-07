//! Procedural Stencil-style art: an ordered-dither planet arc with stars.
//! Generated once into a texture; nothing is downloaded.

use eframe::egui::{self, Color32, ColorImage, TextureHandle, TextureOptions};

const BAYER: [[f32; 4]; 4] = [[0., 8., 2., 10.], [12., 4., 14., 6.], [3., 11., 1., 9.], [15., 7., 13., 5.]];

/// `w`×`h` image of a big planet whose limb sweeps across the top-right.
pub fn planet(w: usize, h: usize, fill: [u8; 3], edge: [u8; 3]) -> ColorImage {
    let mut px = vec![Color32::BLACK; w * h];
    // Planet circle: centre far to the top-right, radius larger than the image.
    let (cx, cy, r) = (w as f32 * 1.05, -(h as f32) * 0.55, h as f32 * 1.25);
    for y in 0..h {
        for x in 0..w {
            let (fx, fy) = (x as f32, y as f32);
            let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            let t = (BAYER[y % 4][x % 4] + 0.5) / 16.0;
            let c = if d < r - 6.0 {
                // Density falls off away from the limb (lit edge).
                let depth = ((r - d) / (r * 0.55)).clamp(0.0, 1.0);
                let lit = (1.0 - depth).powf(1.6) * 0.95 + 0.03;
                if lit > t { Color32::from_rgb(fill[0], fill[1], fill[2]) } else { Color32::BLACK }
            } else if d < r {
                Color32::from_rgb(edge[0], edge[1], edge[2])
            } else {
                // Atmosphere glow as sparse dither, then stars.
                let a = ((r + 60.0 - d) / 60.0).clamp(0.0, 1.0) * 0.55;
                if a > t {
                    Color32::from_rgb(edge[0] / 2 + 40, edge[1] / 2 + 40, edge[2] / 2 + 60)
                } else {
                    let hsh = (x as u32).wrapping_mul(73856093) ^ (y as u32).wrapping_mul(19349663);
                    if hsh % 1400 == 0 { Color32::from_gray(210) } else if hsh % 600 == 0 { Color32::from_gray(90) } else { Color32::BLACK }
                }
            };
            px[y * w + x] = c;
        }
    }
    ColorImage::new([w, h], px)
}

pub fn planet_texture(ctx: &egui::Context) -> TextureHandle {
    ctx.load_texture("inert-planet", planet(900, 560, [180, 140, 255], [127, 156, 255]), TextureOptions::NEAREST)
}

/// Placeholder pattern for images still loading.
pub fn dither_box(w: usize, h: usize, c: [u8; 3]) -> ColorImage {
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
    ColorImage::new([w, h], px)
}
