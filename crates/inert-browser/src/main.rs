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
mod cookies;
mod dictate;
mod drive;
mod favicon;
mod md;
mod icons;
mod import;
mod import_ui;
mod inert_tab;
mod menu;
mod omnibox;
mod page_js;
mod pages;
mod passwords;
mod profile;
mod render;
mod servo_web;
mod mem;
mod shot;
mod theme;
mod vpn;
mod web;
mod when;

use eframe::egui;

fn main() -> eframe::Result {
    mem::tune_allocator();
    let _ = tracing_subscriber::fmt().with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "warn,webrender=error,servo_base=error,script=error,profile_traits=error,storage=error".into())).try_init();
    // Servo uses rustls internally and needs a process-wide default provider.
    let _ = inert_transport::rustls::crypto::aws_lc_rs::default_provider().install_default();
    let args: Vec<String> = std::env::args().collect();
    let demo = args.iter().any(|a| a == "--demo");
    let shot = args.iter().position(|a| a == "--shot").and_then(|i| args.get(i + 1)).map(std::path::PathBuf::from);
    let drive = args.iter().position(|a| a == "--drive").and_then(|i| args.get(i + 1)).map(std::path::PathBuf::from);
    // `--write-icon <png> [size]`: the app icon, for scripts/bundle.sh.
    if let Some(out) = args.iter().position(|a| a == "--write-icon").and_then(|i| args.get(i + 1)) {
        let n = args.iter().position(|a| a == "--write-icon").and_then(|i| args.get(i + 2)).and_then(|s| s.parse().ok()).unwrap_or(1024);
        drive::save_png(std::path::Path::new(out), &art::icon_image(n));
        return Ok(());
    }
    // `--import-check <file>`: what the Chrome import can see on this Mac,
    // as counts only (nothing from the browser's data is written out).
    if let Some(out) = args.iter().position(|a| a == "--import-check").and_then(|i| args.get(i + 1)) {
        let mut lines = vec![format!("blocked by macOS: {:?}", import::blocked())];
        for src in import::sources() {
            let folder = src.dir.file_name().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
            let exists = |n: &str| if src.dir.join(n).exists() { "yes" } else { "no" };
            let r = import::run(&src, import::What { bookmarks: true, history: true, passwords: false, cookies: false, settings: false });
            lines.push(format!(
                "{} · folder {folder}: Bookmarks file {} · AccountBookmarks file {} · bookmarks read {} · history rows {} · icons {} · errors {:?}",
                src.browser,
                exists("Bookmarks"),
                exists("AccountBookmarks"),
                r.bookmarks.len(),
                r.history.len(),
                r.favicons.len(),
                r.errors
            ));
        }
        if lines.len() == 1 {
            lines.push("no browser profiles readable".into());
        }
        let _ = std::fs::write(out, lines.join("\n") + "\n");
        return Ok(());
    }
    let icon = art::icon_image(256);
    let icon = egui::IconData { rgba: icon.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect(), width: 256, height: 256 };

    let mut vp = egui::ViewportBuilder::default()
        .with_title("Inert")
        .with_inner_size([1440.0, 900.0])
        .with_min_inner_size([760.0, 480.0])
        .with_fullsize_content_view(true)
        .with_titlebar_shown(false)
        .with_title_shown(false)
        .with_transparent(false)
        .with_icon(icon);
    if shot.is_some() || drive.is_some() {
        // INERT_SHOT_RECT="x,y,w,h" (points) — must match scripts/walkthrough.sh.
        let r: Vec<f32> = std::env::var("INERT_SHOT_RECT").unwrap_or_else(|_| "40,40,1440,900".into()).split(',').filter_map(|v| v.trim().parse().ok()).collect();
        if r.len() == 4 {
            vp = vp.with_position([r[0], r[1]]).with_inner_size([r[2], r[3]]);
        }
        vp = vp.with_always_on_top().with_active(true);
    }
    // Two swapchain images instead of three: at 2880×1800 each one is ~21 MB.
    let mut wgpu_options = eframe::egui_wgpu::WgpuConfiguration::default();
    wgpu_options.surface = eframe::egui_wgpu::SurfaceConfig::LOW_LATENCY;
    let opts = eframe::NativeOptions { viewport: vp, wgpu_options, ..Default::default() };
    eframe::run_native("Inert", opts, Box::new(move |cc| Ok(Box::new(app::Browser::new(cc, demo, shot, drive)))))
}
