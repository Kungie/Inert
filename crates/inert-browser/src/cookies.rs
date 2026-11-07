//! Cookies, a built-in extension: see and delete the cookies of the page
//! you're on, keep no cookies for chosen sites, and reject cookie banners
//! automatically. Works on both engines: WebKit through its cookie store
//! (WKHTTPCookieStore), Servo through the helper process.
//!
//! Cookie values are never shown or kept by the manager; it only reads
//! names, domains and flags.

use crate::app::{Browser, Event, Kind};
use eframe::egui;
use inert_servo_ipc::CookieJson;
use std::collections::HashMap;
use std::sync::mpsc::Sender;

/// Clicks the "reject" / "only necessary" button of common consent banners
/// (OneTrust, Cookiebot, Didomi, Quantcast, Usercentrics, Google, …), or a
/// button with that text inside something that looks like a cookie banner.
/// Retries for ten seconds, since banners often arrive late. Records what it
/// did in `window.__inertConsent`.
pub const BANNER_JS: &str = r#"(function(){
if (window.__inertCB) return; window.__inertCB = 1;
var sels = ['#onetrust-reject-all-handler','.ot-pc-refuse-all-handler','#CybotCookiebotDialogBodyButtonDecline',
 '#CybotCookiebotDialogBodyLevelButtonLevelOptinDeclineAll','#didomi-notice-disagree-button','.didomi-continue-without-agreeing',
 '.qc-cmp2-summary-buttons button[mode="secondary"]','[data-testid="uc-deny-all-button"]','#uc-btn-deny-banner','.cmplz-deny',
 '.cky-btn-reject','.osano-cm-denyAll','.fc-cta-do-not-consent','#sp-cc-rejectall-link','.js-reject-cookies','#cookie-reject',
 'button[data-cookiebanner="accept_only_essential_button"]','button[data-cookie-reject]','.truste-consent-required'];
var words = /^(reject all|reject|decline all|decline|deny all|deny|refuse all|refuse|only necessary|necessary only|only essential|use necessary cookies only|alle ablehnen|ablehnen|nur notwendige|nur erforderliche|tümünü reddet|reddet|tout refuser|refuser|rifiuta tutto|rifiuta|rechazar todo|rechazar|weigeren)\b/i;
var box = '[class*=cookie],[id*=cookie],[class*=consent],[id*=consent],[class*=gdpr],[id*=gdpr],[class*=cmp],[id*=cmp],[class*=privacy],[aria-modal=true],[role=dialog]';
function vis(e){ return e && e.offsetParent !== null; }
function run(){
  for (var i = 0; i < sels.length; i++) { var e = document.querySelector(sels[i]); if (vis(e)) { e.click(); window.__inertConsent = 'rejected'; return true; } }
  var bs = document.querySelectorAll('button,[role=button],a[class*=button],a[class*=btn]');
  for (var j = 0; j < bs.length && j < 500; j++) {
    var b = bs[j], t = (b.innerText || b.textContent || '').trim();
    if (t.length < 48 && words.test(t) && vis(b) && b.closest(box)) { b.click(); window.__inertConsent = 'rejected'; return true; }
  }
  return false;
}
var n = 0, iv = setInterval(function(){ if (run() || ++n > 20) clearInterval(iv); }, 500);
run();
})();"#;

/// What the banner script did on this page ("rejected" or "").
pub const CONSENT_QUERY: &str = "String(window.__inertConsent || '')";

#[derive(Default)]
pub struct State {
    /// The page the list below belongs to.
    pub url: String,
    pub host: String,
    pub list: Vec<CookieJson>,
    pub loading: bool,
    pub req: u64,
    /// Per tab: what the banner script did.
    pub consent: HashMap<u64, String>,
    /// Every WebKit cookie, for the settings page (names and domains only).
    pub all: Vec<CookieJson>,
}

pub fn host_of(url: &str) -> String {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_default()
}

/// The registrable part of a host ("mail.google.com" → "google.com",
/// "www.bbc.co.uk" → "bbc.co.uk"). A short list instead of the full public
/// suffix list; good enough to group and delete cookies by site.
pub fn site_of(host: &str) -> String {
    let h = host.trim_start_matches('.').to_ascii_lowercase();
    let parts: Vec<&str> = h.split('.').filter(|p| !p.is_empty()).collect();
    if parts.len() <= 2 || h.parse::<std::net::IpAddr>().is_ok() {
        return h;
    }
    let n = parts.len();
    let second = parts[n - 2];
    let take = if parts[n - 1].len() == 2 && matches!(second, "co" | "com" | "org" | "net" | "ac" | "gov" | "edu" | "gen" | "ne" | "or") { 3 } else { 2 };
    parts[n - take..].join(".")
}

/// Whether a cookie with this domain is sent to `host`.
pub fn domain_matches(cookie_domain: &str, host: &str) -> bool {
    let d = cookie_domain.trim_start_matches('.').to_ascii_lowercase();
    let h = host.to_ascii_lowercase();
    !d.is_empty() && (h == d || h.ends_with(&format!(".{d}")))
}

