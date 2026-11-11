//! Import from Chrome (and other Chromium browsers: Brave, Edge, Arc,
//! Vivaldi, Chromium): bookmarks, history, saved passwords, cookies (so you
//! stay signed in) and habits (search engine, bookmarks bar, startup pages).
//!
//! Chrome's databases are SQLite. They are copied first (Chrome keeps them
//! locked) and read with the system's `sqlite3` tool in its own process, so
//! no SQLite parser runs inside the browser. Passwords and cookies are
//! encrypted with a key Chrome keeps in the macOS Keychain ("Chrome Safe
//! Storage"); reading it makes macOS ask the user first.

use crate::profile::{Bookmark, HistoryEntry, SearchEngine, Visit};
use aws_lc_rs::cipher::{AES_128, DecryptionContext, PaddedBlockDecryptingKey, UnboundCipherKey};
use aws_lc_rs::iv::FixedLength;
use inert_servo_ipc::CookieJson;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The cookie query the import runs (also checked against real Chromium files).
pub const COOKIES_SQL: &str = "SELECT host_key, name, path, expires_utc, is_secure, is_httponly, samesite, value, hex(encrypted_value) AS enc FROM cookies";

/// Seconds between 1601-01-01 (Chrome's epoch) and 1970-01-01.
const EPOCH_DELTA: i64 = 11_644_473_600;

#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub browser: String,
    /// Profile folder, e.g. ".../Google/Chrome/Default".
    pub dir: PathBuf,
    pub profile: String,
    /// Keychain item with the encryption key: (service, account).
    pub keychain: (String, String),
    /// Demo sources come with their key (no Keychain).
    pub demo_key: Option<String>,
}

