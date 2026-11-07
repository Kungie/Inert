//! The omnibox: URL vs search detection, ranked suggestions from open tabs,
//! bookmarks, history and inert sites, inline completion of what you type,
//! and (optionally) the search engine's remote suggestions.

use crate::app::Event;
use crate::profile::{Profile, SearchEngine};
use std::sync::mpsc::Sender;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Url,
    Search,
    Remote,
    History,
    Bookmark,
    OpenTab,
    Inert,
    Ai,
}

#[derive(Clone, Debug)]
pub struct Suggestion {
    pub kind: Kind,
    pub title: String,
    /// URL to open, search text, tab id (as string) or AI question.
    pub target: String,
}

pub enum Nav {
    Url(String),
    Search(String),
}

pub fn classify(text: &str) -> Nav {
    let t = text.trim();
    let lower = t.to_ascii_lowercase();
    if ["http://", "https://", "inert://", "about:", "file://"].iter().any(|p| lower.starts_with(p)) {
        return Nav::Url(t.to_owned());
    }
    if t.contains(' ') || t.is_empty() {
        return Nav::Search(t.to_owned());
    }
    let host = lower.split(['/', '?', '#']).next().unwrap_or("");
    let host_no_port = host.split(':').next().unwrap_or("");
    let is_ip = host_no_port.split('.').count() == 4 && host_no_port.split('.').all(|p| p.parse::<u8>().is_ok());
    if host_no_port == "localhost" || host_no_port.ends_with(".local") || is_ip {
        return Nav::Url(format!("http://{t}"));
    }
    if host_no_port.ends_with(".example") || host_no_port.ends_with(".inert") {
        return Nav::Url(format!("inert://{t}"));
    }
    let tld_ok = host_no_port.rsplit('.').next().is_some_and(|tld| tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic())) && host_no_port.contains('.');
    if tld_ok { Nav::Url(format!("https://{t}")) } else { Nav::Search(t.to_owned()) }
}

pub fn strip(url: &str) -> &str {
    let u = url.split_once("://").map(|x| x.1).unwrap_or(url);
    let u = u.strip_prefix("www.").unwrap_or(u);
    u.strip_suffix('/').unwrap_or(u)
}

/// Full text to show when the typed text can be completed inline, e.g.
/// typing `gith` → `github.com`. The caller selects the added part.
pub fn inline_completion(p: &Profile, typed: &str) -> Option<String> {
    if typed.len() < 2 || typed.contains(' ') {
        return None;
    }
    let t = typed.to_ascii_lowercase();
    let mut best: Option<(u32, String)> = None;
    let mut consider = |url: &str, score: u32| {
        let s = strip(url);
        if s.to_ascii_lowercase().starts_with(&t) && s.len() > t.len() {
            // Complete to the host first, like Chrome does for a partial host.
            let host_end = s.find('/').unwrap_or(s.len());
            let cand = if t.len() < host_end { &s[..host_end] } else { s };
            if best.as_ref().is_none_or(|(b, _)| score > *b) {
                best = Some((score, cand.to_owned()));
            }
        }
    };
    for h in &p.history {
        consider(&h.url, h.visits * 2 + 1);
    }
    for b in &p.bookmarks {
        consider(&b.url, 50);
    }
    best.map(|(_, c)| format!("{typed}{}", &c[typed.len()..]))
}

fn score(hay: &str, words: &[String]) -> Option<u32> {
    let h = hay.to_lowercase();
    let mut s = 0;
    for w in words {
        let i = h.find(w.as_str())?;
        s += if i == 0 { 30 } else if h[..i].ends_with(['.', '/', ' ', '-']) { 20 } else { 8 };
    }
    Some(s)
}