impl Browser {
    /// Reloads the cookie list for the active tab.
    pub fn cookies_refresh(&mut self) {
        let id = self.active;
        let Some(Kind::Web(w)) = self.tab(id).map(|t| &t.kind) else {
            self.cookie = State { consent: std::mem::take(&mut self.cookie.consent), all: std::mem::take(&mut self.cookie.all), ..Default::default() };
            return;
        };
        let url = w.url.clone();
        let on_servo = w.sv.is_some();
        self.cookie.host = host_of(&url);
        self.cookie.url = url.clone();
        self.cookie.list.clear();
        self.cookie.loading = true;
        let Some(Kind::Web(w)) = self.tabs.iter().find(|t| t.id == id).map(|t| &t.kind) else { return };
        if on_servo {
            let Some(sv) = &w.sv else { return };
            crate::servo_web::read_consent(sv, id);
            if let Some(h) = &self.servo {
                self.cookie.req = h.cookies(&url);
            }
        } else {
            #[cfg(target_os = "macos")]
            wk::all(self.tx.clone(), self.ctx.clone());
            if let Some(v) = &w.view {
                let tx = self.tx.clone();
                let ctx = self.ctx.clone();
                let _ = v.evaluate_script_with_callback(CONSENT_QUERY, move |s| {
                    let what = serde_json::from_str::<String>(&s).unwrap_or(s);
                    let _ = tx.send(Event::Consent { tab: id, what });
                    ctx.request_repaint();
                });
            }
        }
    }

    /// All WebKit cookies, for the settings page.
    pub fn cookies_refresh_all(&mut self) {
        #[cfg(target_os = "macos")]
        wk::all(self.tx.clone(), self.ctx.clone());
    }

    pub fn on_wk_cookies(&mut self, list: Vec<CookieJson>) {
        let host = self.cookie.host.clone();
        let on_servo = matches!(self.active_tab().map(|t| &t.kind), Some(Kind::Web(w)) if w.sv.is_some());
        if !on_servo && !host.is_empty() {
            self.cookie.list = list.iter().filter(|c| domain_matches(&c.domain, &host)).cloned().collect();
            self.cookie.loading = false;
        }
        self.cookie.all = list;
    }

    pub fn on_servo_cookies(&mut self, req: u64, list: Vec<CookieJson>) {
        if req == self.cookie.req {
            self.cookie.list = list;
            self.cookie.loading = false;
        }
    }

    /// Deletes one cookie (WebKit tabs; Servo deletes per site).
    pub fn cookie_delete(&mut self, c: &CookieJson) {
        #[cfg(target_os = "macos")]
        {
            let (n, d, p) = (c.name.clone(), c.domain.clone(), c.path.clone());
            wk::delete(move |x| x.name == n && x.domain == d && x.path == p);
        }
        self.cookie.list.retain(|x| x != c);
    }

    /// Deletes every cookie and site storage of a site, in both engines.
    pub fn cookies_clear_site(&mut self, site: &str) {
        #[cfg(target_os = "macos")]
        {
            let s = site.to_owned();
            wk::delete(move |x| site_of(&x.domain) == s);
        }
        if let Some(h) = &self.servo {
            h.clear_cookies(vec![site.to_owned()]);
        }
        self.cookie.list.clear();
        self.cookie.all.retain(|x| site_of(&x.domain) != site);
    }

    /// After a web page finishes loading: banner rejection and the
    /// "keep no cookies" list.
    pub fn after_web_load(&mut self, tab: u64, url: &str) {
        let site = site_of(&host_of(url));
        if self.profile.settings.cookie_banners {
            if let Some(Kind::Web(w)) = self.tab(tab).map(|t| &t.kind) {
                crate::web::js(w, BANNER_JS);
            }
        }
        if !site.is_empty() && self.profile.settings.cookie_block.iter().any(|s| *s == site) {
            self.cookies_clear_site(&site);
        }
    }
}

/// Deletes every cookie and all website data, in both engines.
pub fn clear_all(b: &mut Browser) {
    #[cfg(target_os = "macos")]
    wk::clear_all();
    if let Some(h) = &b.servo {
        h.clear_cookies(Vec::new());
    }
    b.cookie.list.clear();
    b.cookie.all.clear();
}

/// Adds imported cookies to both engines.
pub fn import(b: &mut Browser, list: &[(String, CookieJson)]) {
    #[cfg(target_os = "macos")]
    wk::set(list.iter().map(|(_, c)| c));
    if let Some(h) = &b.servo {
        h.set_cookies(list.to_vec());
    } else {
        // Servo starts on first use; it gets them then.
        b.pending_servo_cookies = list.to_vec();
    }
}

