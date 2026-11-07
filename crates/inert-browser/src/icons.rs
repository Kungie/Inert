//! Line icons drawn with the painter (no icon fonts, no image files).

use eframe::egui::{self, Color32, Painter, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2, pos2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Back,
    Forward,
    Reload,
    Stop,
    Plus,
    Close,
    Star,
    StarFilled,
    Shield,
    Menu,
    Spark,
    Inbox,
    Server,
    Code,
    Globe,
    Lock,
    Unlock,
    Search,
    History,
    Bookmark,
    Tab,
    Chevron,
    ChevronDown,
    Vpn,
    Gear,
    Layers,
    Folder,
}

fn s(c: Color32) -> Stroke {
    Stroke::new(1.4, c)
}

pub fn paint(p: &Painter, r: Rect, icon: Icon, c: Color32) {
    let ctr = r.center();
    let u = r.width().min(r.height()) / 16.0;
    let at = |x: f32, y: f32| pos2(ctr.x + x * u, ctr.y + y * u);
    let line = |a: Pos2, b: Pos2| {
        p.line_segment([a, b], s(c));
    };
    match icon {
        Icon::Back => {
            line(at(4.0, 0.0), at(-5.0, 0.0));
            line(at(-5.0, 0.0), at(-1.0, -4.0));
            line(at(-5.0, 0.0), at(-1.0, 4.0));
        }
        Icon::Forward => {
            line(at(-4.0, 0.0), at(5.0, 0.0));
            line(at(5.0, 0.0), at(1.0, -4.0));
            line(at(5.0, 0.0), at(1.0, 4.0));
        }
        Icon::Reload => {
            let pts: Vec<Pos2> = (0..=20).map(|i| {
                let a = 0.9 + i as f32 / 20.0 * 5.0;
                at(5.0 * a.cos(), 5.0 * a.sin())
            }).collect();
            p.add(egui::Shape::line(pts, s(c)));
            let tip = at(5.0 * 0.9f32.cos(), 5.0 * 0.9f32.sin());
            line(tip, tip + vec2(0.0, -3.5 * u));
            line(tip, tip + vec2(3.5 * u, 0.0));
        }
        Icon::Stop | Icon::Close => {
            line(at(-4.0, -4.0), at(4.0, 4.0));
            line(at(4.0, -4.0), at(-4.0, 4.0));
        }
        Icon::Plus => {
            line(at(-5.0, 0.0), at(5.0, 0.0));
            line(at(0.0, -5.0), at(0.0, 5.0));
        }
        Icon::Star | Icon::StarFilled => {
            let pts: Vec<Pos2> = (0..10).map(|i| {
                let a = -std::f32::consts::FRAC_PI_2 + i as f32 * std::f32::consts::PI / 5.0;
                let rr = if i % 2 == 0 { 6.0 } else { 2.6 };
                at(rr * a.cos(), rr * a.sin() + 0.5)
            }).collect();
            if icon == Icon::StarFilled {
                p.add(egui::Shape::convex_polygon(pts.clone(), c, Stroke::NONE));
                // concave star: fill the inner pentagon too
            }
            p.add(egui::Shape::closed_line(pts, s(c)));
        }
        Icon::Shield => {
            let pts = vec![at(0.0, -6.0), at(5.0, -4.0), at(4.5, 2.0), at(0.0, 6.0), at(-4.5, 2.0), at(-5.0, -4.0)];
            p.add(egui::Shape::closed_line(pts, s(c)));
        }
        Icon::Menu => {
            for y in [-4.0, 0.0, 4.0] {
                line(at(-5.0, y), at(5.0, y));
            }
        }
        Icon::Spark => {
            line(at(0.0, -6.0), at(0.0, 6.0));
            line(at(-6.0, 0.0), at(6.0, 0.0));
            line(at(-3.0, -3.0), at(3.0, 3.0));
            line(at(3.0, -3.0), at(-3.0, 3.0));
            p.circle_filled(ctr, 1.6 * u, c);
        }
        Icon::Inbox => {
            let pts = vec![at(-6.0, 1.0), at(-4.0, -5.0), at(4.0, -5.0), at(6.0, 1.0), at(6.0, 5.0), at(-6.0, 5.0)];
            p.add(egui::Shape::closed_line(pts, s(c)));
            p.add(egui::Shape::line(vec![at(-6.0, 1.0), at(-2.0, 1.0), at(-1.0, 3.0), at(1.0, 3.0), at(2.0, 1.0), at(6.0, 1.0)], s(c)));
        }
        Icon::Server => {
            for y in [-5.0, 1.0] {
                p.rect_stroke(Rect::from_min_max(at(-6.0, y), at(6.0, y + 4.0)), 1.0, s(c), egui::StrokeKind::Middle);
                p.circle_filled(at(-3.5, y + 2.0), 0.9 * u, c);
                line(at(1.0, y + 2.0), at(4.0, y + 2.0));
            }
        }
        Icon::Code => {
            line(at(-2.5, -5.0), at(-6.5, 0.0));
            line(at(-6.5, 0.0), at(-2.5, 5.0));
            line(at(2.5, -5.0), at(6.5, 0.0));
            line(at(6.5, 0.0), at(2.5, 5.0));
            line(at(1.2, -6.0), at(-1.2, 6.0));
        }
        Icon::Globe => {
            p.circle_stroke(ctr, 6.0 * u, s(c));
            line(at(-6.0, 0.0), at(6.0, 0.0));
            let pts: Vec<Pos2> = (0..=16).map(|i| {
                let a = -std::f32::consts::FRAC_PI_2 + i as f32 / 16.0 * std::f32::consts::PI;
                at(2.6 * a.cos(), 6.0 * a.sin())
            }).collect();
            p.add(egui::Shape::line(pts.clone(), s(c)));
            p.add(egui::Shape::line(pts.iter().map(|q| pos2(2.0 * ctr.x - q.x, q.y)).collect(), s(c)));
        }
        Icon::Lock | Icon::Unlock => {
            p.rect_stroke(Rect::from_min_max(at(-4.5, -0.5), at(4.5, 6.0)), 1.0, s(c), egui::StrokeKind::Middle);
            let lift = if icon == Icon::Unlock { -2.0 } else { 0.0 };
            let pts: Vec<Pos2> = (0..=12).map(|i| {
                let a = std::f32::consts::PI + i as f32 / 12.0 * std::f32::consts::PI;
                at(3.0 * a.cos(), -0.5 + lift + 3.5 * a.sin())
            }).collect();
            p.add(egui::Shape::line(pts, s(c)));
        }
        Icon::Search => {
            p.circle_stroke(at(-1.0, -1.0), 4.0 * u, s(c));
            line(at(2.0, 2.0), at(6.0, 6.0));
        }
        Icon::History => {
            p.circle_stroke(ctr, 6.0 * u, s(c));
            line(ctr, at(0.0, -4.0));
            line(ctr, at(3.0, 1.5));
        }
        Icon::Bookmark => {
            p.add(egui::Shape::closed_line(vec![at(-4.0, -6.0), at(4.0, -6.0), at(4.0, 6.0), at(0.0, 3.0), at(-4.0, 6.0)], s(c)));
        }
        Icon::Tab => {
            p.rect_stroke(Rect::from_min_max(at(-6.0, -4.0), at(6.0, 5.0)), 1.0, s(c), egui::StrokeKind::Middle);
            line(at(-6.0, -1.0), at(6.0, -1.0));
        }
        Icon::Chevron => {
            line(at(-2.0, -4.0), at(2.0, 0.0));
            line(at(2.0, 0.0), at(-2.0, 4.0));
        }
        Icon::ChevronDown => {
            line(at(-4.0, -2.0), at(0.0, 2.0));
            line(at(0.0, 2.0), at(4.0, -2.0));
        }
        Icon::Vpn => {
            p.circle_stroke(ctr, 6.0 * u, s(c));
            p.add(egui::Shape::line(vec![at(-3.0, 0.5), at(-0.8, 3.0), at(3.5, -2.5)], s(c)));
        }
        Icon::Gear => {
            p.circle_stroke(ctr, 2.5 * u, s(c));
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::FRAC_PI_4;
                line(at(4.0 * a.cos(), 4.0 * a.sin()), at(6.0 * a.cos(), 6.0 * a.sin()));
            }
            p.circle_stroke(ctr, 4.5 * u, s(c));
        }
        Icon::Layers => {
            for (i, y) in [-3.0, 0.5, 4.0].iter().enumerate() {
                let w = 6.0 - i as f32 * 0.0;
                p.add(egui::Shape::closed_line(vec![at(0.0, y - 2.5), at(w, *y), at(0.0, y + 2.5), at(-w, *y)], s(c)));
            }
        }
        Icon::Folder => {
            p.add(egui::Shape::closed_line(vec![at(-6.0, -4.0), at(-2.0, -4.0), at(-0.5, -2.5), at(6.0, -2.5), at(6.0, 5.0), at(-6.0, 5.0)], s(c)));
        }
    }
}

/// A square icon button with hover background. Returns the response.
pub fn button(ui: &mut Ui, icon: Icon, size: f32, tooltip: &str) -> Response {
    button_c(ui, icon, size, tooltip, crate::theme::FG, true)
}

pub fn button_c(ui: &mut Ui, icon: Icon, size: f32, tooltip: &str, color: Color32, enabled: bool) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), if enabled { Sense::click() } else { Sense::hover() });
    let resp = resp.on_hover_text(tooltip);
    if resp.hovered() && enabled {
        ui.painter().rect_filled(rect, 3.0, crate::theme::HOVER);
    }
    let c = if enabled { color } else { crate::theme::LINE_STRONG };
    paint(ui.painter(), rect.shrink(size * 0.22), icon, c);
    resp
}
