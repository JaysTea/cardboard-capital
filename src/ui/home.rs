//! Home: the total and its split between games, a tile per game with its value, and only the
//! account, Settings and Update buttons. A placeholder layout until the real design is in.

use std::f32::consts::TAU;

use egui::{
    pos2, vec2, Align, Align2, Color32, CornerRadius, FontId, Frame, Layout, Margin, Pos2, Rect,
    RichText, Sense, Shape, Stroke, StrokeKind, Ui, UiBuilder,
};

use crate::app::{App, Screen};
use crate::cloud;
use crate::model::{self, Game, HomeChart, Value};
use crate::ui::widgets::{self, bold, chip, game_color, ACCENT, CARD_ASPECT};

/// Room the floating Update button takes at the bottom right, so toasts can sit above it.
pub const UPDATE_HEIGHT: f32 = 76.0;
const HEADER_HEIGHT: f32 = 68.0;
const EMPTY: &str = "Add cards to see your collection's value";

/// Settings' header: the way back, logo, name and version.
pub fn top_bar(app: &mut App, ctx: &egui::Context) {
    egui::TopBottomPanel::top("settings_header")
        .frame(Frame::side_top_panel(&ctx.style()).inner_margin(Margin::symmetric(16, 10)))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("← Home").clicked() {
                    app.screen = Screen::Home;
                }
                ui.add(egui::Image::new((app.logo.id(), vec2(26.0, 26.0))));
                ui.label(bold("Cardboard Capital").size(20.0).color(ACCENT));
                ui.label(RichText::new(crate::VERSION).weak());
            });
        });
}

/// Account on the left, the name with the version under it in the middle, Settings on the right.
fn header(app: &mut App, ctx: &egui::Context) {
    egui::TopBottomPanel::top("home_header")
        .exact_height(HEADER_HEIGHT)
        .frame(Frame::side_top_panel(&ctx.style()).inner_margin(Margin::symmetric(16, 8)))
        .show(ctx, |ui| {
            let r = ui.max_rect();
            let side = |left: bool| {
                let w = 160.0;
                let x = if left { r.left() } else { r.right() - w };
                Rect::from_min_size(pos2(x, r.top()), vec2(w, r.height()))
            };
            ui.allocate_new_ui(UiBuilder::new().max_rect(side(true)).layout(Layout::left_to_right(Align::Center)), |ui| {
                let account = ui
                    .add(egui::Button::new(RichText::new("👤").size(20.0)).corner_radius(18).min_size(vec2(38.0, 36.0)))
                    .on_hover_text("Account");
                if account.clicked() {
                    app.account_open = !app.account_open;
                }
                if app.account.is_developer() {
                    chip(ui, "DEV", ACCENT, ACCENT.gamma_multiply(0.2));
                }
            });
            ui.allocate_new_ui(UiBuilder::new().max_rect(r).layout(Layout::top_down(Align::Center)), |ui| {
                ui.label(bold("Cardboard Capital").size(24.0).color(ACCENT));
                ui.label(RichText::new(crate::VERSION).small().weak());
            });
            ui.allocate_new_ui(UiBuilder::new().max_rect(side(false)).layout(Layout::right_to_left(Align::Center)), |ui| {
                if ui.add(egui::Button::new(RichText::new("⚙").size(20.0)).min_size(vec2(38.0, 36.0))).on_hover_text("Settings").clicked() {
                    app.screen = Screen::Settings;
                }
                // Placeholder from the design sketch; what it does is still to be decided.
                ui.add_enabled(false, egui::Button::new(RichText::new("↔").size(20.0)).min_size(vec2(38.0, 36.0)))
                    .on_disabled_hover_text("Coming soon");
            });
        });
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    header(app, ctx);

    let totals: Vec<(Game, Value)> = Game::ALL.iter().map(|&g| (g, app.game_total(g))).collect();
    let grand: f64 = totals.iter().map(|(_, v)| v.total).sum();
    let mut open = None;
    if app.settings.home_chart == HomeChart::Donut {
        egui::SidePanel::right("home_chart")
            .resizable(false)
            .exact_width(300.0)
            .show(ctx, |ui| {
                ui.add_space(14.0);
                ui.label(bold("Collection value").size(17.0));
                ui.add_space(10.0);
                ui.vertical_centered(|ui| donut(ui, &totals, 220.0));
                ui.add_space(10.0);
                legend(ui, &totals);
            });
    }
    egui::CentralPanel::default().show(ctx, |ui| {
        ui.add_space(12.0);
        if app.settings.home_chart == HomeChart::Bar {
            ui.vertical_centered(|ui| {
                ui.label(bold(model::cur().fmt(grand)).size(26.0));
            });
            ui.add_space(6.0);
            // The bar at 70% of the width, centred, with the (?) key to its right.
            let w = ui.available_width();
            let bar_w = (w * 0.7).max(200.0);
            ui.horizontal(|ui| {
                ui.add_space((w - bar_w) / 2.0);
                ui.allocate_ui(vec2(bar_w, 22.0), |ui| stacked_bar(ui, &totals, 22.0));
                ui.add_space(8.0);
                ui.label(RichText::new("(?)").weak()).on_hover_ui(|ui| {
                    legend(ui, &totals);
                    ui.label(RichText::new("Bar or Donut: Settings → Appearance.").small().weak());
                });
            });
            ui.add_space(24.0);
        }
        // Tiles as tall as the screen allows, scrolling sideways when there are more games.
        let tile_h = (ui.available_height() - 64.0).clamp(220.0, 620.0);
        egui::ScrollArea::horizontal().id_salt("game_tiles").show(ui, |ui| {
            ui.horizontal_top(|ui| {
                ui.add_space(24.0);
                for (game, value) in &totals {
                    if tile(ui, *game, value, tile_h * CARD_ASPECT).clicked() {
                        open = Some(*game);
                    }
                    ui.add_space(40.0);
                }
            });
        });
    });
    update_button(app, ctx);
    account_panel(app, ctx);
    if let Some(game) = open {
        app.screen = Screen::Game(game);
    }
}

