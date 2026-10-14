//! Settings, opened from Home or a game's Settings tab: general settings first, then each game's.

use egui::{RichText, Ui};

use crate::app::App;
use crate::cloud::{self, CloudStatus};
use crate::model::HomeChart;
use crate::storage;
use crate::ui::widgets::{self, bold};

fn group(ui: &mut Ui, title: &str) {
    ui.add_space(22.0);
    ui.label(bold(title).size(21.0).color(widgets::ACCENT));
}

fn section(ui: &mut Ui, title: &str, add: impl FnOnce(&mut Ui)) {
    ui.add_space(14.0);
    ui.label(bold(title).size(17.0));
    ui.add_space(4.0);
    egui::Frame::default()
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(10)
        .inner_margin(14)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            add(ui);
        });
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    let now = ctx.input(|i| i.time);
    let mut settings_changed = false;
    let mut reexport = false;
    let mut fetch_fx = false;
    let mut export_now = false;
    let mut open_sheet = false;
    let mut refresh = false;
    let mut new_export_dir: Option<Option<std::path::PathBuf>> = None;
    let mut new_currency: Option<&'static str> = None;

    egui::CentralPanel::default().show(ctx, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.set_max_width(760.0);
            let export_path = app.export_path();
            let paths = app.paths.clone();
            let fx_loading = app.fx_loading;
            let pending = app.pending_currency.clone();
            let s = &mut app.settings;

            group(ui, "General");
            let cur = s.currency();
            section(ui, "Currency", |ui| {
                ui.horizontal(|ui| {
                    ui.label("Show prices in");
                    egui::ComboBox::from_id_salt("currency")
                        .selected_text(format!("{} – {}", cur.code, cur.name))
                        .width(230.0)
                        .height(420.0)
                        .show_ui(ui, |ui| {
                            for c in crate::model::CURRENCIES {
                                if ui
                                    .selectable_label(c.code == cur.code, format!("{} – {} ({})", c.code, c.name, c.symbol))
                                    .clicked()
                                {
                                    new_currency = Some(c.code);
                                }
                            }
                        });
                    if pending.is_some() {
                        ui.spinner();
                        ui.label(RichText::new("Switching…").weak());
                    }
                });
                ui.label(
                    RichText::new(
                        "Prices come from Scryfall in US dollars (TCGplayer market) and are converted at a live rate. \
                         Switching converts your manual prices too.",
                    )
                    .small()
                    .weak(),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    ui.label(bold(format!("Live rate: 1 USD = {} {:.4}", cur.symbol, s.fx_rate)));
                    if fx_loading {
                        ui.spinner();
                    } else if ui.button("🔄 Refresh").clicked() {
                        fetch_fx = true;
                    }
                });
                ui.label(
                    RichText::new(format!("Source: {}   ·   fetched {}", s.fx_source, s.fx_updated))
                        .small()
                        .weak(),
                );
                ui.add_space(6.0);
                if ui.checkbox(&mut s.fx_manual, "Use a manual rate instead").changed() {
                    settings_changed = true;
                    reexport = true;
                }
                ui.add_enabled_ui(s.fx_manual, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("1 USD =");
                        if ui
                            .add(
                                egui::DragValue::new(&mut s.fx_manual_rate)
                                    .speed(0.001)
                                    .range(0.0001..=100000.0)
                                    .fixed_decimals(4)
                                    .prefix(format!("{} ", cur.symbol)),
                            )
                            .changed()
                        {
                            settings_changed = true;
                            reexport = true;
                        }
                    });
                });
                ui.label(
                    RichText::new(
                        "The rate is also written into the spreadsheet (Summary!B4), so you can change it there too.",
                    )
                    .small()
                    .weak(),
                );
            });

            section(ui, "Appearance", |ui| {
                ui.horizontal(|ui| {
                    settings_changed |= ui.selectable_value(&mut s.dark_mode, true, "🌙 Dark").changed();
                    settings_changed |= ui.selectable_value(&mut s.dark_mode, false, "☀ Light").changed();
                });
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.label("Home screen value chart");
                    for c in HomeChart::ALL {
                        settings_changed |= ui.selectable_value(&mut s.home_chart, c, c.label()).changed();
                    }
                });
                ui.label(
                    RichText::new("Bar: across the top of Home. Donut: in its own panel on the right.")
                        .small()
                        .weak(),
                );
            });

            section(ui, "Data", |ui| {
                if paths.legacy {
                    widgets::warning(
                        ui,
                        "For now the app is using its old data folder next to the app (see the message shown at start-up).",
                    );
                }
                ui.label("Your collection and settings are saved in:");
                ui.label(RichText::new(paths.root.display().to_string()).monospace());
                ui.label(
                    RichText::new(format!(
                        "Back up this folder. A copy from the start of each session is kept as collection.backup.json. \
                         The app itself can live anywhere; to keep everything next to it instead (e.g. on a USB \
                         stick), put a file named {} next to the app (copy this folder's data folder there first).",
                        storage::PORTABLE_MARKER
                    ))
                    .small()
                    .weak(),
                );
                ui.horizontal(|ui| {
                    if ui.button("📁 Open Cardboard Capital folder").clicked() {
                        let _ = open::that_detached(&paths.root);
                    }
                    if ui.button("📁 Open app folder").clicked() {
                        let _ = open::that_detached(&paths.exe_dir);
                    }
                    if ui.button("Clear image cache").clicked() {
                        let _ = std::fs::remove_dir_all(&paths.cache);
                        let _ = std::fs::create_dir_all(&paths.cache);
                    }
                });
            });

            section(ui, "Updates", |ui| crate::ui::update::settings_section(app, ui));

            if app.account.is_developer() {
                developer_sections(app, ui, &paths);
            }

            section(ui, "About", |ui| {
                ui.label(bold(format!("Cardboard Capital {}", crate::VERSION)));
                ui.label("Card data, images and prices from Scryfall (scryfall.com). Exchange rates from Frankfurter (ECB) with ExchangeRate-API as a fallback.");
                ui.label(
                    RichText::new(
                        "Magic: The Gathering is © Wizards of the Coast. This tool isn't affiliated with Wizards of the Coast or Scryfall.",
                    )
                    .small()
                    .weak(),
                );
            });

            group(ui, "Magic: The Gathering");
            let s = &mut app.settings;
            section(ui, "Spreadsheet", |ui| {
                ui.label("Your spreadsheet is saved as");
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(export_path.display().to_string()).monospace());
                });
                let custom = !s.export_dir.is_empty() || !s.export_path.is_empty();
                ui.horizontal(|ui| {
                    if ui.button("📁 Choose folder…").clicked() {
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            new_export_dir = Some(Some(dir));
                        }
                    }
                    if custom && ui.button("Use the default folder").clicked() {
                        new_export_dir = Some(None);
                    }
                    if ui.button("Open folder").clicked() {
                        if let Some(dir) = export_path.parent() {
                            let _ = std::fs::create_dir_all(dir);
                            let _ = open::that_detached(dir);
                        }
                    }
                });
                ui.add_space(6.0);
                settings_changed |= ui
                    .checkbox(&mut s.auto_export, "Update the spreadsheet automatically after every change")
                    .changed();
                ui.horizontal(|ui| {
                    ui.label("Highlight rows worth at least");
                    if ui
                        .add(egui::DragValue::new(&mut s.highlight_value).speed(0.5).range(0.0..=100000000.0).prefix(format!("{} ", cur.symbol)))
                        .changed()
                    {
                        settings_changed = true;
                        reexport = true;
                    }
                });
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    export_now = widgets::primary_button(ui, "⬇  Export now").clicked();
                    open_sheet = ui.button("📄 Open sheet").clicked();
                });
                ui.label(
                    RichText::new(
                        "Google Sheets: File → Import → Upload the .xlsx. LibreOffice/Excel: just open it. \
                         The sheet is regenerated from the app's data on each export, so make lasting edits here in the app.",
                    )
                    .small()
                    .weak(),
                );
            });

            section(ui, "Prices", |ui| {
                ui.horizontal(|ui| {
                    if ui.button("🔄 Refresh all prices").clicked() {
                        refresh = true;
                    }
                    ui.label(RichText::new("Re-fetches every card's price from Scryfall.").small().weak());
                });
            });
            ui.add_space(20.0);
        });
    });

    if settings_changed {
        app.save_settings();
    }
    if reexport {
        app.mark_dirty(now);
    }
    if fetch_fx {
        app.fetch_fx();
    }
    if export_now {
        app.export(true);
    }
    if open_sheet {
        app.open_export();
    }
    if refresh {
        app.refresh_prices();
    }
    if let Some(dir) = new_export_dir {
        app.set_export_dir(dir);
    }
    if let Some(code) = new_currency {
        app.change_currency(code);
    }
}

