//! Update notifications: the bottom bar and Home button, the "update available" window and the
//! Settings section. Deliberately plain for now; the UI will be redesigned later.

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
            let _ = app.worker.jobs.send(Job::InstallUpdate { info });
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
            app.update.installing = false;
            app.save_now();
            // Restarting now would stop at the "not saved" question while the new version waits.
            if app.unsaved_error.is_some() {
                app.update.show_window = false;
                app.toast(
                    ToastKind::Info,
                    "Update installed. Restart the app to use it once your changes are saved.",
                );
                return;
            }
            // The new process waits for this one to release the single-instance lock.
            match std::process::Command::new(&exe).arg(crate::AFTER_UPDATE).spawn() {
                Ok(_) => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
                Err(e) => {
                    // The new exe won't start: put the running version back.
                    let (_, old) = crate::update::staging(&exe);
                    let restored = std::fs::rename(&exe, crate::update::staging(&exe).0)
                        .and_then(|()| std::fs::rename(&old, &exe));
                    let msg = match restored {
                        Ok(()) => format!("The new version wouldn't start ({e}), so this version was kept."),
                        Err(e2) => format!(
                            "The new version wouldn't start ({e}) and the previous one couldn't be put back \
                             ({e2}). It's at {}; rename it to {} to use it.",
                            old.display(),
                            exe.file_name().unwrap_or_default().to_string_lossy()
                        ),
                    };
                    app.toast(ToastKind::Error, msg);
                }
            }
        }
        Err(e) => {
            app.update.installing = false;
            app.toast(ToastKind::Error, e);
        }
    }
}

/// Height of the update bar, so toasts can sit above it.
pub const BAR_HEIGHT: f32 = 44.0;

/// A slim bar along the bottom of the window while an update is waiting, with the
/// button at the bottom right. Must be added before the tab's own panels.
pub fn bar(app: &mut App, ctx: &egui::Context) {
    let Some(info) = app.update.available.clone() else { return };
    egui::TopBottomPanel::bottom("update_bar")
        .exact_height(BAR_HEIGHT)
        .frame(egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(14, 6)))
        .show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                let kind = if info.prerelease { "beta" } else { "version" };
                ui.label(RichText::new(format!("A new {kind} of Cardboard Capital is available.")).weak());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| button(app, ui));
            });
        });
}

/// "⬆ Update to vX", shown while an update is waiting; opens the update window.
pub fn button(app: &mut App, ui: &mut Ui) {
    let Some(info) = &app.update.available else { return };
    let label = format!("⬆  Update to {}", info.tag);
    if ui
        .add(egui::Button::new(bold(label).color(egui::Color32::from_rgb(24, 20, 12))).fill(ACCENT))
        .on_hover_text("See what's new and install it")
        .clicked()
    {
        app.update.show_window = true;
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
        let _ = app.worker.jobs.send(Job::InstallUpdate { info });
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