/// Floating at the bottom right while an update is waiting: Update (with the version) and ✕.
fn update_button(app: &mut App, ctx: &egui::Context) {
    let Some(info) = app.update.available.clone() else { return };
    if app.update.dismissed {
        return;
    }
    egui::Area::new(egui::Id::new("home_update"))
        .anchor(Align2::RIGHT_BOTTOM, [-16.0, -16.0])
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                let dark = Color32::from_rgb(24, 20, 12);
                let label = RichText::new(format!("⬆  Update\n{}", info.tag)).color(dark).strong().size(16.0);
                if ui
                    .add(egui::Button::new(label).fill(Color32::from_rgb(240, 214, 60)).min_size(vec2(150.0, 56.0)))
                    .on_hover_text("See what's new and install it")
                    .clicked()
                {
                    app.update.show_window = true;
                }
                let close = RichText::new("×").color(Color32::WHITE).size(18.0);
                if ui
                    .add(egui::Button::new(close).fill(Color32::from_rgb(214, 72, 52)).min_size(vec2(40.0, 56.0)))
                    .on_hover_text("Hide until next time (still in Settings → Updates)")
                    .clicked()
                {
                    app.update.dismissed = true;
                }
            });
        });
}

/// A small window under the account button. Online accounts are a placeholder for now.
fn account_panel(app: &mut App, ctx: &egui::Context) {
    if !app.account_open {
        return;
    }
    let mut open = true;
    egui::Window::new("Account")
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .default_width(320.0)
        .anchor(Align2::LEFT_TOP, [16.0, HEADER_HEIGHT + 8.0])
        .show(ctx, |ui| {
            let acc = &app.account;
            if acc.is_developer() {
                ui.horizontal(|ui| {
                    chip(ui, "DEV", ACCENT, ACCENT.gamma_multiply(0.2));
                    ui.label(bold(&acc.name));
                });
                ui.label(
                    RichText::new(format!("Developer key {} · issued {}", acc.key_id, acc.issued))
                        .small()
                        .weak(),
                );
                ui.add_space(4.0);
                ui.label(format!("Cloud: {}", cloud::status(acc).describe()));
            } else {
                ui.label(bold("Not signed in"));
                ui.label(RichText::new(cloud::status(acc).describe()).weak());
            }
            ui.add_space(6.0);
            if ui.button("📁 Open account folder").clicked() {
                let _ = std::fs::create_dir_all(&app.paths.account);
                let _ = open::that_detached(&app.paths.account);
            }
        });
    if !open {
        app.account_open = false;
    }
}


