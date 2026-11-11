//! Site icons. One small image per host, kept as PNG files in the profile
//! and as textures once shown. They come from two places: the Chrome import
//! (Chrome's own icon cache, no network) and a fetch after the user visits a
//! site. Bookmarks never trigger a fetch on their own, so showing the
//! bookmarks bar contacts no one.

use crate::app::Event;
use eframe::egui::{self, ColorImage, TextureHandle, TextureOptions};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

const SIDE: u32 = 32;
const MAX_BYTES: usize = 400_000;

pub struct Favicons {
    dir: PathBuf,
    demo: bool,
    tex: HashMap<String, Option<TextureHandle>>,
    asked: HashSet<String>,
}

pub fn host_of(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    if !matches!(u.scheme(), "http" | "https") {
        return None;
    }
    Some(u.host_str()?.trim_start_matches("www.").to_ascii_lowercase())
}

fn file(dir: &Path, host: &str) -> PathBuf {
    let safe: String = host.chars().map(|c| if c.is_ascii_alphanumeric() || c == '.' || c == '-' { c } else { '_' }).collect();
    dir.join(format!("{safe}.png"))
}

/// Decodes an icon (ICO, PNG, JPEG, GIF, BMP or WebP) and scales it down.
pub fn decode(bytes: &[u8]) -> Option<ColorImage> {
    if bytes.len() > MAX_BYTES {
        return None;
    }
    let img = image::load_from_memory(bytes).ok()?;
    if img.width() == 0 || img.height() == 0 || img.width() > 1024 || img.height() > 1024 {
        return None;
    }
    let img = if img.width() > SIDE || img.height() > SIDE { img.resize(SIDE, SIDE, image::imageops::FilterType::Triangle) } else { img };
    let rgba = img.to_rgba8();
    Some(ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}

impl Favicons {
    pub fn new(profile_dir: PathBuf, demo: bool) -> Self {
        Favicons { dir: profile_dir.join("favicons"), demo, tex: HashMap::new(), asked: HashSet::new() }
    }

    /// The icon for a URL's site, if there is one in memory or on disk.
    pub fn get(&mut self, ctx: &egui::Context, url: &str) -> Option<egui::TextureId> {
        let host = host_of(url)?;
        if !self.tex.contains_key(&host) {
            let t = std::fs::read(file(&self.dir, &host)).ok().and_then(|b| decode(&b)).map(|img| ctx.load_texture(format!("fav:{host}"), img, TextureOptions::LINEAR));
            self.tex.insert(host.clone(), t);
        }
        self.tex.get(&host)?.as_ref().map(|t| t.id())
    }

    /// After a visit: fetch the site's icon unless we have it or already tried.
    pub fn want(&mut self, url: &str, tx: &Sender<Event>, ctx: &egui::Context) {
        let Some(host) = host_of(url) else { return };
        if self.demo || !self.asked.insert(host.clone()) || file(&self.dir, &host).exists() {
            return;
        }
        let (tx, ctx, https) = (tx.clone(), ctx.clone(), url.starts_with("https://"));
        let page = url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or(host.clone());
        std::thread::spawn(move || {
            if let Some(bytes) = fetch(&page, https) {
                let _ = tx.send(Event::Favicon { host, bytes });
                ctx.request_repaint();
            }
        });
    }

    /// Stores an icon (from the network or from Chrome's cache).
    pub fn put(&mut self, host: &str, bytes: &[u8]) {
        let Some(img) = decode(bytes) else { return };
        let _ = std::fs::create_dir_all(&self.dir);
        crate::drive::save_png(&file(&self.dir, host), &img);
        self.tex.remove(host);
    }
}

fn get(client: &reqwest::blocking::Client, url: &str) -> Option<Vec<u8>> {
    let r = client.get(url).send().ok()?;
    if !r.status().is_success() {
        return None;
    }
    let b = r.bytes().ok()?;
    (b.len() <= MAX_BYTES * 2 && !b.is_empty()).then(|| b.to_vec())
}

/// `/favicon.ico` first, then the icon the page itself names.
fn fetch(host: &str, https: bool) -> Option<Vec<u8>> {
    let client = reqwest::blocking::Client::builder().timeout(std::time::Duration::from_secs(8)).user_agent(crate::web::USER_AGENT).build().ok()?;
    let base = format!("{}://{host}", if https { "https" } else { "http" });
    if let Some(b) = get(&client, &format!("{base}/favicon.ico")).filter(|b| decode(b).is_some()) {
        return Some(b);
    }
    let html = String::from_utf8_lossy(&get(&client, &format!("{base}/"))?).into_owned();
    let href = icon_href(&html)?;
    let abs = url::Url::parse(&format!("{base}/")).ok()?.join(&href).ok()?;
    get(&client, abs.as_str()).filter(|b| decode(b).is_some())
}

/// The `href` of the first `<link rel="…icon…">` that isn't an SVG.
pub fn icon_href(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let mut at = 0;
    while let Some(i) = lower[at..].find("<link") {
        let start = at + i;
        let end = start + lower[start..].find('>')?;
        let tag = &lower[start..end];
        at = end;
        let attr = |name: &str| -> Option<String> {
            let p = tag.find(&format!("{name}="))? + name.len() + 1;
            let raw = &html[start + p..end];
            let q = raw.chars().next()?;
            if q == '"' || q == '\'' { raw[1..].split(q).next().map(str::to_owned) } else { raw.split_whitespace().next().map(str::to_owned) }
        };
        let rel = attr("rel").unwrap_or_default().to_ascii_lowercase();
        if rel.split_whitespace().any(|r| r == "icon" || r == "apple-touch-icon") {
            if let Some(h) = attr("href").filter(|h| !h.to_ascii_lowercase().contains(".svg") && !h.starts_with("data:")) {
                return Some(h.replace("&amp;", "&"));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts() {
        assert_eq!(host_of("https://www.github.com/a/b").as_deref(), Some("github.com"));
        assert!(host_of("inert://bank.example").is_none());
    }

    #[test]
    fn finds_the_page_icon() {
        let html = r#"<head><link rel="stylesheet" href="a.css"><link rel="icon" type="image/svg+xml" href="/i.svg"><link rel="alternate icon" href='/fav.png?v=2&amp;x=1'></head>"#;
        assert_eq!(icon_href(html).as_deref(), Some("/fav.png?v=2&x=1"));
        assert!(icon_href("<link rel=stylesheet href=a.css>").is_none());
    }

    #[test]
    fn rejects_junk() {
        assert!(decode(b"not an image").is_none());
    }
}
