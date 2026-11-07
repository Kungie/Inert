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

/// Only the expiry date; the account number is never read into the UI.
pub fn expiry() -> String {
    run(&["account", "get"])
        .and_then(|t| t.lines().find(|l| l.trim_start().starts_with("Expires at")).map(|l| l.split_once(':').map(|x| x.1.trim().to_owned()).unwrap_or_default()))
        .unwrap_or_default()
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
            let s = Status { installed: true, state: "connected".into(), country: "Sweden".into(), city: "Gothenburg".into(), hostname: "se-got-wg-001".into(), ip: "203.0.113.24".into(), mullvad_exit: true, expiry: "2027-01-01".into() };
            let _ = tx.send(Event::Vpn(s));
            let _ = tx.send(Event::VpnRelays(demo_relays()));
            ctx.request_repaint();
            return;
        }
        let rl = relays();
        let _ = tx.send(Event::VpnRelays(rl));
        let exp = expiry();
        let mut last = Status::default();
        loop {
            let mut s = status();
            s.expiry = exp.clone();
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
    Location(String, Option<String>),
}

/// Runs a Mullvad command off the UI thread.
pub fn exec(cmd: Cmd, demo: bool, tx: Sender<Event>, ctx: eframe::egui::Context) {
    std::thread::spawn(move || {
        if demo {
            return;
        }
        match cmd {
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
        s.expiry = expiry();
        let _ = tx.send(Event::Vpn(s));
        ctx.request_repaint();
    });
}