/// A vertical, card-shaped tile for a game, with its value centred underneath.
fn tile(ui: &mut Ui, game: Game, value: &Value, width: f32) -> egui::Response {
    ui.vertical(|ui| {
        ui.set_width(width);
        let (rect, resp) = ui.allocate_exact_size(vec2(width, width / CARD_ASPECT), Sense::click());
        let resp = resp
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(format!("Open {}", game.name()));
        let p = ui.painter();
        let radius = 12;
        p.rect_filled(rect, radius, game_color(game));
        let band = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 52.0), rect.max);
        p.rect_filled(
            band,
            CornerRadius { nw: 0, ne: 0, sw: radius, se: radius },
            Color32::from_black_alpha(80),
        );
        p.text(
            rect.center() - vec2(0.0, width * 0.12),
            Align2::CENTER_CENTER,
            game.short_name(),
            widgets::bold_font((width * 0.22).max(28.0)),
            Color32::WHITE,
        );
        p.text(band.center(), Align2::CENTER_CENTER, game.name(), FontId::proportional(13.0), Color32::WHITE);
        if resp.hovered() {
            p.rect_stroke(rect, radius, Stroke::new(2.5_f32, ACCENT), StrokeKind::Outside);
        }
        ui.add_space(4.0);
        ui.vertical_centered(|ui| {
            ui.label(bold(model::cur().fmt(value.total)).size(16.0));
            ui.label(RichText::new(format!("{} cards", value.cards)).small().weak());
        });
        resp
    })
    .inner
}

fn shares(totals: &[(Game, Value)]) -> Vec<(Game, f64)> {
    let values: Vec<(Game, f64)> = totals.iter().map(|(g, v)| (*g, v.total)).collect();
    model::value_shares(&values)
}

fn share_text(game: Game, totals: &[(Game, Value)], share: f64) -> String {
    let value = totals.iter().find(|(g, _)| *g == game).map(|(_, v)| v.total).unwrap_or(0.0);
    format!("{}: {} ({:.0}%)", game.name(), model::cur().fmt(value), share * 100.0)
}

/// Each game's share of the value as one bar.
fn stacked_bar(ui: &mut Ui, totals: &[(Game, Value)], height: f32) {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 6, ui.visuals().faint_bg_color);
    let shares = shares(totals);
    if shares.is_empty() {
        p.text(rect.center(), Align2::CENTER_CENTER, EMPTY, FontId::proportional(13.0), ui.visuals().weak_text_color());
        return;
    }
    let mut x = rect.left();
    let mut hovered = None;
    for (i, &(game, share)) in shares.iter().enumerate() {
        let last = i + 1 == shares.len();
        let w = if last { rect.right() - x } else { share as f32 * rect.width() };
        let seg = Rect::from_min_max(pos2(x, rect.top()), pos2(x + w, rect.bottom()));
        let (l, r) = (if i == 0 { 6 } else { 0 }, if last { 6 } else { 0 });
        p.rect_filled(seg, CornerRadius { nw: l, sw: l, ne: r, se: r }, game_color(game));
        if resp.hover_pos().is_some_and(|pos| seg.contains(pos)) {
            hovered = Some((game, share));
        }
        x += w;
    }
    if let Some((game, share)) = hovered {
        resp.on_hover_text_at_pointer(share_text(game, totals, share));
    }
}

