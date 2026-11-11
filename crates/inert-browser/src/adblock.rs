//! Native ad and tracker blocking for web tabs.
//!
//! EasyList + EasyPrivacy are converted with Brave's `adblock` crate into
//! WebKit content-blocker rules, compiled once by WebKit
//! (`WKContentRuleListStore`) and attached to every web view. Blocking then
//! happens inside WebKit's network layer — no per-request Rust code, no JS.
//! `inert://` pages need none of this: they cannot load third-party code.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

/// The lists uBlock Origin ships with by default: EasyList and EasyPrivacy,
/// plus uBlock Origin's own filters. The first two must download; a uBlock
/// list that fails to download is skipped until the next refresh.
const LISTS: [(&str, &str); 7] = [
    ("easylist", "https://easylist.to/easylist/easylist.txt"),
    ("ublock-filters", "https://ublockorigin.github.io/uAssets/filters/filters.min.txt"),
    ("ublock-unbreak", "https://ublockorigin.github.io/uAssets/filters/unbreak.min.txt"),
    ("ublock-quick-fixes", "https://ublockorigin.github.io/uAssets/filters/quick-fixes.min.txt"),
    ("ublock-badware", "https://ublockorigin.github.io/uAssets/filters/badware.min.txt"),
    ("ublock-privacy", "https://ublockorigin.github.io/uAssets/filters/privacy.min.txt"),
    ("easyprivacy", "https://easylist.to/easylist/easyprivacy.txt"),
];
/// WebKit accepts at most 150k rules per list; stay below it.
const MAX_RULES: usize = 140_000;
const RULES_VERSION: &str = "inert-adblock-v3";

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    Off,
    Preparing,
    Compiling(usize),
    Ready(usize),
    Failed(String),
}

fn cache_dir() -> PathBuf {
    dirs::data_dir().unwrap_or_else(std::env::temp_dir).join("Inert").join("adblock")
}

/// Background part: fetch lists (cached for 4 days) and convert to WebKit JSON.
/// Sends `Event::AdblockRules(json, count)` or `Event::AdblockFailed`.
pub fn prepare(tx: Sender<crate::app::Event>, ctx: eframe::egui::Context) {
    std::thread::spawn(move || {
        let r = (|| -> anyhow::Result<(String, usize)> {
            let dir = cache_dir();
            std::fs::create_dir_all(&dir)?;
            let json_path = dir.join(format!("{RULES_VERSION}.json"));
            let fresh = |p: &PathBuf| std::fs::metadata(p).and_then(|m| m.modified()).ok().and_then(|t| t.elapsed().ok()).is_some_and(|age| age.as_secs() < 4 * 86400);
            if fresh(&json_path) {
                let s = std::fs::read_to_string(&json_path)?;
                let n = s.matches("\"trigger\"").count();
                return Ok((s, n));
            }
            let client = reqwest::blocking::Client::builder().user_agent("InertBrowser/0.1 (filter list update)").timeout(std::time::Duration::from_secs(30)).build()?;
            let mut set = adblock::lists::FilterSet::new(true);
            for (name, url) in LISTS {
                let p = dir.join(format!("{name}.txt"));
                let text = if fresh(&p) {
                    std::fs::read_to_string(&p)?
                } else {
                    match client.get(url).send().and_then(|r| r.error_for_status()).and_then(|r| r.text()) {
                        Ok(t) => {
                            std::fs::write(&p, &t)?;
                            t
                        }
                        // An old copy is better than none; only the EasyList pair is required.
                        Err(e) => match std::fs::read_to_string(&p) {
                            Ok(t) => t,
                            Err(_) if name.starts_with("ublock-") => continue,
                            Err(_) => return Err(e.into()),
                        },
                    }
                };
                set.add_filter_list(text, adblock::lists::ParseOptions::default());
            }
            let (mut rules, _) = set.into_content_blocking().map_err(|_| anyhow::anyhow!("conversion failed"))?;
            // Generic hiding rules (no site named) are left out. uBlock Origin
            // applies them with per-site exceptions WebKit's rule format loses
            // ("##.ads" hid Gmail's message bodies, `div.adn.ads`). Network
            // blocking and site-specific hiding stay.
            rules.retain(|r| {
                let v = serde_json::to_value(r).unwrap_or_default();
                let hide = v.pointer("/action/type").and_then(|t| t.as_str()) == Some("css-display-none");
                let sited = v.pointer("/trigger/if-domain").and_then(|d| d.as_array()).is_some_and(|a| !a.is_empty());
                !hide || sited
            });
            if rules.len() > MAX_RULES {
                rules.truncate(MAX_RULES);
            }
            let n = rules.len();
            let json = serde_json::to_string(&rules)?;
            std::fs::write(&json_path, &json)?;
            Ok((json, n))
        })();
        let _ = tx.send(match r {
            Ok((json, n)) => crate::app::Event::AdblockRules(json, n),
            Err(e) => crate::app::Event::AdblockFailed(e.to_string()),
        });
        ctx.request_repaint();
    });
}