impl Source {
    pub fn label(&self) -> String {
        format!("{} · {}", self.browser, self.profile)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct What {
    pub bookmarks: bool,
    pub history: bool,
    pub passwords: bool,
    pub cookies: bool,
    pub settings: bool,
}

impl Default for What {
    fn default() -> Self {
        What { bookmarks: true, history: true, passwords: true, cookies: true, settings: true }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Login {
    pub origin: String,
    pub username: String,
    pub password: String,
}

#[derive(Default, Debug)]
pub struct Imported {
    pub source: String,
    pub bookmarks: Vec<Bookmark>,
    pub history: Vec<HistoryEntry>,
    pub visits: Vec<Visit>,
    pub logins: Vec<Login>,
    /// (URL the cookie belongs to, cookie).
    pub cookies: Vec<(String, CookieJson)>,
    pub search: Option<SearchEngine>,
    pub show_bookmarks_bar: Option<bool>,
    pub startup_urls: Vec<String>,
    /// Site icons from Chrome's own cache: (host, PNG bytes).
    pub favicons: Vec<(String, Vec<u8>)>,
    pub errors: Vec<String>,
}

/// Chromium browsers this Mac has, with their profiles.
pub fn sources() -> Vec<Source> {
    let Some(base) = dirs::data_dir() else { return Vec::new() };
    let known: [(&str, &str, &str, &str); 7] = [
        ("Chrome", "Google/Chrome", "Chrome Safe Storage", "Chrome"),
        ("Dia", "Dia/User Data", "Dia Safe Storage", "Dia"),
        ("Brave", "BraveSoftware/Brave-Browser", "Brave Safe Storage", "Brave"),
        ("Edge", "Microsoft Edge", "Microsoft Edge Safe Storage", "Microsoft Edge"),
        ("Arc", "Arc/User Data", "Arc Safe Storage", "Arc"),
        ("Vivaldi", "Vivaldi", "Vivaldi Safe Storage", "Vivaldi"),
        ("Chromium", "Chromium", "Chromium Safe Storage", "Chromium"),
    ];
    let mut out = Vec::new();
    for (name, rel, svc, acct) in known {
        out.extend(profiles_in(&base.join(rel), name, (svc.into(), acct.into())));
    }
    out
}

/// Browsers whose data folder exists but macOS won't let Inert read
/// (Privacy & Security protects other apps' data).
pub fn blocked() -> Vec<&'static str> {
    let Some(base) = dirs::data_dir() else { return Vec::new() };
    [("Chrome", "Google/Chrome"), ("Dia", "Dia/User Data"), ("Brave", "BraveSoftware/Brave-Browser"), ("Edge", "Microsoft Edge"), ("Arc", "Arc/User Data"), ("Vivaldi", "Vivaldi"), ("Chromium", "Chromium")]
        .into_iter()
        .filter(|(_, rel)| matches!(std::fs::read_dir(base.join(rel)), Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied))
        .map(|(n, _)| n)
        .collect()
}

/// Profiles under a Chromium "User Data" folder (names from `Local State`).
pub fn profiles_in(root: &Path, browser: &str, keychain: (String, String)) -> Vec<Source> {
    let names: Value = std::fs::read_to_string(root.join("Local State")).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or(Value::Null);
    let cache = names.pointer("/profile/info_cache").and_then(Value::as_object);
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(root) else { return out };
    let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.join("Bookmarks").exists() || p.join("History").exists()).collect();
    dirs.sort();
    for d in dirs {
        let folder = d.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        let profile = cache.and_then(|c| c.get(&folder)).and_then(|p| p.get("name")).and_then(Value::as_str).unwrap_or(&folder).to_owned();
        out.push(Source { browser: browser.into(), dir: d, profile, keychain: keychain.clone(), demo_key: None });
    }
    out
}

fn chrome_time(t: i64) -> Option<u64> {
    (t > 0).then(|| (t / 1_000_000 - EPOCH_DELTA).max(0) as u64)
}

/// Runs a query on a copy of a Chromium SQLite file.
fn query(db: &Path, sql: &str) -> Result<Vec<Value>, String> {
    if !db.exists() {
        return Ok(Vec::new());
    }
    let tmp = std::env::temp_dir().join(format!("inert-import-{}-{}", std::process::id(), db.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default().replace(' ', "_")));
    std::fs::copy(db, &tmp).map_err(|e| format!("copy {}: {e}", db.display()))?;
    for ext in ["-wal", "-shm"] {
        let side = PathBuf::from(format!("{}{ext}", db.display()));
        if side.exists() {
            let _ = std::fs::copy(&side, format!("{}{ext}", tmp.display()));
        }
    }
    let out = Command::new("/usr/bin/sqlite3").arg("-readonly").arg("-json").arg(&tmp).arg(sql).output().map_err(|e| format!("sqlite3: {e}"));
    for ext in ["", "-wal", "-shm"] {
        let _ = std::fs::remove_file(format!("{}{ext}", tmp.display()));
    }
    let out = out?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_owned());
    }
    let text = String::from_utf8_lossy(&out.stdout);
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    serde_json::from_str::<Vec<Value>>(&text).map_err(|e| e.to_string())
}

