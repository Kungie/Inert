//! Native VPN, powered by Mullvad. The browser drives the installed Mullvad
//! daemon through its official CLI (`mullvad`), so the tunnel is Mullvad's own
//! WireGuard implementation; Inert only dresses it in its own UI.
//! Note: Mullvad tunnels the whole Mac, not only the browser.

use crate::app::Event;
use serde::Deserialize;
use std::process::Command;
use std::sync::mpsc::Sender;
use std::time::Duration;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub installed: bool,
    pub state: String,
    pub country: String,
    pub city: String,
    pub hostname: String,
    pub ip: String,
    pub mullvad_exit: bool,
    pub expiry: String,
    /// Whether this Mac is signed in to a Mullvad account (`None`: not known yet).
    pub logged_in: Option<bool>,
}

impl Status {
    pub fn connected(&self) -> bool {
        self.state == "connected"
    }
    pub fn busy(&self) -> bool {
        self.state == "connecting" || self.state == "disconnecting"
    }
    pub fn short(&self) -> String {
        if !self.installed {
            return "VPN".into();
        }
        if self.connected() {
            let cc = country_code(&self.country);
            format!("VPN · {cc}")
        } else if self.busy() {
            "VPN · …".into()
        } else {
            "VPN · OFF".into()
        }
    }
}

fn country_code(name: &str) -> String {
    name.chars().filter(|c| c.is_alphabetic()).take(2).collect::<String>().to_uppercase()
}

#[derive(Clone, Debug)]
pub struct Country {
    pub name: String,
    pub code: String,
    pub cities: Vec<(String, String)>,
}

#[derive(Deserialize)]
struct StatusJson {
    state: String,
    details: Option<Details>,
}

#[derive(Deserialize)]
struct Details {
    location: Option<Loc>,
}

#[derive(Deserialize)]
struct Loc {
    ipv4: Option<String>,
    city: Option<String>,
    country: Option<String>,
    hostname: Option<String>,
    mullvad_exit_ip: Option<bool>,
}

fn bin() -> Option<&'static str> {
    ["/usr/local/bin/mullvad", "/opt/homebrew/bin/mullvad", "/Applications/Mullvad VPN.app/Contents/Resources/mullvad"].into_iter().find(|p| std::path::Path::new(p).exists())
}

fn run(args: &[&str]) -> Option<String> {
    let out = Command::new(bin()?).args(args).output().ok()?;
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn status() -> Status {
    let Some(_) = bin() else { return Status::default() };
    let mut s = Status { installed: true, state: "unknown".into(), ..Default::default() };
    if let Some(j) = run(&["status", "-j"]).and_then(|t| serde_json::from_str::<StatusJson>(&t).ok()) {
        s.state = j.state;
        if let Some(l) = j.details.and_then(|d| d.location) {
            s.country = l.country.unwrap_or_default();
            s.city = l.city.unwrap_or_default();
            s.hostname = l.hostname.unwrap_or_default();
            s.ip = l.ipv4.unwrap_or_default();
            s.mullvad_exit = l.mullvad_exit_ip.unwrap_or(false);
        }
    }
    s
}

/// Signed in or not, and the expiry date. The account number is in the
/// CLI's output but is dropped right here: it never reaches the UI.
pub fn account() -> (Option<bool>, String) {
    let Some(t) = run(&["account", "get"]) else { return (None, String::new()) };
    account_from(&t)
}

fn account_from(t: &str) -> (Option<bool>, String) {
    let expiry = t.lines().find(|l| l.trim_start().starts_with("Expires at")).and_then(|l| l.split_once(':')).map(|x| x.1.trim().to_owned()).unwrap_or_default();
    if t.contains("Mullvad account:") {
        (Some(true), expiry)
    } else if t.to_ascii_lowercase().contains("not logged in") {
        (Some(false), String::new())
    } else {
        (None, expiry)
    }
}

/// A Mullvad account number: 16 digits; spaces and dashes are ignored.
pub fn clean_account(input: &str) -> Option<String> {
    let d: String = input.chars().filter(|c| !c.is_whitespace() && *c != '-').collect();
    (d.len() == 16 && d.chars().all(|c| c.is_ascii_digit())).then_some(d)
}

pub fn relays() -> Vec<Country> {
    let Some(text) = run(&["relay", "list"]) else { return Vec::new() };
    let mut out: Vec<Country> = Vec::new();
    let paren = |l: &str| -> Option<(String, String)> {
        let (name, rest) = l.split_once(" (")?;
        let code = rest.split(')').next()?;
        Some((name.trim().to_owned(), code.trim().to_owned()))
    };
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let tabs = line.chars().take_while(|c| *c == '\t').count();
        let l = line.trim();
        match tabs {
            0 => {
                if let Some((name, code)) = paren(l) {
                    out.push(Country { name, code, cities: Vec::new() });
                }
            }
            1 => {
                if let (Some(c), Some((name, code))) = (out.last_mut(), paren(l)) {
                    c.cities.push((name, code));
                }
            }
            _ => {}
        }
    }
    out
}

