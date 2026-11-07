//! Inert Browser.
//!
//! `inert-browser`                       normal profile
//! `inert-browser --demo`                fresh demo profile (safe for screenshots)
//! `inert-browser --demo --shot <dir>`   automated feature walkthrough

mod adblock;
mod ai;
mod app;
mod art;
mod chrome;
mod icons;
mod inert_tab;
mod menu;
mod omnibox;
mod pages;
mod profile;
mod render;
mod shot;
mod theme;
mod vpn;
mod web;

use eframe::egui;

fn main() -> eframe::Result {
    let _ = tracing_subscriber::fmt().with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "warn".into())).try_init();
    let args: Vec<String> = std::env::args().collect();
    let demo = args.iter().any(|a| a == "--demo");
    let shot = args.iter().position(|a| a == "--shot").and_then(|i| args.get(i + 1)).map(std::path::PathBuf::from);

    let mut vp = egui::ViewportBuilder::default()
        .with_title("Inert")
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([760.0, 480.0])
        .with_fullsize_content_view(true)
        .with_titlebar_shown(false)
        .with_title_shown(false)
        .with_transparent(false);
    if shot.is_some() {
        vp = vp.with_position([40.0, 40.0]).with_always_on_top().with_active(true);
    }
    let opts = eframe::NativeOptions { viewport: vp, ..Default::default() };
    eframe::run_native("Inert", opts, Box::new(move |cc| Ok(Box::new(app::Browser::new(cc, demo, shot)))))
}