/// Each game's share of the value as a ring, with the total in the middle.
fn donut(ui: &mut Ui, totals: &[(Game, Value)], diameter: f32) {
    let (resp, painter) = ui.allocate_painter(egui::Vec2::splat(diameter), Sense::hover());
    let c = resp.rect.center();
    let outer = diameter * 0.5 - 4.0;
    let thick = outer * 0.32;
    let mid = outer - thick * 0.5;
    let total: f64 = totals.iter().map(|(_, v)| v.total).sum();
    let shares = shares(totals);
    if shares.is_empty() {
        painter.add(ring(c, mid, Stroke::new(thick, ui.visuals().faint_bg_color)));
        painter.text(c, Align2::CENTER_CENTER, "No value yet", FontId::proportional(13.0), ui.visuals().weak_text_color());
        return;
    }
    // Which slice the pointer is over: angle 0 at 12 o'clock, clockwise (y points down).
    let hovered = resp.hover_pos().and_then(|pos| {
        let d = pos - c;
        if d.length() < outer - thick || d.length() > outer {
            return None;
        }
        let a = (d.y.atan2(d.x) + TAU * 0.25).rem_euclid(TAU);
        let mut acc = 0.0_f32;
        shares.iter().position(|(_, s)| {
            acc += *s as f32 * TAU;
            a < acc
        })
    });
    let mut start = -TAU * 0.25;
    for (i, &(game, share)) in shares.iter().enumerate() {
        let width = if hovered == Some(i) { thick + 6.0 } else { thick };
        // One game is the whole ring: a closed ring has no seam.
        if shares.len() == 1 {
            painter.add(ring(c, mid, Stroke::new(width, game_color(game))));
            break;
        }
        let sweep = share as f32 * TAU;
        let steps = ((sweep / TAU) * 128.0).ceil().max(2.0) as usize;
        let points: Vec<Pos2> = (0..=steps)
            .map(|k| {
                let a = start + sweep * k as f32 / steps as f32;
                c + mid * vec2(a.cos(), a.sin())
            })
            .collect();
        painter.add(Shape::line(points, Stroke::new(width, game_color(game))));
        start += sweep;
    }
    painter.text(
        c,
        Align2::CENTER_CENTER,
        model::cur().fmt(total),
        widgets::bold_font(17.0),
        ui.visuals().strong_text_color(),
    );
    if let Some(i) = hovered {
        let (game, share) = shares[i];
        resp.on_hover_text_at_pointer(share_text(game, totals, share));
    }
}

/// A full ring with the stroke centred on `radius` (`circle_stroke` draws outside it).
fn ring(center: Pos2, radius: f32, stroke: Stroke) -> Shape {
    let points = (0..128)
        .map(|k| {
            let a = TAU * k as f32 / 128.0;
            center + radius * vec2(a.cos(), a.sin())
        })
        .collect();
    Shape::closed_line(points, stroke)
}

/// Colour key with each game's value and share, then the grand total.
fn legend(ui: &mut Ui, totals: &[(Game, Value)]) {
    let grand: f64 = totals.iter().map(|(_, v)| v.total).sum();
    ui.horizontal_wrapped(|ui| {
        for (game, value) in totals {
            let (r, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
            ui.painter().rect_filled(r, 3, game_color(*game));
            ui.label(game.short_name());
            ui.label(bold(model::cur().fmt(value.total)));
            if grand > 0.0 {
                ui.label(RichText::new(format!("{:.0}%", value.total / grand * 100.0)).weak());
            }
            ui.add_space(12.0);
        }
    });
    ui.label(bold(format!("Total {}", model::cur().fmt(grand))));
}