fn s(v: &Value, k: &str) -> String {
    match v.get(k) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn n(v: &Value, k: &str) -> i64 {
    match v.get(k) {
        Some(Value::Number(n)) => n.as_i64().unwrap_or(0),
        Some(Value::String(s)) => s.parse().unwrap_or(0),
        _ => 0,
    }
}

/// Chrome's macOS key: PBKDF2-HMAC-SHA1 over the Keychain secret.
pub fn derive_key(secret: &str) -> [u8; 16] {
    let mut k = [0u8; 16];
    aws_lc_rs::pbkdf2::derive(aws_lc_rs::pbkdf2::PBKDF2_HMAC_SHA1, std::num::NonZeroU32::new(1003).unwrap(), b"saltysalt", secret.as_bytes(), &mut k);
    k
}

/// Decrypts a "v10" value (AES-128-CBC, IV of 16 spaces, PKCS#7).
pub fn decrypt(key: &[u8; 16], blob: &[u8]) -> Option<Vec<u8>> {
    let body = blob.strip_prefix(b"v10")?;
    let k = PaddedBlockDecryptingKey::cbc_pkcs7(UnboundCipherKey::new(&AES_128, key).ok()?).ok()?;
    let mut buf = body.to_vec();
    let plain = k.decrypt(&mut buf, DecryptionContext::Iv128(FixedLength::from([b' '; 16]))).ok()?;
    Some(plain.to_vec())
}

fn keychain_secret(src: &Source) -> Result<String, String> {
    if let Some(k) = &src.demo_key {
        return Ok(k.clone());
    }
    let out = Command::new("/usr/bin/security").args(["find-generic-password", "-w", "-s", &src.keychain.0, "-a", &src.keychain.1]).output().map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err(format!("macOS didn't hand over \"{}\" (denied or missing)", src.keychain.0));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
}

fn walk(node: &Value, out: &mut Vec<Bookmark>) {
    match node.get("type").and_then(Value::as_str) {
        Some("url") => {
            let url = s(node, "url");
            if url.starts_with("http://") || url.starts_with("https://") {
                out.push(Bookmark { title: s(node, "name"), url });
            }
        }
        _ => {
            if let Some(children) = node.get("children").and_then(Value::as_array) {
                for c in children {
                    walk(c, out);
                }
            }
        }
    }
}

fn engine(prefs: &Value) -> Option<SearchEngine> {
    let d = prefs.pointer("/default_search_provider_data/template_url_data")?;
    let all = format!("{} {} {}", s(d, "short_name"), s(d, "keyword"), s(d, "url")).to_ascii_lowercase();
    if all.contains("duckduckgo") {
        Some(SearchEngine::DuckDuckGo)
    } else if all.contains("brave") {
        Some(SearchEngine::Brave)
    } else if all.contains("google") {
        Some(SearchEngine::Google)
    } else {
        None
    }
}

/// Reads everything asked for. Never panics on odd data; problems go to `errors`.
pub fn run(src: &Source, what: What) -> Imported {
    let mut r = Imported { source: src.label(), ..Default::default() };
    let d = &src.dir;
    if what.bookmarks {
        // Chrome keeps bookmarks in two files: "Bookmarks" (local, or synced
        // the old way) and "AccountBookmarks" (the Google account's, for
        // people signed in to Chrome). Read both; the bar comes first, since
        // it becomes Inert's bookmarks bar.
        let files: Vec<Value> = ["Bookmarks", "AccountBookmarks"]
            .iter()
            .filter_map(|name| {
                let p = d.join(name);
                match std::fs::read_to_string(&p).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
                    Some(v) => Some(v),
                    None if p.exists() => {
                        r.errors.push(format!("{name} file couldn't be read"));
                        None
                    }
                    None => None,
                }
            })
            .collect();
        for root in ["bookmark_bar", "other", "synced"] {
            for b in &files {
                if let Some(n) = b.pointer(&format!("/roots/{root}")) {
                    walk(n, &mut r.bookmarks);
                }
            }
        }
        let mut seen = std::collections::HashSet::new();
        r.bookmarks.retain(|b| seen.insert(b.url.clone()));
    }
    if what.bookmarks || what.history {
        // Site icons, straight from Chrome's cache (no network): per site,
        // the bitmap closest to 32 px.
        let sql = "SELECT m.page_url AS page, hex(b.image_data) AS png, b.width AS w FROM favicon_bitmaps b JOIN icon_mapping m ON m.icon_id = b.icon_id WHERE b.width BETWEEN 16 AND 64 AND length(b.image_data) BETWEEN 1 AND 30000 GROUP BY b.icon_id, b.width LIMIT 4000";
        if let Ok(rows) = query(&d.join("Favicons"), sql) {
            let mut best: std::collections::HashMap<String, (i64, Vec<u8>)> = Default::default();
            for v in rows {
                let Some(host) = crate::favicon::host_of(&s(&v, "page")) else { continue };
                let score = (n(&v, "w") - 32).abs();
                if best.get(&host).is_none_or(|(sc, _)| score < *sc) {
                    if let Ok(png) = hex::decode(s(&v, "png")) {
                        best.insert(host, (score, png));
                    }
                }
            }
            r.favicons = best.into_iter().map(|(h, (_, p))| (h, p)).collect();
        }
    }
    if what.history {
        match query(&d.join("History"), "SELECT url, title, visit_count, last_visit_time FROM urls WHERE hidden = 0 ORDER BY last_visit_time DESC LIMIT 5000") {
            Ok(rows) => {
                r.history = rows.iter().filter_map(|v| Some(HistoryEntry { url: s(v, "url"), title: s(v, "title"), visits: n(v, "visit_count").max(1) as u32, last: chrome_time(n(v, "last_visit_time"))? })).filter(|h| h.url.starts_with("http")).collect();
            }
            Err(e) => r.errors.push(format!("History: {e}")),
        }
        match query(&d.join("History"), "SELECT u.url AS url, u.title AS title, v.visit_time AS t FROM visits v JOIN urls u ON u.id = v.url WHERE u.hidden = 0 ORDER BY v.visit_time DESC LIMIT 20000") {
            Ok(rows) => {
                r.visits = rows.iter().filter_map(|v| Some(Visit { url: s(v, "url"), title: s(v, "title"), t: chrome_time(n(v, "t"))? })).filter(|v| v.url.starts_with("http")).collect();
                r.visits.reverse();
            }
            Err(e) => r.errors.push(format!("Visits: {e}")),
        }
    }
    if what.settings {
        if let Some(p) = std::fs::read_to_string(d.join("Preferences")).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok()) {
            r.search = engine(&p);
            r.show_bookmarks_bar = p.pointer("/bookmark_bar/show_on_all_tabs").and_then(Value::as_bool);
            if p.pointer("/session/restore_on_startup").and_then(Value::as_i64) == Some(4) {
                r.startup_urls = p.pointer("/session/startup_urls").and_then(Value::as_array).map(|a| a.iter().filter_map(|u| u.as_str().map(str::to_owned)).collect()).unwrap_or_default();
            }
        }
    }
    if what.passwords || what.cookies {
        let key = match keychain_secret(src) {
            Ok(sec) => Some(derive_key(&sec)),
            Err(e) => {
                r.errors.push(format!("Passwords and cookies skipped: {e}"));
                None
            }
        };
        if let Some(key) = key {
            if what.passwords {
                match query(&d.join("Login Data"), "SELECT origin_url, username_value, hex(password_value) AS pw FROM logins WHERE blacklisted_by_user = 0") {
                    Ok(rows) => {
                        for v in rows {
                            let blob = hex::decode(s(&v, "pw")).unwrap_or_default();
                            let (Some(origin), Some(pw)) = (crate::passwords::origin_of(&s(&v, "origin_url")), decrypt(&key, &blob)) else { continue };
                            r.logins.push(Login { origin, username: s(&v, "username_value"), password: String::from_utf8_lossy(&pw).into_owned() });
                        }
                    }
                    Err(e) => r.errors.push(format!("Passwords: {e}")),
                }
            }
            if what.cookies {
                // Newer Chrome keeps cookies under Network/.
                let cookies_db = if d.join("Network/Cookies").exists() { d.join("Network/Cookies") } else { d.join("Cookies") };
                let version = query(&cookies_db, "SELECT value FROM meta WHERE key = 'version'").ok().and_then(|v| v.first().map(|x| n(x, "value"))).unwrap_or(0);
                match query(&cookies_db, COOKIES_SQL) {
                    Ok(rows) => {
                        let now = crate::profile::now() as i64;
                        for v in rows {
                            let host = s(&v, "host_key");
                            let mut value = s(&v, "value");
                            let enc = hex::decode(s(&v, "enc")).unwrap_or_default();
                            if !enc.is_empty() {
                                let Some(mut plain) = decrypt(&key, &enc) else { continue };
                                // Since database version 24 the value is prefixed with SHA-256(host).
                                if version >= 24 && plain.len() >= 32 {
                                    plain.drain(..32);
                                }
                                value = String::from_utf8_lossy(&plain).into_owned();
                            }
                            let expires = chrome_time(n(&v, "expires_utc")).map(|t| t as i64);
                            if expires.is_some_and(|t| t < now) {
                                continue;
                            }
                            let same_site = match n(&v, "samesite") {
                                0 => Some("None".to_owned()),
                                1 => Some("Lax".to_owned()),
                                2 => Some("Strict".to_owned()),
                                _ => None,
                            };
                            let path = s(&v, "path");
                            let secure = n(&v, "is_secure") == 1;
                            let url = format!("{}://{}{}", if secure { "https" } else { "http" }, host.trim_start_matches('.'), if path.is_empty() { "/" } else { &path });
                            r.cookies.push((url, CookieJson { name: s(&v, "name"), value, domain: host, path, expires, secure, http_only: n(&v, "is_httponly") == 1, same_site }));
                        }
                    }
                    Err(e) => r.errors.push(format!("Cookies: {e}")),
                }
            }
        }
    }
    r
}