/// Local suggestions, best first.
pub fn local(p: &Profile, tabs: &[(u64, String, String)], inert_sites: &[(String, String)], text: &str, ai_name: Option<&str>) -> Vec<Suggestion> {
    let words: Vec<String> = text.split_whitespace().map(str::to_lowercase).collect();
    let mut out: Vec<(u32, Suggestion)> = Vec::new();
    let mut seen = std::collections::HashSet::new();
    if words.is_empty() {
        return Vec::new();
    }
    match classify(text) {
        Nav::Url(u) => out.push((1000, Suggestion { kind: Kind::Url, title: u.clone(), target: u })),
        Nav::Search(q) => out.push((900, Suggestion { kind: Kind::Search, title: q.clone(), target: q })),
    }
    for (host, name) in inert_sites {
        if let Some(s) = score(&format!("{host} {name} inert"), &words) {
            let url = format!("inert://{host}");
            if seen.insert(url.clone()) {
                out.push((400 + s, Suggestion { kind: Kind::Inert, title: format!("{name} — signed inert:// site"), target: url }));
            }
        }
    }
    for (id, title, url) in tabs {
        if let Some(s) = score(&format!("{title} {url}"), &words) {
            out.push((300 + s, Suggestion { kind: Kind::OpenTab, title: format!("Switch to tab · {title}"), target: id.to_string() }));
        }
    }
    for b in &p.bookmarks {
        if let Some(s) = score(&format!("{} {}", b.title, strip(&b.url)), &words) {
            if seen.insert(b.url.clone()) {
                out.push((250 + s, Suggestion { kind: Kind::Bookmark, title: b.title.clone(), target: b.url.clone() }));
            }
        }
    }
    for h in &p.history {
        if let Some(s) = score(&format!("{} {}", h.title, strip(&h.url)), &words) {
            if seen.insert(h.url.clone()) {
                out.push((100 + s + h.visits.min(50), Suggestion { kind: Kind::History, title: if h.title.is_empty() { strip(&h.url).to_owned() } else { h.title.clone() }, target: h.url.clone() }));
            }
        }
    }
    if let Some(ai) = ai_name {
        if text.trim().len() > 3 {
            out.push((120, Suggestion { kind: Kind::Ai, title: format!("Ask {ai}: {}", text.trim()), target: text.trim().to_owned() }));
        }
    }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out.into_iter().map(|x| x.1).take(9).collect()
}

/// Fetches the search engine's suggestions in the background.
pub fn remote(engine: SearchEngine, q: String, tx: Sender<Event>, ctx: eframe::egui::Context) {
    std::thread::spawn(move || {
        let enc: String = url::form_urlencoded::byte_serialize(q.as_bytes()).collect();
        let url = match engine {
            SearchEngine::DuckDuckGo => format!("https://duckduckgo.com/ac/?q={enc}&type=list"),
            SearchEngine::Google => format!("https://suggestqueries.google.com/complete/search?client=firefox&q={enc}"),
            SearchEngine::Brave => format!("https://search.brave.com/api/suggest?q={enc}"),
        };
        let r = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .user_agent(crate::web::USER_AGENT)
            .build()
            .ok()
            .and_then(|c| c.get(url).send().ok())
            .and_then(|r| r.json::<serde_json::Value>().ok());
        let list: Vec<String> = r.and_then(|v| v.get(1).and_then(|a| a.as_array().cloned())).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_owned)).take(5).collect()).unwrap_or_default();
        let _ = tx.send(Event::Remote(q, list));
        ctx.request_repaint();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies() {
        assert!(matches!(classify("github.com"), Nav::Url(u) if u == "https://github.com"));
        assert!(matches!(classify("localhost:3000"), Nav::Url(u) if u == "http://localhost:3000"));
        assert!(matches!(classify("bank.example"), Nav::Url(u) if u == "inert://bank.example"));
        assert!(matches!(classify("rust borrow checker"), Nav::Search(_)));
        assert!(matches!(classify("192.168.1.1"), Nav::Url(u) if u == "http://192.168.1.1"));
        assert!(matches!(classify("inert://notes.example"), Nav::Url(_)));
    }

    #[test]
    fn completes_inline() {
        let mut p = Profile::default();
        p.record_visit("https://github.com/anthropics", "GitHub");
        assert_eq!(inline_completion(&p, "gith").as_deref(), Some("github.com"));
        assert_eq!(inline_completion(&p, "github.com/a").as_deref(), Some("github.com/anthropics"));
        assert_eq!(inline_completion(&p, "x"), None);
    }
}