/// Polls status every 3 s on a background thread.
pub fn start_polling(tx: Sender<Event>, ctx: eframe::egui::Context, demo: bool) {
    std::thread::spawn(move || {
        if demo {
            // INERT_DEMO_VPN=not-installed | logged-out shows the setup steps.
            let s = match std::env::var("INERT_DEMO_VPN").as_deref() {
                Ok("not-installed") => Status::default(),
                Ok("logged-out") => demo_status(false),
                _ => demo_status(true),
            };
            let _ = tx.send(Event::Vpn(s));
            let _ = tx.send(Event::VpnRelays(demo_relays()));
            ctx.request_repaint();
            return;
        }
        let rl = relays();
        let _ = tx.send(Event::VpnRelays(rl));
        let mut acct = account();
        let mut last = Status::default();
        let mut n = 0u32;
        loop {
            // The account is checked every 30 s (and after log in / log out).
            if n % 10 == 0 && n > 0 {
                acct = account();
            }
            n += 1;
            let mut s = status();
            (s.logged_in, s.expiry) = acct.clone();
            if s != last {
                last = s.clone();
                if tx.send(Event::Vpn(s)).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
            std::thread::sleep(Duration::from_secs(3));
        }
    });
}

fn demo_status(logged_in: bool) -> Status {
    if logged_in {
        Status { installed: true, state: "connected".into(), country: "Sweden".into(), city: "Gothenburg".into(), hostname: "se-got-wg-001".into(), ip: "203.0.113.24".into(), mullvad_exit: true, expiry: "2027-01-01".into(), logged_in: Some(true) }
    } else {
        Status { installed: true, state: "disconnected".into(), ip: "198.51.100.7".into(), logged_in: Some(false), ..Default::default() }
    }
}

fn demo_relays() -> Vec<Country> {
    let c = |n: &str, code: &str, cities: &[(&str, &str)]| Country { name: n.into(), code: code.into(), cities: cities.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect() };
    vec![
        c("Germany", "de", &[("Berlin", "ber"), ("Frankfurt", "fra")]),
        c("Netherlands", "nl", &[("Amsterdam", "ams")]),
        c("Sweden", "se", &[("Gothenburg", "got"), ("Stockholm", "sto")]),
        c("Switzerland", "ch", &[("Zurich", "zrh")]),
        c("Turkey", "tr", &[("Istanbul", "ist")]),
    ]
}

pub enum Cmd {
    Connect,
    Disconnect,
    /// Signs this Mac in with an account number (already checked by `clean_account`).
    Login(String),
    /// Signs out: disconnects and removes this device from the account.
    Logout,
    Location(String, Option<String>),
}

/// Runs a Mullvad command off the UI thread.
pub fn exec(cmd: Cmd, demo: bool, tx: Sender<Event>, ctx: eframe::egui::Context) {
    std::thread::spawn(move || {
        if demo {
            // The demo profile pretends: any well-formed number signs in.
            let (s, login) = match cmd {
                Cmd::Login(_) => (demo_status(true), true),
                Cmd::Logout => (demo_status(false), false),
                _ => return,
            };
            std::thread::sleep(Duration::from_millis(500));
            if login {
                let _ = tx.send(Event::VpnLogin(Ok(())));
            }
            let _ = tx.send(Event::Vpn(s));
            ctx.request_repaint();
            return;
        }
        match cmd {
            Cmd::Login(number) => {
                // The CLI takes the number as an argument (it has no stdin
                // mode); it is visible to this user's processes for the
                // moment the command runs, never logged or stored by Inert.
                let r = bin().ok_or_else(|| "Mullvad isn't installed".to_owned()).and_then(|b| {
                    let out = Command::new(b).args(["account", "login", number.as_str()]).output().map_err(|e| e.to_string())?;
                    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
                    if out.status.success() && !text.to_ascii_lowercase().contains("error") {
                        Ok(())
                    } else {
                        Err(login_error(&text))
                    }
                });
                let _ = tx.send(Event::VpnLogin(r));
            }
            Cmd::Logout => {
                run(&["disconnect"]);
                run(&["account", "logout"]);
            }
            Cmd::Connect => {
                run(&["connect"]);
            }
            Cmd::Disconnect => {
                run(&["disconnect"]);
            }
            Cmd::Location(cc, city) => {
                let mut a = vec!["relay", "set", "location", cc.as_str()];
                if let Some(c) = city.as_deref() {
                    a.push(c);
                }
                run(&a);
                run(&["connect"]);
            }
        }
        std::thread::sleep(Duration::from_millis(400));
        let mut s = status();
        (s.logged_in, s.expiry) = account();
        let _ = tx.send(Event::Vpn(s));
        ctx.request_repaint();
    });
}

/// A short, number-free message for a failed login.
fn login_error(cli: &str) -> String {
    let l = cli.to_ascii_lowercase();
    if l.contains("invalid") || l.contains("not found") || l.contains("does not exist") {
        "Mullvad doesn't know that account number.".into()
    } else if l.contains("too many devices") || l.contains("max") {
        "This account already has the maximum number of devices. Remove one at mullvad.net.".into()
    } else if l.contains("network") || l.contains("connect") || l.contains("timed out") {
        "Couldn't reach Mullvad. Check your connection and try again.".into()
    } else {
        "Mullvad couldn't sign in with that number.".into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_numbers() {
        assert_eq!(clean_account("1234 5678 9012 3456").as_deref(), Some("1234567890123456"));
        assert_eq!(clean_account("1234-5678-9012-3456").as_deref(), Some("1234567890123456"));
        assert!(clean_account("1234").is_none());
        assert!(clean_account("abcd5678901234567").is_none());
    }

    #[test]
    fn account_output_never_keeps_the_number() {
        let (on, exp) = account_from("Mullvad account:    0000111122223333\nExpires at:         2026-12-01 10:00:00 +01:00\nDevice name:        Demo Fox\n");
        assert_eq!(on, Some(true));
        assert_eq!(exp, "2026-12-01 10:00:00 +01:00");
        assert_eq!(account_from("Not logged in on any account\n").0, Some(false));
    }
}