#[cfg(target_os = "macos")]
pub mod mac {
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::MainThreadMarker;
    use objc2_foundation::{NSError, NSString};
    use objc2_web_kit::{WKContentRuleList, WKContentRuleListStore};
    use std::cell::RefCell;
    use std::rc::Rc;

    pub type Slot = Rc<RefCell<Option<Result<Retained<WKContentRuleList>, String>>>>;

    /// Compiles (or reuses WebKit's cached compilation of) the rule list.
    /// Must be called on the main thread. The result lands in `slot`.
    pub fn compile(json: &str, slot: Slot, ctx: eframe::egui::Context) {
        let Some(mtm) = MainThreadMarker::new() else {
            *slot.borrow_mut() = Some(Err("not on the main thread".into()));
            return;
        };
        let Some(store) = (unsafe { WKContentRuleListStore::defaultStore(mtm) }) else {
            *slot.borrow_mut() = Some(Err("no content rule list store".into()));
            return;
        };
        // The identifier changes whenever the converted rules change, so a
        // cached compilation under the same id is always the right one.
        let id = NSString::from_str(&format!("{}-{}", super::RULES_VERSION, json.len()));
        let json = Rc::new(json.to_owned());
        let lookup_store = store.clone();
        let lookup_id = id.clone();
        let lookup = RcBlock::new(move |list: *mut WKContentRuleList, _err: *mut NSError| {
            if !list.is_null() {
                if let Some(l) = unsafe { Retained::retain(list) } {
                    *slot.borrow_mut() = Some(Ok(l));
                    ctx.request_repaint();
                    return;
                }
            }
            // Not compiled yet (first launch or new lists): compile once.
            let (slot, ctx) = (slot.clone(), ctx.clone());
            let done = RcBlock::new(move |list: *mut WKContentRuleList, err: *mut NSError| {
                let r = if !list.is_null() {
                    match unsafe { Retained::retain(list) } {
                        Some(l) => Ok(l),
                        None => Err("null rule list".to_owned()),
                    }
                } else if !err.is_null() {
                    Err(unsafe { (*err).localizedDescription() }.to_string())
                } else {
                    Err("unknown WebKit error".to_owned())
                };
                *slot.borrow_mut() = Some(r);
                ctx.request_repaint();
            });
            let encoded = NSString::from_str(&json);
            unsafe { lookup_store.compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(Some(&lookup_id), Some(&encoded), Some(&done)) };
        });
        unsafe { store.lookUpContentRuleListForIdentifier_completionHandler(Some(&id), Some(&lookup)) };
    }

    pub fn attach(view: &wry::WebView, list: &WKContentRuleList) {
        use wry::WebViewExtMacOS;
        let mgr = view.manager();
        unsafe { mgr.addContentRuleList(list) };
    }

    pub fn detach(view: &wry::WebView) {
        use wry::WebViewExtMacOS;
        let mgr = view.manager();
        unsafe { mgr.removeAllContentRuleLists() };
    }
}