// ---------------------------------------------------------------- fixtures

/// Writes a small, fake Chrome profile (demo data and tests): the same files
/// and formats Chrome uses, encrypted with `secret` as the Safe Storage key.
pub fn write_fixture(root: &Path, secret: &str) -> Result<Source, String> {
    use aws_lc_rs::cipher::{EncryptionContext, PaddedBlockEncryptingKey};
    let dir = root.join("Default");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let key = derive_key(secret);
    let enc = |plain: &[u8]| -> String {
        let k = PaddedBlockEncryptingKey::cbc_pkcs7(UnboundCipherKey::new(&AES_128, &key).unwrap()).unwrap();
        let mut buf = plain.to_vec();
        k.less_safe_encrypt(&mut buf, EncryptionContext::Iv128(FixedLength::from([b' '; 16]))).unwrap();
        let mut out = b"v10".to_vec();
        out.extend(buf);
        hex::encode(out)
    };
    std::fs::write(root.join("Local State"), r#"{"profile":{"info_cache":{"Default":{"name":"Demo profile"}}}}"#).map_err(|e| e.to_string())?;
    std::fs::write(
        dir.join("Bookmarks"),
        r#"{"roots":{"bookmark_bar":{"type":"folder","children":[
            {"type":"url","name":"Hacker News","url":"https://news.ycombinator.com/"},
            {"type":"url","name":"Rust","url":"https://www.rust-lang.org/"},
            {"type":"folder","name":"Homelab","children":[{"type":"url","name":"Printables","url":"https://www.printables.com/"}]}]},
          "other":{"type":"folder","children":[{"type":"url","name":"Servo","url":"https://servo.org/"}]},
          "synced":{"type":"folder","children":[]}}}"#,
    )
    .map_err(|e| e.to_string())?;
    std::fs::write(
        dir.join("Preferences"),
        r#"{"bookmark_bar":{"show_on_all_tabs":true},
            "default_search_provider_data":{"template_url_data":{"short_name":"DuckDuckGo","keyword":"duckduckgo.com","url":"https://duckduckgo.com/?q={searchTerms}"}},
            "session":{"restore_on_startup":4,"startup_urls":["https://news.ycombinator.com/"]}}"#,
    )
    .map_err(|e| e.to_string())?;
    let now = crate::profile::now() as i64;
    let ct = |unix: i64| (unix + EPOCH_DELTA) * 1_000_000;
    let sql = |db: &Path, script: String| -> Result<(), String> {
        let _ = std::fs::remove_file(db);
        let out = Command::new("/usr/bin/sqlite3").arg(db).arg(script).output().map_err(|e| e.to_string())?;
        if out.status.success() { Ok(()) } else { Err(String::from_utf8_lossy(&out.stderr).into_owned()) }
    };
    sql(
        &dir.join("History"),
        format!(
            "CREATE TABLE urls(id INTEGER PRIMARY KEY, url TEXT, title TEXT, visit_count INTEGER, typed_count INTEGER, last_visit_time INTEGER, hidden INTEGER DEFAULT 0);
             CREATE TABLE visits(id INTEGER PRIMARY KEY, url INTEGER, visit_time INTEGER);
             INSERT INTO urls VALUES(1,'https://news.ycombinator.com/','Hacker News',42,10,{a},0);
             INSERT INTO urls VALUES(2,'https://www.rust-lang.org/','Rust Programming Language',7,2,{b},0);
             INSERT INTO urls VALUES(3,'https://www.printables.com/','Printables',3,0,{c},0);
             INSERT INTO visits(url, visit_time) VALUES(1,{a}),(1,{a2}),(2,{b}),(3,{c});",
            a = ct(now - 600),
            a2 = ct(now - 90_000),
            b = ct(now - 3_600),
            c = ct(now - 200_000)
        ),
    )?;
    sql(
        &dir.join("Login Data"),
        format!(
            "CREATE TABLE logins(origin_url TEXT, username_value TEXT, password_value BLOB, blacklisted_by_user INTEGER);
             INSERT INTO logins VALUES('https://news.ycombinator.com/login','demo_user',X'{p1}',0);
             INSERT INTO logins VALUES('https://github.com/session','demo@example.com',X'{p2}',0);
             INSERT INTO logins VALUES('https://never.example/','',X'',1);",
            p1 = enc(b"correct horse battery staple"),
            p2 = enc(b"demo-password-2")
        ),
    )?;
    let mut prefixed = sha2_prefix(".github.com");
    prefixed.extend_from_slice(b"demo-session-token");
    sql(
        &dir.join("Cookies"),
        format!(
            "CREATE TABLE meta(key TEXT, value TEXT); INSERT INTO meta VALUES('version','24');
             CREATE TABLE cookies(host_key TEXT, name TEXT, path TEXT, expires_utc INTEGER, is_secure INTEGER, is_httponly INTEGER, samesite INTEGER, value TEXT, encrypted_value BLOB);
             INSERT INTO cookies VALUES('.github.com','user_session','/',{f},1,1,1,'',X'{c1}');
             INSERT INTO cookies VALUES('news.ycombinator.com','user','/',{f},1,1,0,'',X'{c2}');
             INSERT INTO cookies VALUES('.example.com','old','/',{gone},0,0,-1,'expired','');",
            f = ct(now + 30 * 86_400),
            gone = ct(now - 86_400),
            c1 = enc(&prefixed),
            c2 = enc(&{
                let mut v = sha2_prefix("news.ycombinator.com");
                v.extend_from_slice(b"demo_user&hash");
                v
            })
        ),
    )?;
    Ok(Source { browser: "Chrome".into(), dir, profile: "Demo profile".into(), keychain: ("Chrome Safe Storage".into(), "Chrome".into()), demo_key: Some(secret.into()) })
}

