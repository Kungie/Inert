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
mod drive;
mod icons;
mod inert_tab;
mod menu;
mod omnibox;
mod pages;
mod profile;
mod render;
mod servo_web;
mod shot;
mod theme;
mod vpn;
mod web;

use eframe::egui;

fn main() -> eframe::Result {
    let _ = tracing_subscriber::fmt().with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "warn,webrender=error,servo_base=error,script=error,profile_traits=error,storage=error".into())).try_init();
    // Servo uses rustls internally and needs a process-wide default provider.
    let _ = inert_transport::rustls::crypto::aws_lc_rs::default_provider().install_default();
    let args: Vec<String> = std::env::args().collect();
    let demo = args.iter().any(|a| a == "--demo");
    let shot = args.iter().position(|a| a == "--shot").and_then(|i| args.get(i + 1)).map(std::path::PathBuf::from);
    let drive = args.iter().position(|a| a == "--drive").and_then(|i| args.get(i + 1)).map(std::path::PathBuf::from);

    let mut vp = egui::ViewportBuilder::default()
        .with_title("Inert")
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([760.0, 480.0])
        .with_fullsize_content_view(true)
        .with_titlebar_shown(false)
        .with_title_shown(false)
        .with_transparent(false);
    if shot.is_some() || drive.is_some() {
        // INERT_SHOT_RECT="x,y,w,h" (points) — must match scripts/walkthrough.sh.
        let r: Vec<f32> = std::env::var("INERT_SHOT_RECT").unwrap_or_else(|_| "40,40,1440,900".into()).split(',').filter_map(|v| v.trim().parse().ok()).collect();
        if r.len() == 4 {
            vp = vp.with_position([r[0], r[1]]).with_inner_size([r[2], r[3]]);
        }
        vp = vp.with_always_on_top().with_active(true);
    }
    let opts = eframe::NativeOptions { viewport: vp, ..Default::default() };
    eframe::run_native("Inert", opts, Box::new(move |cc| Ok(Box::new(app::Browser::new(cc, demo, shot, drive)))))
}
