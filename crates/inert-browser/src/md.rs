//! A small Markdown renderer for the assistant's replies: headings, lists,
//! quotes, rules, fenced code, and inline bold, italic, code and links.
//! Nothing here runs or loads anything: links are only styled text.

use crate::theme::{self, *};
use eframe::egui::{self, FontId, RichText, Stroke, TextFormat, Ui, text::LayoutJob, vec2};

fn fmt(size: f32, color: egui::Color32) -> TextFormat {
    TextFormat::simple(FontId::new(size, theme::sans()), color)
}

/// Lays out one line's inline markup.
pub fn inline(text: &str, size: f32, color: egui::Color32) -> LayoutJob {
    let mut job = LayoutJob::default();
    let chars: Vec<char> = text.chars().collect();
    let mut plain = String::new();
    let mut i = 0;
    // The index just past the next `pat` after `from`, if the span closes.
    let find = |from: usize, pat: &[char]| -> Option<usize> { (from..chars.len().saturating_sub(pat.len() - 1)).find(|&j| chars[j..j + pat.len()] == *pat) };
    let flush = |plain: &mut String, job: &mut LayoutJob| {
        if !plain.is_empty() {
            job.append(plain, 0.0, fmt(size, color));
            plain.clear();
        }
    };
    while i < chars.len() {
        let c = chars[i];
        if c == '`' {
            if let Some(end) = find(i + 1, &['`']) {
                flush(&mut plain, &mut job);
                let s: String = chars[i + 1..end].iter().collect();
                let mut f = TextFormat::simple(FontId::new(size - 1.0, theme::mono()), SECURE);
                f.background = RAISED;
                job.append(&s, 0.0, f);
                i = end + 1;
                continue;
            }
        }
        if c == '*' && chars.get(i + 1) == Some(&'*') {
            if let Some(end) = find(i + 2, &['*', '*']) {
                flush(&mut plain, &mut job);
                let s: String = chars[i + 2..end].iter().collect();
                job.append(&s, 0.0, TextFormat::simple(FontId::new(size, theme::sans_medium()), WHITE));
                i = end + 2;
                continue;
            }
        }
        if (c == '*' || c == '_') && chars.get(i + 1).is_some_and(|n| !n.is_whitespace()) && (i == 0 || !chars[i - 1].is_alphanumeric()) {
            if let Some(end) = find(i + 1, &[c]).filter(|&e| e > i + 1 && !chars[e - 1].is_whitespace()) {
                flush(&mut plain, &mut job);
                let s: String = chars[i + 1..end].iter().collect();
                let mut f = fmt(size, color);
                f.italics = true;
                job.append(&s, 0.0, f);
                i = end + 1;
                continue;
            }
        }
        if c == '[' {
            if let Some(close) = find(i + 1, &[']', '(']) {
                if let Some(end) = find(close + 2, &[')']) {
                    flush(&mut plain, &mut job);
                    let s: String = chars[i + 1..close].iter().collect();
                    let mut f = fmt(size, SECURE);
                    f.underline = Stroke::new(1.0, SECURE.gamma_multiply(0.6));
                    job.append(&s, 0.0, f);
                    i = end + 1;
                    continue;
                }
            }
        }
        plain.push(c);
        i += 1;
    }
    flush(&mut plain, &mut job);
    job
}

fn para(ui: &mut Ui, text: &str, size: f32, color: egui::Color32) {
    let mut job = inline(text, size, color);
    job.wrap.max_width = ui.available_width();
    ui.label(job);
}

/// Renders a reply.
pub fn render(ui: &mut Ui, text: &str) {
    ui.spacing_mut().item_spacing.y = 5.0;
    let mut code: Option<String> = None;
    let mut number = 0;
    for line in text.lines() {
        let t = line.trim_end();
        if t.trim_start().starts_with("```") {
            match code.take() {
                Some(body) => {
                    egui::Frame::new().fill(RAISED).stroke(Stroke::new(1.0, LINE)).corner_radius(10).inner_margin(egui::Margin::symmetric(12, 9)).show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.label(RichText::new(body.trim_end()).font(FontId::new(12.5, theme::mono())).color(FG));
                    });
                }
                None => code = Some(String::new()),
            }
            continue;
        }
        if let Some(body) = &mut code {
            body.push_str(line);
            body.push('\n');
            continue;
        }
        let s = t.trim_start();
        let indent = (t.len() - s.len()) as f32 * 4.0;
        if s.is_empty() {
            ui.add_space(3.0);
            number = 0;
        } else if let Some(h) = s.strip_prefix("### ") {
            ui.add_space(4.0);
            para(ui, h, 15.0, WHITE);
        } else if let Some(h) = s.strip_prefix("## ") {
            ui.add_space(6.0);
            ui.label(RichText::new(h.replace("**", "")).font(FontId::new(17.0, theme::sans_medium())).color(WHITE));
        } else if let Some(h) = s.strip_prefix("# ") {
            ui.add_space(6.0);
            ui.label(RichText::new(h.replace("**", "")).font(FontId::new(20.0, theme::sans_medium())).color(WHITE));
        } else if s == "---" || s == "***" {
            ui.separator();
        } else if let Some(q) = s.strip_prefix("> ") {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(vec2(3.0, 18.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 1.5, LINE_STRONG);
                ui.vertical(|ui| para(ui, q, 14.0, MUTED));
            });
        } else if let Some(item) = s.strip_prefix("- ").or_else(|| s.strip_prefix("* ")).or_else(|| s.strip_prefix("• ")) {
            ui.horizontal_top(|ui| {
                ui.add_space(4.0 + indent);
                ui.label(RichText::new("•").font(FontId::new(14.0, theme::sans())).color(ACCENT));
                ui.vertical(|ui| para(ui, item, 14.0, FG));
            });
        } else if let Some((n, item)) = s.split_once(". ").filter(|(n, _)| !n.is_empty() && n.len() <= 3 && n.chars().all(|c| c.is_ascii_digit())) {
            number += 1;
            let _ = number;
            ui.horizontal_top(|ui| {
                ui.add_space(4.0 + indent);
                ui.label(RichText::new(format!("{n}.")).font(FontId::new(13.0, theme::mono())).color(ACCENT));
                ui.vertical(|ui| para(ui, item, 14.0, FG));
            });
        } else {
            para(ui, s, 14.0, FG);
        }
    }
    // An unclosed fence still shows its code.
    if let Some(body) = code {
        ui.label(RichText::new(body.trim_end()).font(FontId::new(12.5, theme::mono())).color(FG));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(j: &LayoutJob) -> String {
        j.text.clone()
    }

    #[test]
    fn inline_markup_is_stripped_from_the_text() {
        let j = inline("a **bold** and `code` and *it* and [link](https://x.y)", 14.0, FG);
        assert_eq!(text_of(&j), "a bold and code and it and link");
        assert!(j.sections.len() >= 7);
    }

    #[test]
    fn unclosed_markers_stay_literal() {
        assert_eq!(text_of(&inline("2 * 3 and a ** b and `open", 14.0, FG)), "2 * 3 and a ** b and `open");
        assert_eq!(text_of(&inline("snake_case_name", 14.0, FG)), "snake_case_name");
    }
}
