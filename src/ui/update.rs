//! Update notifications: header chip, the "update available" window and the Settings section.
//! Deliberately plain for now; the UI will be redesigned later.

use egui::{RichText, Ui};

use crate::app::App;
use crate::model::UpdateChannel;
use crate::ui::widgets::{self, bold, ToastKind, ACCENT};
use crate::update::{self, UpdateInfo};
use crate::worker::Job;

#[derive(Default)]
pub struct UpdateState {
    pub available: Option<UpdateInfo>,
    pub checking: bool,
    pub installing: bool,
    pub show_window: bool,
    /// `--install-update-now` (testing): install whatever the startup check finds.
    pub auto_install: bool,
    /// Result of the last check, shown in Settings.
    pub status: String,
}

pub fn check(app: &mut App, manual: bool) {
    let channel = app.settings.channel();
    if channel == UpdateChannel::Off && !manual {
        return;
    }
    app.update.checking = true;
    let _ = app.worker.jobs.send(Job::CheckUpdate { channel, manual });
}

pub fn on_checked(app: &mut App, manual: bool, result: Result<Option<UpdateInfo>, String>) {
    app.update.checking = false;
    let now = crate::model::now_string();
    match result {
        Ok(Some(info)) if app.update.auto_install && !app.update.installing => {
            app.update.installing = true;
            app.update.available = Some(info.clone());
            app.update.show_window = true;
            let _ = app.worker.jobs.send(Job::InstallUpdate {
                info,
                data_dir: app.data_dir.clone(),
            });
        }
        Ok(Some(info)) => {
            let skipped = !manual && app.settings.skipped_version == info.tag;
            app.update.status = format!("{} is available ({now}).", info.tag);
            if !skipped {
                app.update.show_window = manual;
                app.update.available = Some(info);
            }
        }
        Ok(None) => {
            app.update.available = None;
            app.update.status = if app.settings.channel() == UpdateChannel::Off {
                "Update checks are off.".into()
            } else {
                format!("You're up to date ({now}).")
            };
            if manual {
                let msg = app.update.status.clone();
                app.toast(ToastKind::Success, msg);
            }
        }
        Err(e) => {
            app.update.status = format!("Couldn't check: {e}");
            if manual {
                app.toast(ToastKind::Error, format!("Update check failed: {e}"));
            }
        }
    }
}

pub fn on_installed(app: &mut App, ctx: &egui::Context, result: Result<std::path::PathBuf, String>) {
    match result {
        Ok(exe) => {
            app.save_now();
            match std::process::Command::new(&exe).spawn() {
                Ok(_) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                Err(e) => {
                    app.update.installing = false;
                    app.toast(
                        ToastKind::Info,
                        format!("Update installed. Restart the app to use it ({e})."),
                    );
                }
            }
        }
        Err(e) => {
            app.update.installing = false;
            app.toast(ToastKind::Error, e);
        }
    }
}

/// Small button in the header when an update is waiting.
pub fn header_chip(app: &mut App, ui: &mut Ui) {
    if let Some(info) = &app.update.available {
        let label = format!("⬆ Update {}", info.tag);
        if ui
            .add(egui::Button::new(bold(label).color(egui::Color32::from_rgb(24, 20, 12))).fill(ACCENT))
            .clicked()
        {
            app.update.show_window = true;
        }
    }
}

pub fn window(app: &mut App, ctx: &egui::Context) {
    if !app.update.show_window {
        return;
    }
    let Some(info) = app.update.available.clone() else {
        app.update.show_window = false;
        return;
    };
    let mut open = true;
    let mut install = false;
    let mut skip = false;
    let mut later = false;
    egui::Window::new("Update available")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(460.0)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(bold(format!("Cardboard Capital {}", info.tag)).size(18.0));
            ui.label(format!("You have {}.", crate::VERSION));
            if info.prerelease {
                widgets::warning(ui, "This is a beta: new features that are still being tested.");
            }
            if !info.notes.trim().is_empty() {
                ui.add_space(4.0);
                egui::ScrollArea::vertical().max_height(220.0).show(ui, |ui| {
                    ui.label(RichText::new(info.notes.trim()).small());
                });
            }
            ui.add_space(6.0);
            ui.label(
                RichText::new("Your collection, settings and spreadsheet aren't touched; only the app is replaced.")
                    .small()
                    .weak(),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if app.update.installing {
                    ui.spinner();
                    ui.label("Downloading…");
                } else if widgets::primary_button(ui, "Install & restart").clicked() {
                    install = true;
                }
                if ui.button("Release page").clicked() {
                    let _ = open::that_detached(&info.html_url);
                }
                if ui.button("Skip this version").clicked() {
                    skip = true;
                }
                if ui.button("Later").clicked() {
                    later = true;
                }
            });
        });
    if install {
        app.update.installing = true;
        let _ = app.worker.jobs.send(Job::InstallUpdate {
            info,
            data_dir: app.data_dir.clone(),
        });
    } else if skip {
        app.settings.skipped_version = info.tag.clone();
        app.save_settings();
        app.update.available = None;
        app.update.show_window = false;
    } else if later || !open {
        app.update.show_window = false;
    }
}

/// The "Updates" section in Settings.
pub fn settings_section(app: &mut App, ui: &mut Ui) {
    ui.label(bold(format!("You're on {}", crate::VERSION)));
    ui.add_space(4.0);
    let mut channel = app.settings.channel();
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label("Update channel");
        for c in UpdateChannel::ALL {
            changed |= ui.selectable_value(&mut channel, c, c.label()).changed();
        }
    });
    ui.label(RichText::new(channel.help()).small().weak());
    if changed {
        app.settings.update_channel = Some(channel);
        app.settings.skipped_version.clear();
        app.save_settings();
        if channel != UpdateChannel::Off {
            check(app, false);
        } else {
            app.update.available = None;
        }
    }

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if app.update.checking {
            ui.spinner();
            ui.label("Checking…");
        } else if ui.button("🔄 Check now").clicked() {
            check(app, true);
        }
        if app.update.available.is_some() && ui.button("Show update").clicked() {
            app.update.show_window = true;
        }
        if !app.update.status.is_empty() {
            ui.label(RichText::new(&app.update.status).weak());
        }
    });
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(
                "Switching from Beta back to Stable won't downgrade you; you'll get the next stable release once \
                 it's newer. Older versions can be downloaded from",
            )
            .small()
            .weak(),
        );
        ui.hyperlink_to(RichText::new("the Releases page").small(), update::releases_page());
    });
}
