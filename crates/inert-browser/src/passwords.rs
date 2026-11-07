//! Saved passwords (from the Chrome import). Secrets live in the macOS
//! Keychain under "Inert Passwords"; the profile only keeps which origin and
//! username exist. Demo profiles keep them in memory and never touch the
//! Keychain.

use std::collections::HashMap;
use std::sync::Mutex;

const SERVICE: &str = "Inert Passwords";

static DEMO: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);

fn account(origin: &str, user: &str) -> String {
    format!("{origin}\t{user}")
}

pub fn save(origin: &str, user: &str, password: &str, demo: bool) -> Result<(), String> {
    let a = account(origin, user);
    if demo {
        DEMO.lock().map_err(|e| e.to_string())?.get_or_insert_with(HashMap::new).insert(a, password.to_owned());
        return Ok(());
    }
    keyring::Entry::new(SERVICE, &a).and_then(|e| e.set_password(password)).map_err(|e| e.to_string())
}

pub fn get(origin: &str, user: &str, demo: bool) -> Option<String> {
    let a = account(origin, user);
    if demo {
        return DEMO.lock().ok()?.as_ref()?.get(&a).cloned();
    }
    keyring::Entry::new(SERVICE, &a).ok()?.get_password().ok()
}

pub fn forget(origin: &str, user: &str, demo: bool) {
    let a = account(origin, user);
    if demo {
        if let Ok(mut m) = DEMO.lock() {
            if let Some(m) = m.as_mut() {
                m.remove(&a);
            }
        }
        return;
    }
    if let Ok(e) = keyring::Entry::new(SERVICE, &a) {
        let _ = e.delete_credential();
    }
}

/// "https://accounts.google.com/signin" → "https://accounts.google.com".
pub fn origin_of(url: &str) -> Option<String> {
    let u = url::Url::parse(url).ok()?;
    match u.origin() {
        url::Origin::Tuple(..) => Some(u.origin().ascii_serialization()),
        url::Origin::Opaque(_) => None,
    }
}

/// Fills a username and password into the page's visible login form.
/// Only ever runs when the user clicks "Fill" for this exact origin.
pub fn fill_js(user: &str, password: &str) -> String {
    let u = serde_json::to_string(user).unwrap_or_default();
    let p = serde_json::to_string(password).unwrap_or_default();
    format!(
        r#"(function(u, p) {{
var vis = function(e) {{ return e.offsetParent !== null; }};
var pw = [].slice.call(document.querySelectorAll('input[type=password]')).filter(vis)[0];
var set = function(e, v) {{
  var d = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value');
  d.set.call(e, v);
  e.dispatchEvent(new Event('input', {{ bubbles: true }}));
  e.dispatchEvent(new Event('change', {{ bubbles: true }}));
}};
var scope = (pw && pw.form) || document;
var users = [].slice.call(scope.querySelectorAll('input[type=email],input[type=text],input[autocomplete=username],input:not([type])')).filter(vis)
  .filter(function(e) {{ return !pw || (e.compareDocumentPosition(pw) & 4); }});
if (users.length) set(users[users.length - 1], u);
if (pw) set(pw, p);
return pw ? 'filled' : (users.length ? 'user' : 'nofield');
}})({u}, {p});"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origins() {
        assert_eq!(origin_of("https://accounts.google.com/signin?x=1").as_deref(), Some("https://accounts.google.com"));
        assert_eq!(origin_of("http://localhost:3000/login").as_deref(), Some("http://localhost:3000"));
        assert!(origin_of("data:text/html,hi").is_none());
    }

    #[test]
    fn fill_script_escapes() {
        let js = fill_js("a\"b", "p'</script>");
        assert!(js.contains(r#""a\"b""#));
        assert!(js.contains("p'</script>"));
    }

    #[test]
    fn demo_store_stays_in_memory() {
        save("https://demo.example", "ada", "pw1", true).unwrap();
        assert_eq!(get("https://demo.example", "ada", true).as_deref(), Some("pw1"));
        forget("https://demo.example", "ada", true);
        assert!(get("https://demo.example", "ada", true).is_none());
    }
}