#[cfg(target_os = "macos")]
pub mod wk {
    //! WebKit's shared cookie store (all WKWebViews use the default data store).
    use super::*;
    use block2::RcBlock;
    use objc2::MainThreadMarker;
    use objc2::rc::Retained;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSArray, NSDate, NSDictionary, NSHTTPCookie, NSString};
    use objc2_web_kit::{WKHTTPCookieStore, WKWebsiteDataStore};
    use std::ptr::NonNull;

    fn store() -> Option<Retained<WKHTTPCookieStore>> {
        let mtm = MainThreadMarker::new()?;
        // SAFETY: plain getters on the main thread.
        Some(unsafe { WKWebsiteDataStore::defaultDataStore(mtm).httpCookieStore() })
    }

    fn json(c: &NSHTTPCookie) -> CookieJson {
        CookieJson {
            name: c.name().to_string(),
            value: String::new(),
            domain: c.domain().to_string(),
            path: c.path().to_string(),
            expires: c.expiresDate().map(|d| d.timeIntervalSince1970() as i64),
            secure: c.isSecure(),
            http_only: c.isHTTPOnly(),
            same_site: c.sameSitePolicy().map(|s| s.to_string()),
        }
    }

    /// Sends every cookie (without values) as `Event::WkCookies`.
    pub fn all(tx: Sender<Event>, ctx: egui::Context) {
        let Some(s) = store() else { return };
        let block = RcBlock::new(move |arr: NonNull<NSArray<NSHTTPCookie>>| {
            // SAFETY: WebKit passes a valid array for the duration of the call.
            let arr = unsafe { arr.as_ref() };
            let list: Vec<CookieJson> = arr.iter().map(|c| json(&c)).collect();
            let _ = tx.send(Event::WkCookies(list));
            ctx.request_repaint();
        });
        // SAFETY: main thread; the block lives as long as WebKit needs it.
        unsafe { s.getAllCookies(&block) };
    }

    pub fn delete(pred: impl Fn(&CookieJson) -> bool + 'static) {
        let Some(s) = store() else { return };
        let s2 = s.clone();
        let block = RcBlock::new(move |arr: NonNull<NSArray<NSHTTPCookie>>| {
            // SAFETY: as above.
            for c in unsafe { arr.as_ref() }.iter() {
                if pred(&json(&c)) {
                    unsafe { s2.deleteCookie_completionHandler(&c, None) };
                }
            }
        });
        unsafe { s.getAllCookies(&block) };
    }

    pub fn clear_all() {
        let Some(mtm) = MainThreadMarker::new() else { return };
        // SAFETY: main thread; the empty completion block is retained by WebKit.
        unsafe {
            let ds = WKWebsiteDataStore::defaultDataStore(mtm);
            let types = WKWebsiteDataStore::allWebsiteDataTypes(mtm);
            let done = RcBlock::new(|| {});
            ds.removeDataOfTypes_modifiedSince_completionHandler(&types, &NSDate::distantPast(), &done);
        }
    }

    pub fn set<'a>(list: impl Iterator<Item = &'a CookieJson>) {
        let Some(s) = store() else { return };
        for c in list {
            let http_only_key = NSString::from_str("HttpOnly");
            let mut keys: Vec<&NSString> = Vec::new();
            let mut vals: Vec<Retained<AnyObject>> = Vec::new();
            let mut put = |k: &'static NSString, v: Retained<AnyObject>| {
                keys.push(k);
                vals.push(v);
            };
            let st = |x: &str| -> Retained<AnyObject> { Retained::into_super(Retained::into_super(NSString::from_str(x))) };
            // SAFETY: the property keys are Foundation's own constants.
            unsafe {
                use objc2_foundation::*;
                put(NSHTTPCookieName, st(&c.name));
                put(NSHTTPCookieValue, st(&c.value));
                put(NSHTTPCookieDomain, st(&c.domain));
                put(NSHTTPCookiePath, st(&c.path));
                if c.secure {
                    put(NSHTTPCookieSecure, st("TRUE"));
                }
                if let Some(t) = c.expires {
                    put(NSHTTPCookieExpires, Retained::into_super(Retained::into_super(NSDate::dateWithTimeIntervalSince1970(t as f64))));
                }
                if let Some(ss) = c.same_site.as_deref() {
                    put(NSHTTPCookieSameSitePolicy, st(&ss.to_ascii_lowercase()));
                }
            }
            if c.http_only {
                keys.push(&http_only_key);
                vals.push(st("TRUE"));
            }
            let dict = NSDictionary::from_retained_objects(&keys, &vals);
            // SAFETY: a dictionary of NSString keys to NSString/NSDate values, as NSHTTPCookie expects.
            if let Some(cookie) = unsafe { NSHTTPCookie::cookieWithProperties(&dict) } {
                unsafe { s.setCookie_completionHandler(&cookie, None) };
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sites_and_domains() {
        assert_eq!(site_of("mail.google.com"), "google.com");
        assert_eq!(site_of(".github.com"), "github.com");
        assert_eq!(site_of("www.bbc.co.uk"), "bbc.co.uk");
        assert_eq!(site_of("localhost"), "localhost");
        assert_eq!(site_of("192.168.1.1"), "192.168.1.1");
        assert!(domain_matches(".google.com", "mail.google.com"));
        assert!(domain_matches("github.com", "github.com"));
        assert!(!domain_matches("hub.com", "github.com"));
        assert!(!domain_matches("", "github.com"));
    }
}