/// Shown only with a developer key: key details, where things live, and the cloud placeholder.
fn developer_sections(app: &App, ui: &mut Ui, paths: &storage::Paths) {
    section(ui, "Developer", |ui| {
        let acc = &app.account;
        ui.label(bold(format!("Developer key: {} (key {}, issued {})", acc.name, acc.key_id, acc.issued)));
        ui.label(
            RichText::new("Shows developer screens only; it doesn't change your data or updates.")
                .small()
                .weak(),
        );
        ui.add_space(4.0);
        egui::Grid::new("dev_paths").num_columns(2).spacing([12.0, 4.0]).show(ui, |ui| {
            for (label, path) in [
                ("Data", &paths.root),
                ("Settings", &paths.settings_dir),
                ("MTG collection", &paths.mtg_dir),
                ("Account", &paths.account),
                ("Image cache", &paths.cache),
                ("Machine-local", &paths.local),
                ("App", &paths.exe_dir),
            ] {
                ui.label(RichText::new(label).weak());
                ui.label(RichText::new(path.display().to_string()).monospace());
                ui.end_row();
            }
        });
        if ui.button("📁 Open account folder").clicked() {
            let _ = std::fs::create_dir_all(&paths.account);
            let _ = open::that_detached(&paths.account);
        }
    });

    section(ui, "Cloud (developer preview)", |ui| {
        let status = cloud::status(&app.account);
        ui.label(bold(status.describe()));
        ui.label(
            RichText::new(
                "Online storage is planned on Supabase. Builds connect to the project set in \
                 CARDBOARD_CAPITAL_SUPABASE_URL and CARDBOARD_CAPITAL_SUPABASE_ANON_KEY when they're built.",
            )
            .small()
            .weak(),
        );
        ui.horizontal(|ui| {
            let ready = status == CloudStatus::Ready;
            let _ = ui.add_enabled(false, egui::Button::new("Sign in")).on_disabled_hover_text(if ready {
                "Coming next"
            } else {
                "Supabase isn't configured yet"
            });
            let _ = ui.add_enabled(false, egui::Button::new("🔄 Sync now"));
        });
    });
}

