//! "Settings" tab.

use egui::{RichText, Ui};

use crate::app::App;
use crate::storage;
use crate::ui::widgets::{self, bold};

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

    egui::CentralPanel::default().show(ctx, |ui| {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.set_max_width(760.0);
            let export_path = app.export_path();
            let data_dir = app.data_dir.clone();
            let app_dir = app.app_dir.clone();
            let fx_loading = app.fx_loading;
            let s = &mut app.settings;

            section(ui, "Currency (USD → SGD)", |ui| {
                ui.horizontal(|ui| {
                    ui.label(bold(format!("Live rate: 1 USD = S$ {:.4}", s.fx_rate)));
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
                                    .range(0.1..=10.0)
                                    .fixed_decimals(4)
                                    .prefix("S$ "),
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
                        "Prices come from Scryfall in USD (TCGplayer market price) and are converted with this rate. \
                         The rate is also written into the spreadsheet (Summary!B4), so you can change it there too.",
                    )
                    .small()
                    .weak(),
                );
            });

            section(ui, "Spreadsheet", |ui| {
                ui.label("Export file");
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(export_path.display().to_string()).monospace());
                });
                ui.horizontal(|ui| {
                    if ui.button("Change…").clicked() {
                        if let Some(p) = rfd::FileDialog::new()
                            .add_filter("Excel workbook", &["xlsx"])
                            .set_file_name("Cardboard Capital.xlsx")
                            .save_file()
                        {
                            let mut p = p;
                            if p.extension().is_none() {
                                p.set_extension("xlsx");
                            }
                            s.export_path = p.display().to_string();
                            settings_changed = true;
                            reexport = true;
                        }
                    }
                    if !s.export_path.is_empty() && ui.button("Reset to default").clicked() {
                        s.export_path.clear();
                        settings_changed = true;
                        reexport = true;
                    }
                });
                ui.add_space(6.0);
                settings_changed |= ui
                    .checkbox(&mut s.auto_export, "Update the spreadsheet automatically after every change")
                    .changed();
                ui.horizontal(|ui| {
                    ui.label("Highlight rows worth at least");
                    if ui
                        .add(egui::DragValue::new(&mut s.highlight_sgd).speed(0.5).range(0.0..=100000.0).prefix("S$ "))
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

            section(ui, "Appearance", |ui| {
                ui.horizontal(|ui| {
                    settings_changed |= ui.selectable_value(&mut s.dark_mode, true, "🌙 Dark").changed();
                    settings_changed |= ui.selectable_value(&mut s.dark_mode, false, "☀ Light").changed();
                });
            });

            section(ui, "Data", |ui| {
                ui.label("Your collection and settings are saved in the app's data folder:");
                ui.label(RichText::new(data_dir.join("collection.json").display().to_string()).monospace());
                ui.label(
                    RichText::new(
                        "A copy from the start of each session is kept as collection.backup.json. \
                         To move the app, move its whole folder (the exe plus data and exports).",
                    )
                    .small()
                    .weak(),
                );
                ui.horizontal(|ui| {
                    if ui.button("📁 Open data folder").clicked() {
                        let _ = open::that_detached(&data_dir);
                    }
                    if ui.button("📁 Open app folder").clicked() {
                        let _ = open::that_detached(&app_dir);
                    }
                    if ui.button("🔄 Refresh all prices").clicked() {
                        refresh = true;
                    }
                    if ui.button("🧹 Clear image cache").clicked() {
                        let _ = std::fs::remove_dir_all(storage::cache_dir(&data_dir));
                        let _ = std::fs::create_dir_all(storage::cache_dir(&data_dir));
                    }
                });
            });

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
}