fn sha2_prefix(host: &str) -> Vec<u8> {
    use sha2::Digest;
    sha2::Sha256::digest(host.as_bytes()).to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs the import's cookie query, with `LIMIT 0` (no values are read),
    /// on real Chromium-format files listed in INERT_REAL_COOKIES
    /// (colon-separated), and reports each file's schema version.
    #[test]
    #[ignore]
    fn real_chromium_cookie_schema() {
        let paths = std::env::var("INERT_REAL_COOKIES").expect("set INERT_REAL_COOKIES");
        for p in paths.split(':').filter(|p| !p.is_empty()) {
            let path = Path::new(p);
            let version = query(path, "SELECT value FROM meta WHERE key = 'version'").unwrap().first().map(|x| n(x, "value")).unwrap_or(0);
            let rows = query(path, &format!("{COOKIES_SQL} LIMIT 0"));
            println!("{p}: schema version {version}, import query {}", if rows.is_ok() { "OK" } else { "FAILED" });
            assert!(rows.is_ok(), "{p}: {:?}", rows.err());
            assert!(version >= 18, "{p}: unexpected schema version {version}");
        }
    }

    /// A profile written by a real Chrome (INERT_REAL_PROFILE = its "Default"
    /// folder), started with --use-mock-keychain so its key is known
    /// (INERT_REAL_KEY) and the user's Keychain is never involved.
    #[test]
    #[ignore]
    fn real_chrome_profile() {
        let dir = PathBuf::from(std::env::var("INERT_REAL_PROFILE").expect("set INERT_REAL_PROFILE"));
        let key = std::env::var("INERT_REAL_KEY").expect("set INERT_REAL_KEY");
        let src = Source { browser: "Chrome".into(), dir, profile: "real".into(), keychain: (String::new(), String::new()), demo_key: Some(key) };
        let r = run(&src, What::default());
        println!("errors: {:?}", r.errors);
        println!("bookmarks: {:?}", r.bookmarks.iter().map(|b| (&b.title, &b.url)).collect::<Vec<_>>());
        println!("history: {:?}", r.history.iter().map(|h| (&h.title, &h.url, h.visits)).collect::<Vec<_>>());
        println!("visits: {}", r.visits.len());
        println!("logins: {:?}", r.logins.iter().map(|l| (&l.origin, &l.username, l.password.len())).collect::<Vec<_>>());
        println!("cookies: {:?}", r.cookies.iter().map(|(u, c)| (u, &c.name, &c.value, c.http_only, &c.same_site)).collect::<Vec<_>>());
        println!("search: {:?}", r.search);
        assert!(r.bookmarks.iter().any(|b| b.title == "Inert Test Mark" && b.url == "https://example.com/inert"));
        assert!(r.history.iter().any(|h| h.title == "Alpha page" && h.visits >= 2));
        assert!(r.visits.len() >= 3);
        assert!(r.logins.iter().any(|l| l.origin == "https://login.example.com" && l.username == "ada" && l.password == "hunter2-real"));
        assert!(r.cookies.iter().any(|(_, c)| c.name == "sid" && c.value == "hello123" && c.http_only));
    }

    #[test]
    fn imports_a_chrome_profile() {
        let root = std::env::temp_dir().join(format!("inert-import-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let src = write_fixture(&root, "peanuts").unwrap();
        // Profile discovery reads Local State like Chrome does.
        let found = profiles_in(&root, "Chrome", ("Chrome Safe Storage".into(), "Chrome".into()));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].profile, "Demo profile");

        let r = run(&src, What::default());
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        let titles: Vec<&str> = r.bookmarks.iter().map(|b| b.title.as_str()).collect();
        assert_eq!(titles, ["Hacker News", "Rust", "Printables", "Servo"]);
        assert_eq!(r.history.len(), 3);
        assert_eq!(r.history[0].visits, 42);
        assert_eq!(r.visits.len(), 4);
        assert!(r.visits.windows(2).all(|w| w[0].t <= w[1].t), "visits are oldest first");
        assert_eq!(r.logins.len(), 2);
        assert_eq!(r.logins[0], Login { origin: "https://news.ycombinator.com".into(), username: "demo_user".into(), password: "correct horse battery staple".into() });
        // The expired cookie is dropped; the SHA-256 prefix is removed.
        assert_eq!(r.cookies.len(), 2);
        let gh = &r.cookies.iter().find(|c| c.1.name == "user_session").unwrap().1;
        assert_eq!(gh.value, "demo-session-token");
        assert!(gh.secure && gh.http_only);
        assert_eq!(gh.same_site.as_deref(), Some("Lax"));
        assert_eq!(r.search, Some(SearchEngine::DuckDuckGo));
        assert_eq!(r.show_bookmarks_bar, Some(true));
        assert_eq!(r.startup_urls, ["https://news.ycombinator.com/"]);

        // A wrong key decrypts nothing (and doesn't panic).
        let mut bad = src.clone();
        bad.demo_key = Some("wrong".into());
        let r = run(&bad, What::default());
        assert!(r.logins.is_empty() && r.cookies.is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }
}
