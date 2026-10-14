#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod account;
mod app;
mod cloud;
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

use std::path::Path;
use std::sync::Arc;

/// "v1.1.0" (SemVer): new features bump the middle number, bug/UI fixes the last,
/// and big overhauls or incompatible data changes the first.
pub const VERSION: &str = concat!("v", env!("CARGO_PKG_VERSION"));

/// Passed to the new exe after a self-update, so it waits for the old one to close.
pub const AFTER_UPDATE: &str = "--after-update";

fn main() -> eframe::Result<()> {
    // `--pretend-version 1.1.0` makes update checks act as if this were that version (for testing).
    let args: Vec<String> = std::env::args().collect();
    if let Some(i) = args.iter().position(|a| a == "--pretend-version") {
        if let Some(v) = args.get(i + 1) {
            let _ = update::pretend_version(v);
        }
    }

    // Find (and if needed move) the data before the window opens, and only in one copy of the app.
    let paths = storage::Paths::resolve();
    let Ok(instance_lock) = lock_instance(&paths.local, args.iter().any(|a| a == AFTER_UPDATE)) else {
        rfd::MessageDialog::new()
            .set_title("Cardboard Capital")
            .set_description("Cardboard Capital is already running.")
            .set_level(rfd::MessageLevel::Info)
            .show();
        return Ok(());
    };
    let boot = storage::boot(paths);

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
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, boot, instance_lock)))),
    )
}

/// Holds `instance.lock` while the app runs: every copy now shares one data folder, and two
/// copies saving would overwrite each other's changes. Err if another copy holds it; Ok(None)
/// if the lock can't be taken at all (the app then runs anyway).
fn lock_instance(local: &Path, after_update: bool) -> Result<Option<std::fs::File>, ()> {
    let _ = std::fs::create_dir_all(local);
    let Ok(file) = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(local.join("instance.lock"))
    else {
        return Ok(None);
    };
    // Right after an update the previous version takes a moment to close.
    let tries = if after_update { 40 } else { 1 };
    for i in 0..tries {
        match file.try_lock() {
            Ok(()) => return Ok(Some(file)),
            Err(std::fs::TryLockError::WouldBlock) if i + 1 < tries => {
                std::thread::sleep(std::time::Duration::from_millis(250))
            }
            Err(std::fs::TryLockError::WouldBlock) => return Err(()),
            Err(std::fs::TryLockError::Error(_)) => return Ok(None),
        }
    }
    Err(())
}
