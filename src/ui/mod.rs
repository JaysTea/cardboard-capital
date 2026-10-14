pub mod add;
pub mod collection;
pub mod home;
pub mod import;
pub mod settings;
pub mod update;
pub mod widgets;

use egui::{Align, Color32, Layout, RichText};

use crate::app::{App, Screen, Tab};
use crate::model::Game;

use widgets::{bold, chip, ACCENT};

/// The header of a game's screen: back to Home, the game's tabs, its total and the exchange rate.
pub fn header(app: &mut App, ctx: &egui::Context) {
    let rate = app.rate();
    let value = app.game_total(Game::Mtg);

    egui::TopBottomPanel::top("header")
        .frame(
            egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(16, 10)),
        )
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .button("← Home")
                    .on_hover_text("Home: all your games")
                    .clicked()
                {
                    app.screen = Screen::Home;
                }
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
                        .on_hover_text("Refresh the live exchange rate")
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
                    ui.label(bold(format!("1 USD = {} {rate:.4}", crate::model::cur().symbol)))
                        .on_hover_text(hover);
                    if app.settings.fx_manual {
                        chip(ui, "MANUAL", ACCENT, ACCENT.gamma_multiply(0.2));
                    }
                    ui.separator();
                    ui.label(format!("{} cards · {}", value.cards, crate::model::cur().fmt(value.total)));
                    if let Some(why) = &app.collection_locked {
                        let red = Color32::from_rgb(230, 80, 70);
                        chip(ui, "NOT SAVING", red, red.gamma_multiply(0.2)).on_hover_text(why.as_str());
                    }
                });
            });
        });
}
