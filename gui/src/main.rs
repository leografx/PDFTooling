//! Impose Settings — a Mac-style editor for sheet-impose `job.toml` and
//! pdf-impose `impose.toml` files.
//!
//!   impose-settings                     open the welcome window
//!   impose-settings jobs/job.toml       open a file
//!   impose-settings PDFIn               open the settings of a hot folder
//!   impose-settings --write-icon a.png  write the 1024 px app icon (used by the .app bundler)

#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod doc;
mod icon;
mod iconpix;
mod schema;
mod theme;

use eframe::egui;
use std::path::PathBuf;

fn main() -> eframe::Result {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(i) = args.iter().position(|a| a == "--write-icon") {
        let out = args.get(i + 1).map(String::as_str).unwrap_or("icon.png");
        let size = args.get(i + 2).and_then(|s| s.parse().ok()).unwrap_or(1024);
        std::fs::write(out, iconpix::png(size)).expect("writing icon");
        println!("wrote {out}");
        return Ok(());
    }
    if args.iter().any(|a| a == "-h" || a == "--help") {
        println!("impose-settings [FILE.toml | HOT-FOLDER]\n\nEdit sheet-impose job.toml and pdf-impose impose.toml settings.");
        return Ok(());
    }
    // macOS passes -psn_… when launched from Finder on old systems; ignore flags.
    let file = args.iter().find(|a| !a.starts_with('-')).map(PathBuf::from);

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Impose Settings")
        .with_app_id("impose-settings")
        .with_inner_size([1040.0, 720.0])
        .with_min_inner_size([780.0, 520.0])
        .with_drag_and_drop(true)
        .with_icon(std::sync::Arc::new(icon::icon_data(256)));
    if cfg!(target_os = "macos") && std::env::var_os("IMPOSE_SETTINGS_CLASSIC_TITLEBAR").is_none() {
        // Unified Mac window: no title bar strip, traffic lights over the sidebar.
        viewport = viewport.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false);
    }
    let options = eframe::NativeOptions { viewport, centered: true, ..Default::default() };
    eframe::run_native("Impose Settings", options, Box::new(move |cc| Ok(Box::new(app::App::new(cc, file)))))
}
