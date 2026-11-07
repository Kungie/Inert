//! Native ad and tracker blocking for web tabs.
//!
//! EasyList + EasyPrivacy are converted with Brave's `adblock` crate into
//! WebKit content-blocker rules, compiled once by WebKit
//! (`WKContentRuleListStore`) and attached to every web view. Blocking then
//! happens inside WebKit's network layer — no per-request Rust code, no JS.
//! `inert://` pages need none of this: they cannot load third-party code.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

const LISTS: [(&str, &str); 2] = [("easylist", "https://easylist.to/easylist/easylist.txt"), ("easyprivacy", "https://easylist.to/easylist/easyprivacy.txt")];
/// WebKit accepts at most 150k rules per list; stay below it.
const MAX_RULES: usize = 140_000;
const RULES_VERSION: &str = "inert-adblock-v1";

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
                    let t = client.get(url).send()?.error_for_status()?.text()?;
                    std::fs::write(&p, &t)?;
                    t
                };
                set.add_filter_list(text, adblock::lists::ParseOptions::default());
            }
            let (mut rules, _) = set.into_content_blocking().map_err(|_| anyhow::anyhow!("conversion failed"))?;
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
        let id = NSString::from_str(&format!("{}-{}", super::RULES_VERSION, json.len()));
        let encoded = NSString::from_str(json);
        let block = RcBlock::new(move |list: *mut WKContentRuleList, err: *mut NSError| {
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
        unsafe { store.compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(Some(&id), Some(&encoded), Some(&block)) };
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
