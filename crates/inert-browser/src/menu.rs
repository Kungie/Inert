//! Native macOS menu bar. Key equivalents live here so shortcuts work even
//! while a web view has keyboard focus, and the Edit menu gives web views
//! copy / paste / select all.

use crate::app::Event;
use muda::accelerator::{Accelerator, Code, Modifiers};
use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use std::sync::mpsc::Sender;

pub struct AppMenu {
    _menu: Menu,
}

fn item(id: &str, text: &str, mods: Option<Modifiers>, code: Option<Code>) -> MenuItem {
    let acc = code.map(|c| Accelerator::new(mods.unwrap_or(Modifiers::empty()), c));
    MenuItem::with_id(id, text, true, acc)
}

impl AppMenu {
    pub fn install(tx: Sender<Event>, ctx: eframe::egui::Context) -> Option<AppMenu> {
        #[cfg(target_os = "macos")]
        {
            let cmd = Some(Modifiers::META);
            let cmd_shift = Some(Modifiers::META | Modifiers::SHIFT);
            let cmd_alt = Some(Modifiers::META | Modifiers::ALT);
            let menu = Menu::new();
            let app = Submenu::with_items(
                "Inert",
                true,
                &[
                    &PredefinedMenuItem::about(Some("About Inert"), None),
                    &PredefinedMenuItem::separator(),
                    &item("settings", "Settings…", cmd, Some(Code::Comma)),
                    &PredefinedMenuItem::separator(),
                    &PredefinedMenuItem::hide(None),
                    &PredefinedMenuItem::hide_others(None),
                    &PredefinedMenuItem::separator(),
                    &PredefinedMenuItem::quit(None),
                ],
            )
            .ok()?;
            let file = Submenu::with_items(
                "File",
                true,
                &[
                    &item("new-tab", "New Tab", cmd, Some(Code::KeyT)),
                    &item("reopen", "Reopen Closed Tab", cmd_shift, Some(Code::KeyT)),
                    &item("focus-omnibox", "Open Location…", cmd, Some(Code::KeyL)),
                    &PredefinedMenuItem::separator(),
                    &item("close-tab", "Close Tab", cmd, Some(Code::KeyW)),
                ],
            )
            .ok()?;
            let edit = Submenu::with_items(
                "Edit",
                true,
                &[
                    // Our own items, not AppKit's: AppKit's send cut:/copy:/paste:
                    // to the first responder, which for egui's text fields is a
                    // view that ignores them, so Cmd+V never reached a field.
                    // `Browser::edit_command` routes them (see chrome.rs).
                    &item("edit-undo", "Undo", cmd, Some(Code::KeyZ)),
                    &item("edit-redo", "Redo", cmd_shift, Some(Code::KeyZ)),
                    &PredefinedMenuItem::separator(),
                    &item("edit-cut", "Cut", cmd, Some(Code::KeyX)),
                    &item("edit-copy", "Copy", cmd, Some(Code::KeyC)),
                    &item("edit-paste", "Paste", cmd, Some(Code::KeyV)),
                    &item("edit-select-all", "Select All", cmd, Some(Code::KeyA)),
                    &PredefinedMenuItem::separator(),
                    &item("find", "Find…", cmd, Some(Code::KeyF)),
                ],
            )
            .ok()?;
            let view = Submenu::with_items(
                "View",
                true,
                &[
                    &item("reload", "Reload Page", cmd, Some(Code::KeyR)),
                    &PredefinedMenuItem::separator(),
                    &item("zoom-in", "Zoom In", cmd, Some(Code::Equal)),
                    &item("zoom-out", "Zoom Out", cmd, Some(Code::Minus)),
                    &item("zoom-reset", "Actual Size", cmd, Some(Code::Digit0)),
                    &PredefinedMenuItem::separator(),
                    &item("bookmarks-bar", "Show Bookmarks Bar", cmd_shift, Some(Code::KeyB)),
                    &item("assistant", "Assistant", cmd, Some(Code::KeyJ)),
                    &item("vpn", "VPN…", cmd_shift, Some(Code::KeyV)),
                    &item("devtools", "Developer Tools", cmd_alt, Some(Code::KeyI)),
                    &PredefinedMenuItem::separator(),
                    &PredefinedMenuItem::fullscreen(None),
                ],
            )
            .ok()?;
            let history = Submenu::with_items(
                "History",
                true,
                &[&item("back", "Back", cmd, Some(Code::BracketLeft)), &item("forward", "Forward", cmd, Some(Code::BracketRight)), &PredefinedMenuItem::separator(), &item("history", "Show All History", cmd, Some(Code::KeyY))],
            )
            .ok()?;
            let marks = Submenu::with_items("Bookmarks", true, &[&item("bookmark", "Bookmark This Tab", cmd, Some(Code::KeyD)), &item("bookmarks", "Show All Bookmarks", cmd_alt, Some(Code::KeyB))]).ok()?;
            let digits = [Code::Digit1, Code::Digit2, Code::Digit3, Code::Digit4, Code::Digit5, Code::Digit6, Code::Digit7, Code::Digit8, Code::Digit9];
            let tab_items: Vec<MenuItem> = digits.iter().enumerate().map(|(i, c)| item(&format!("tab-{}", i + 1), &format!("Tab {}", i + 1), cmd, Some(*c))).collect();
            let window = Submenu::with_items(
                "Window",
                true,
                &[&PredefinedMenuItem::minimize(None), &PredefinedMenuItem::separator(), &item("next-tab", "Next Tab", Some(Modifiers::CONTROL), Some(Code::Tab)), &item("prev-tab", "Previous Tab", Some(Modifiers::CONTROL | Modifiers::SHIFT), Some(Code::Tab))],
            )
            .ok()?;
            for t in &tab_items {
                let _ = window.append(t);
            }
            for m in [&app, &file, &edit, &view, &history, &marks, &window] {
                let _ = menu.append(m);
            }
            menu.init_for_nsapp();
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                let _ = tx.send(Event::Menu(e.id().0.clone()));
                ctx.request_repaint();
            }));
            Some(AppMenu { _menu: menu })
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (tx, ctx);
            None
        }
    }
}