/// Asks once where the spreadsheet should be saved.
pub fn export_prompt(app: &mut App, ctx: &egui::Context) {
    if app.settings.export_prompted {
        return;
    }
    // Someone who already picked a location in an older version has answered this.
    if !app.settings.export_path.is_empty() || !app.settings.export_dir.is_empty() {
        app.settings.export_prompted = true;
        app.save_settings();
        return;
    }
    let default = app.paths.default_export_path();
    let mut choice: Option<Option<std::path::PathBuf>> = None;
    egui::Window::new("Where should your spreadsheet go?")
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.set_max_width(460.0);
            ui.label(
                "Cardboard Capital keeps a spreadsheet of your collection up to date, ready for Google Sheets, \
                 Excel or LibreOffice. By default it's saved here:",
            );
            ui.label(RichText::new(default.display().to_string()).monospace());
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if widgets::primary_button(ui, "Keep it there (recommended)").clicked() {
                    choice = Some(None);
                }
                if ui.button("📁 Choose a folder…").clicked() {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        choice = Some(Some(dir));
                    }
                }
            });
            ui.label(
                RichText::new("You can change this any time in Settings → Spreadsheet.")
                    .small()
                    .weak(),
            );
        });
    if let Some(dir) = choice {
        app.set_export_dir(dir);
    }
}
