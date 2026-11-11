//! Push-to-talk dictation: hold Option+Space, speak, let go, and the words
//! are typed where the keyboard is. Recognition runs in `inert-dictate`, a
//! small helper next to the browser that uses Apple's Speech framework (on
//! this Mac when the language allows it). The browser starts it on key
//! down, tells it to stop on key up, and types the final text.

use crate::app::Event;
use eframe::egui;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::Sender;

pub struct Dictation {
    child: Child,
    stdin: Option<ChildStdin>,
    /// What has been heard so far.
    pub partial: String,
    /// The microphone is live (the helper got its permissions).
    pub live: bool,
    pub started: f64,
    /// Key released: waiting for the final text.
    pub stopping: bool,
}

/// What the helper reported.
#[derive(Debug, Clone)]
pub enum Heard {
    Ready,
    Partial(String),
    Final(String),
    Failed(String),
}

impl Dictation {
    pub fn start(tx: &Sender<Event>, ctx: &egui::Context) -> Result<Dictation, String> {
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let helper = exe.with_file_name("inert-dictate");
        if !helper.exists() {
            return Err("The dictation helper is missing (build it with scripts/build.sh)".into());
        }
        let mut child = Command::new(&helper).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().map_err(|e| e.to_string())?;
        let stdin = child.stdin.take();
        let out = child.stdout.take().ok_or("no output from the dictation helper")?;
        let (tx, ectx) = (tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let mut done = false;
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                let heard = match line.split_at_checked(1) {
                    Some(("R", _)) => Heard::Ready,
                    Some(("P", t)) => Heard::Partial(t.trim().to_owned()),
                    Some(("F", t)) => Heard::Final(t.trim().to_owned()),
                    Some(("E", t)) => Heard::Failed(t.trim().to_owned()),
                    _ => continue,
                };
                done |= matches!(heard, Heard::Final(_) | Heard::Failed(_));
                let _ = tx.send(Event::Dictate(heard));
                ectx.request_repaint();
            }
            if !done {
                let _ = tx.send(Event::Dictate(Heard::Failed("Dictation stopped unexpectedly".into())));
                ectx.request_repaint();
            }
        });
        Ok(Dictation { child, stdin, partial: String::new(), live: false, started: ctx.input(|i| i.time), stopping: false })
    }

    /// The key came up: finish the recording.
    pub fn stop(&mut self) {
        if let Some(mut s) = self.stdin.take() {
            let _ = s.write_all(b"stop\n");
        }
        self.stopping = true;
    }
}

impl Drop for Dictation {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
