#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod export;
mod fx;
mod icon;
mod import;
mod model;
mod scryfall;
mod storage;
mod ui;
mod update;
mod worker;

use std::sync::Arc;

/// "v1.1.0" (SemVer): new features bump the middle number, bug/UI fixes the last,
/// and big overhauls or incompatible data changes the first.
pub const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));

fn main() -> eframe::Result<()> {
    // `--pretend-version 1.1.0` makes update checks act as if this were that version (for testing).
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--pretend-version") {
        if let Some(v) = args.get(i + 1) {
            let _ = update::pretend_version(v);
        }
    }

    let icon_size = 64;
    let icon = egui::IconData {
        rgba: icon::rgba(icon_size),
        width: icon_size,
        height: icon_size,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(format!("Cardboard Capital {VERSION}"))
            .with_inner_size([1320.0, 860.0])
            .with_min_inner_size([980.0, 640.0])
            .with_icon(Arc::new(icon)),
        ..Default::default()
    };
    eframe::run_native(
        "Cardboard Capital",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc)))),
    )
}
