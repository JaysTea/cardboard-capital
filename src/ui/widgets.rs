//! Theme, fonts and small reusable widgets.

use std::sync::Arc;
use std::time::Duration;

use egui::{
    Align2, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Frame, Id,
    Margin, Rect, Response, RichText, Sense, Stroke, TextStyle, Theme, Ui, Vec2,
};

use crate::app::Images;
use crate::model::rarity_label;

pub const ACCENT: Color32 = Color32::from_rgb(214, 162, 58);
/// Width / height of a Magic card.
pub const CARD_ASPECT: f32 = 488.0 / 680.0;
const BOLD: &str = "bold";

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let fonts_dir = std::path::Path::new(r"C:\Windows\Fonts");
    let mut bold_family = Vec::new();

    if let Ok(bytes) = std::fs::read(fonts_dir.join("segoeui.ttf")) {
        fonts
            .font_data
            .insert("segoe".into(), Arc::new(FontData::from_owned(bytes)));
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "segoe".into());
    }
    if let Ok(bytes) = std::fs::read(fonts_dir.join("segoeuib.ttf")) {
        fonts
            .font_data
            .insert("segoe_bold".into(), Arc::new(FontData::from_owned(bytes)));
        bold_family.push("segoe_bold".to_string());
    }
    // Fall back to the regular proportional stack (also covers emoji/icons).
    bold_family.extend(
        fonts
            .families
            .get(&FontFamily::Proportional)
            .cloned()
            .unwrap_or_default(),
    );
    fonts
        .families
        .insert(FontFamily::Name(BOLD.into()), bold_family);
    ctx.set_fonts(fonts);
}

/// Text in the bold font family.
pub fn bold(text: impl Into<String>) -> RichText {
    RichText::new(text).family(FontFamily::Name(BOLD.into()))
}

pub fn apply_theme(ctx: &egui::Context, dark: bool) {
    let theme = if dark { Theme::Dark } else { Theme::Light };
    let mut v = if dark {
        egui::Visuals::dark()
    } else {
        egui::Visuals::light()
    };
    if dark {
        v.panel_fill = Color32::from_rgb(24, 26, 33);
        v.window_fill = Color32::from_rgb(31, 34, 43);
        v.extreme_bg_color = Color32::from_rgb(16, 18, 23);
        v.faint_bg_color = Color32::from_rgb(33, 36, 46);
        v.selection.stroke = Stroke::new(1.0_f32, Color32::WHITE);
    } else {
        v.panel_fill = Color32::from_rgb(246, 247, 250);
        v.window_fill = Color32::WHITE;
        v.extreme_bg_color = Color32::WHITE;
        v.faint_bg_color = Color32::from_rgb(236, 239, 244);
        v.selection.stroke = Stroke::new(1.0_f32, Color32::BLACK);
    }
    v.selection.bg_fill = ACCENT.gamma_multiply(if dark { 0.55 } else { 0.5 });
    v.hyperlink_color = ACCENT;
    let r = CornerRadius::same(4);
    v.widgets.noninteractive.corner_radius = r;
    v.widgets.inactive.corner_radius = r;
    v.widgets.hovered.corner_radius = r;
    v.widgets.active.corner_radius = r;
    v.widgets.open.corner_radius = r;
    v.window_corner_radius = CornerRadius::same(10);
    v.striped = false;

    ctx.set_theme(theme);
    ctx.set_visuals_of(theme, v);
    ctx.style_mut_of(theme, |s| {
        s.spacing.item_spacing = egui::vec2(8.0, 7.0);
        s.spacing.button_padding = egui::vec2(10.0, 5.0);
        s.spacing.interact_size.y = 26.0;
        s.text_styles
            .insert(TextStyle::Body, FontId::proportional(14.5));
        s.text_styles
            .insert(TextStyle::Button, FontId::proportional(14.5));
        s.text_styles.insert(
            TextStyle::Heading,
            FontId::new(22.0, FontFamily::Name(BOLD.into())),
        );
        s.text_styles
            .insert(TextStyle::Small, FontId::proportional(12.0));
        s.text_styles
            .insert(TextStyle::Monospace, FontId::monospace(13.5));
    });
}

pub fn rarity_color(rarity: &str) -> Color32 {
    match rarity {
        "common" => Color32::from_rgb(150, 156, 166),
        "uncommon" => Color32::from_rgb(140, 182, 210),
        "rare" => Color32::from_rgb(222, 180, 60),
        "mythic" => Color32::from_rgb(236, 110, 44),
        _ => Color32::from_rgb(176, 124, 216),
    }
}

