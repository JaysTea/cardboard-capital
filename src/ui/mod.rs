pub mod add;
pub mod collection;
pub mod import;
pub mod settings;
pub mod update;
pub mod widgets;

use egui::{Align, Layout, RichText};

use crate::app::{App, Tab};
use crate::model::money;
use widgets::{bold, chip, ACCENT};

pub fn header(app: &mut App, ctx: &egui::Context) {
    let rate = app.rate();
    let total_qty: u32 = app.collection.iter().map(|e| e.qty).sum();
    let total: f64 = app
        .collection
        .iter()
        .filter_map(|e| e.total_sgd(rate))
        .sum();

    egui::TopBottomPanel::top("header")
        .frame(
            egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(16, 10)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.add(egui::Image::new((app.logo.id(), egui::vec2(26.0, 26.0))));
                ui.label(bold("Cardboard Capital").size(20.0).color(ACCENT));
                ui.label(RichText::new(crate::VERSION).weak());
                ui.add_space(24.0);
                let tabs = [
                    (Tab::Add, "➕  Add card".to_string()),
                    (Tab::Import, "📥  Import".to_string()),
                    (
                        Tab::Collection,
                        format!("📚  Collection ({})", app.collection.len()),
                    ),
                    (Tab::Settings, "⚙  Settings".to_string()),
                ];
                for (tab, label) in tabs {
                    if ui
                        .selectable_label(app.tab == tab, RichText::new(label).size(15.5))
                        .clicked()
                    {
                        app.tab = tab;
                    }
                }

                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui
                        .add_enabled(!app.fx_loading, egui::Button::new("🔄"))
                        .on_hover_text("Refresh the live USD→SGD rate")
                        .clicked()
                    {
                        app.fetch_fx();
                    }
                    if app.fx_loading {
                        ui.spinner();
                    }
                    let hover = if app.settings.fx_manual {
                        "Manual rate (change it in Settings)".to_string()
                    } else {
                        format!(
                            "{}\nFetched {}",
                            app.settings.fx_source, app.settings.fx_updated
                        )
                    };
                    ui.label(bold(format!("1 USD = S$ {rate:.4}")))
                        .on_hover_text(hover);
                    if app.settings.fx_manual {
                        chip(ui, "MANUAL", ACCENT, ACCENT.gamma_multiply(0.2));
                    }
                    ui.separator();
                    ui.label(format!("{total_qty} cards · S$ {}", money(total)));
                    if app.update.available.is_some() {
                        ui.separator();
                        update::header_chip(app, ui);
                    }
                });
            });
        });
}