pub fn chip(ui: &mut Ui, text: &str, fg: Color32, bg: Color32) -> Response {
    Frame::default()
        .fill(bg)
        .corner_radius(10)
        .inner_margin(Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text).color(fg).size(12.5).strong())
        })
        .response
}

pub fn rarity_chip(ui: &mut Ui, rarity: &str) -> Response {
    let c = rarity_color(rarity);
    chip(ui, &rarity_label(rarity), c, c.gamma_multiply(0.2))
}

pub fn warning(ui: &mut Ui, text: &str) {
    let amber = Color32::from_rgb(235, 170, 40);
    Frame::default()
        .fill(amber.gamma_multiply(0.14))
        .stroke(Stroke::new(1.0_f32, amber.gamma_multiply(0.6)))
        .corner_radius(6)
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("⚠").color(amber));
                ui.label(text);
            });
        });
}

pub fn primary_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(
        egui::Button::new(bold(text).color(Color32::from_rgb(24, 20, 12)).size(15.5))
            .fill(ACCENT)
            .min_size(egui::vec2(0.0, 34.0)),
    )
}

/// Paints a card image into `rect`, with a spinner/placeholder while it loads.
pub fn paint_card_image(ui: &mut Ui, images: &mut Images, url: &str, rect: Rect) {
    if !ui.is_rect_visible(rect) {
        return;
    }
    let radius = (rect.width() * 0.045).clamp(4.0, 14.0);
    if let Some(tex) = images.get(url) {
        egui::Image::new((tex.id(), rect.size()))
            .corner_radius(radius)
            .paint_at(ui, rect);
    } else {
        ui.painter()
            .rect_filled(rect, radius, ui.visuals().faint_bg_color);
        if images.failed(url) {
            ui.painter().text(
                rect.center(),
                Align2::CENTER_CENTER,
                "No image",
                FontId::proportional(13.0),
                ui.visuals().weak_text_color(),
            );
        } else {
            egui::Spinner::new().paint_at(
                ui,
                Rect::from_center_size(rect.center(), egui::vec2(22.0, 22.0)),
            );
        }
    }
}

pub fn card_image(ui: &mut Ui, images: &mut Images, url: &str, size: Vec2) -> Response {
    let (rect, resp) = ui.allocate_exact_size(size, Sense::hover());
    paint_card_image(ui, images, url, rect);
    resp
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToastKind {
    Info,
    Success,
    Error,
}

pub struct Toast {
    kind: ToastKind,
    text: String,
    shown_at: Option<f64>,
}

impl Toast {
    pub fn new(kind: ToastKind, text: String) -> Self {
        Toast {
            kind,
            text,
            shown_at: None,
        }
    }

    fn duration(&self) -> f64 {
        match self.kind {
            ToastKind::Error => 9.0,
            _ => 4.0,
        }
    }
}

pub fn show_toasts(ctx: &egui::Context, toasts: &mut Vec<Toast>, now: f64) {
    toasts.retain(|t| t.shown_at.is_none_or(|s| now - s < t.duration()));
    if toasts.is_empty() {
        return;
    }
    ctx.request_repaint_after(Duration::from_millis(250));
    let start = toasts.len().saturating_sub(4);
    let mut dismiss = None;
    egui::Area::new(Id::new("toasts"))
        .anchor(Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -16.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.set_max_width(440.0);
            for (i, t) in toasts.iter_mut().enumerate().skip(start) {
                t.shown_at.get_or_insert(now);
                let (color, icon) = match t.kind {
                    ToastKind::Success => (Color32::from_rgb(70, 190, 110), "✔"),
                    ToastKind::Error => (Color32::from_rgb(230, 80, 70), "⚠"),
                    ToastKind::Info => (Color32::from_rgb(90, 150, 230), "ℹ"),
                };
                let resp = Frame::default()
                    .fill(ui.visuals().window_fill)
                    .stroke(Stroke::new(1.0_f32, color))
                    .corner_radius(8)
                    .inner_margin(Margin::symmetric(12, 9))
                    .shadow(ui.visuals().popup_shadow)
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(icon).color(color).size(16.0));
                            ui.add(egui::Label::new(&t.text).wrap());
                        });
                    })
                    .response
                    .interact(Sense::click())
                    .on_hover_text("Click to dismiss");
                if resp.clicked() {
                    dismiss = Some(i);
                }
                ui.add_space(6.0);
            }
        });
    if let Some(i) = dismiss {
        toasts.remove(i);
    }
}
