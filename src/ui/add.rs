//! "Add card" tab: look a card up, confirm the printing visually, add it.

use std::cmp::Reverse;
use std::time::Duration;

use egui::{
    Align, Align2, FontId, Key, Layout, Rect, Response, RichText, Sense, Stroke, TextEdit, Ui,
};

use crate::app::{App, Images};
use crate::model::{money, parse_money, rarity_label, Condition, Entry, Finish, RarityFilter};
use crate::scryfall::{parse_set_code, price_usd, printed_number_candidates, treatment, Card};
use crate::ui::widgets::{self, bold, ToastKind, ACCENT, CARD_ASPECT};
use crate::worker::{Job, SearchResult};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum LookupMode {
    #[default]
    Name,
    SetNumber,
}

#[derive(Default)]
pub struct AddState {
    pub mode: LookupMode,
    pub name: String,
    pub set_code: String,
    pub number: String,
    pub rarity: RarityFilter,
    pub finish: Finish,
    pub condition: Condition,
    pub qty: u32,
    pub manual_sgd: String,
    pub treatment_override: String,
    pub notes: String,

    suggestions: Vec<String>,
    show_suggestions: bool,
    suggestion_sel: Option<usize>,
    ac_seq: u64,
    ac_due: Option<f64>,

    search_seq: u64,
    searching: bool,
    error: Option<String>,
    prints: Vec<Card>,
    selected: Option<usize>,
    show_variants: bool,
    only_finish: bool,
    only_rarity: bool,
    focus_input: bool,
    printings_on_result: bool,
    /// `input_key()` at the time of the last lookup.
    searched: String,
}

impl AddState {
    pub fn new() -> Self {
        AddState {
            qty: 1,
            focus_input: true,
            ..Default::default()
        }
    }

    pub fn on_autocomplete(&mut self, seq: u64, result: Result<Vec<String>, String>) {
        if seq != self.ac_seq {
            return;
        }
        if let Ok(list) = result {
            // A single suggestion identical to what's typed adds nothing.
            let redundant = list.len() == 1 && list[0].eq_ignore_ascii_case(self.name.trim());
            // Nothing new typed since the last lookup (e.g. the box just regained focus).
            let unchanged = self.input_key() == self.searched;
            self.show_suggestions = !list.is_empty() && !redundant && !unchanged && !self.searching;
            self.suggestions = list;
            self.suggestion_sel = None;
        }
    }

    pub fn on_search(&mut self, seq: u64, result: Result<SearchResult, String>) {
        if seq != self.search_seq {
            return;
        }
        self.searching = false;
        match result {
            Ok(r) if r.prints.is_empty() => {
                self.error = Some("No paper printings found for that card.".into());
            }
            Ok(mut r) => {
                // A face-name search (e.g. "Lightning Bolt") also finds double-faced cards with
                // that face; list the cards whose full name matches first.
                let q = self.name.trim().to_lowercase();
                if self.mode == LookupMode::Name
                    && r.prints.iter().any(|c| c.name.to_lowercase() == q)
                {
                    r.prints.sort_by_key(|c| c.name.to_lowercase() != q);
                }
                self.selected = best_index(
                    &r.prints,
                    r.exact_id.as_deref(),
                    self.rarity_filter(),
                    self.finish,
                );
                self.prints = r.prints;
                if std::mem::take(&mut self.printings_on_result) {
                    self.show_variants = true;
                }
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// Identifies what's typed, to tell whether the shown result still matches it.
    fn input_key(&self) -> String {
        match self.mode {
            LookupMode::Name => format!("name:{}", self.name.trim().to_lowercase()),
            LookupMode::SetNumber => format!(
                "set:{}:{}",
                parse_set_code(&self.set_code),
                self.number.trim()
            ),
        }
    }

    /// Ctrl+Enter adds only when a card is shown and it's the result for what's typed now.
    fn ctrl_enter_adds(&self) -> bool {
        self.selected.is_some() && self.input_key() == self.searched
    }

    /// The rarity to check results against; set + number lookups don't use one.
    fn rarity_filter(&self) -> RarityFilter {
        match self.mode {
            LookupMode::Name => self.rarity,
            LookupMode::SetNumber => RarityFilter::Any,
        }
    }

    fn selected_card(&self) -> Option<&Card> {
        self.selected.and_then(|i| self.prints.get(i))
    }

    fn reset_after_add(&mut self) {
        // Finish, condition and set code are kept: bulk entry usually repeats them.
        self.name.clear();
        self.number.clear();
        self.qty = 1;
        self.manual_sgd.clear();
        self.treatment_override.clear();
        self.notes.clear();
        self.rarity = RarityFilter::Any;
        self.prints.clear();
        self.selected = None;
        self.show_variants = false;
        self.suggestions.clear();
        self.show_suggestions = false;
        self.error = None;
        self.focus_input = true;
    }
}

/// Picks the most likely printing: same card name as the first result, right finish,
/// right rarity, has a price, plain frame, then earliest in the list (newest).
fn best_index(
    prints: &[Card],
    exact_id: Option<&str>,
    rarity: RarityFilter,
    finish: Finish,
) -> Option<usize> {
    if let Some(id) = exact_id {
        if let Some(i) = prints.iter().position(|c| c.id == id) {
            return Some(i);
        }
    }
    let first_name = &prints.first()?.name;
    prints
        .iter()
        .enumerate()
        .max_by_key(|(i, c)| {
            let score = (c.name == *first_name) as u8 * 16
                + (c.has_finish(finish) as u8) * 8
                + (rarity.matches(&c.rarity) as u8) * 4
                + (price_usd(c, finish).is_some() as u8) * 2
                + c.is_plain() as u8;
            (score, Reverse(*i))
        })
        .map(|(i, _)| i)
}

/// Fires the debounced autocomplete request.
pub fn tick(app: &mut App, ctx: &egui::Context, now: f64) {
    let a = &mut app.add;
    if let Some(due) = a.ac_due {
        if now >= due {
            a.ac_due = None;
            let q = a.name.trim().to_string();
            if q.chars().count() >= 2 {
                a.ac_seq += 1;
                let _ = app.worker.jobs.send(Job::Autocomplete {
                    seq: a.ac_seq,
                    query: q,
                });
            } else {
                a.suggestions.clear();
                a.show_suggestions = false;
            }
        } else {
            ctx.request_repaint_after(Duration::from_secs_f64(due - now));
        }
    }
}

fn start_search(app: &mut App) {
    let a = &mut app.add;
    a.show_suggestions = false;
    a.ac_due = None;
    a.ac_seq += 1; // drop any in-flight autocomplete
    let job = match a.mode {
        LookupMode::Name => {
            let name = a.name.trim().to_string();
            if name.is_empty() {
                return;
            }
            a.search_seq += 1;
            Job::SearchName {
                seq: a.search_seq,
                name,
            }
        }
        LookupMode::SetNumber => {
            let set = parse_set_code(&a.set_code);
            if set.is_empty() || a.number.trim().is_empty() {
                return;
            }
            a.search_seq += 1;
            Job::SearchSetNumber {
                seq: a.search_seq,
                set,
                numbers: printed_number_candidates(&a.number),
            }
        }
    };
    a.searched = a.input_key();
    a.focus_input = true; // keep typing without clicking back into the box
    a.searching = true;
    a.error = None;
    a.prints.clear();
    a.selected = None;
    a.show_variants = false;
    let _ = app.worker.jobs.send(job);
}

pub fn lookup_on_start(app: &mut App, name: &str, show_printings: bool) {
    app.add.name = name.to_string();
    app.add.printings_on_result = show_printings;
    start_search(app);
}

fn add_selected(app: &mut App) {
    let Some(card) = app.add.selected_card().cloned() else {
        return;
    };
    let a = &app.add;
    let manual = match parse_money(&a.manual_sgd) {
        Ok(v) => v,
        Err(()) => {
            app.toast(ToastKind::Error, "The manual price isn't a valid number.");
            return;
        }
    };
    let qty = a.qty.max(1);
    let mut entry = Entry::from_card(&card, a.finish, a.condition, qty);
    entry.manual_sgd = manual;
    entry.treatment_override = a.treatment_override.trim().to_string();
    entry.notes = a.notes.trim().to_string();
    let label = format!(
        "{qty}× {} ({} #{}, {}, {})",
        card.name,
        card.set.to_uppercase(),
        card.collector_number,
        a.finish.label(),
        a.condition.label()
    );
    let (merged, total) = app.add_entry(entry);
    let msg = if merged {
        format!("Added {label}. You now have {total}.")
    } else {
        format!("Added {label}.")
    };
    app.toast(ToastKind::Success, msg);
    app.add.reset_after_add();
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    // Ctrl+Enter adds the shown card. If the input changed since that lookup, the shown
    // card is stale, so look the new input up instead of adding the wrong card.
    let ctrl_enter = ctx.input(|i| i.modifiers.command && i.key_pressed(Key::Enter));
    if ctrl_enter && !app.add.searching {
        if app.add.ctrl_enter_adds() {
            add_selected(app);
        } else {
            start_search(app);
        }
    }

    egui::SidePanel::left("add_form")
        .resizable(false)
        .exact_width(350.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| form(app, ui));
        });
    egui::CentralPanel::default().show(ctx, |ui| results(app, ui));
}

fn form(app: &mut App, ui: &mut Ui) {
    let now = ui.input(|i| i.time);
    let enter = ui.input(|i| i.key_pressed(Key::Enter) && !i.modifiers.command);
    let mut do_search = false;
    let a = &mut app.add;
    if a.qty == 0 {
        a.qty = 1;
    }

    ui.add_space(8.0);
    ui.heading("Add a card");
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.selectable_value(&mut a.mode, LookupMode::Name, "By name");
        ui.selectable_value(&mut a.mode, LookupMode::SetNumber, "By set + number");
    });
    ui.add_space(6.0);

    match a.mode {
        LookupMode::Name => {
            ui.label(RichText::new("Card name").weak());
            let resp = ui.add(
                TextEdit::singleline(&mut a.name)
                    .hint_text("e.g. Sheoldred, the Apocalypse")
                    .font(FontId::proportional(16.0))
                    .desired_width(f32::INFINITY),
            );
            if std::mem::take(&mut a.focus_input) {
                resp.request_focus();
            }
            if resp.changed() {
                a.ac_due = Some(now + 0.3);
                a.suggestion_sel = None;
            }
            if resp.has_focus() && a.show_suggestions && !a.suggestions.is_empty() {
                let n = a.suggestions.len().min(8);
                ui.input(|i| {
                    if i.key_pressed(Key::ArrowDown) {
                        a.suggestion_sel = Some(a.suggestion_sel.map_or(0, |s| (s + 1).min(n - 1)));
                    }
                    if i.key_pressed(Key::ArrowUp) {
                        a.suggestion_sel = a.suggestion_sel.and_then(|s| s.checked_sub(1));
                    }
                });
            }
            if ui.input(|i| i.key_pressed(Key::Escape)) {
                a.show_suggestions = false;
            }
            if resp.lost_focus() && enter {
                if let Some(s) = a.suggestion_sel.and_then(|i| a.suggestions.get(i)) {
                    a.name = s.clone();
                }
                do_search = true;
            }

            if a.show_suggestions && !a.suggestions.is_empty() {
                let mut pick = None;
                egui::Frame::group(ui.style())
                    .inner_margin(4)
                    .show(ui, |ui| {
                        ui.with_layout(Layout::top_down_justified(Align::LEFT), |ui| {
                            for (i, s) in a.suggestions.iter().take(8).enumerate() {
                                if ui
                                    .selectable_label(a.suggestion_sel == Some(i), s)
                                    .clicked()
                                {
                                    pick = Some(s.clone());
                                }
                            }
                        });
                    });
                if let Some(s) = pick {
                    a.name = s;
                    do_search = true;
                }
            }
        }
        LookupMode::SetNumber => {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Set code").weak());
                    let r = ui.add(
                        TextEdit::singleline(&mut a.set_code)
                            .hint_text("sos")
                            .font(FontId::proportional(16.0))
                            .desired_width(110.0),
                    );
                    if r.lost_focus() && enter {
                        do_search = true;
                    }
                });
                ui.vertical(|ui| {
                    ui.label(RichText::new("Number (as printed)").weak());
                    let r = ui.add(
                        TextEdit::singleline(&mut a.number)
                            .hint_text("M 0105")
                            .font(FontId::proportional(16.0))
                            .desired_width(110.0),
                    );
                    if std::mem::take(&mut a.focus_input) {
                        r.request_focus();
                    }
                    if r.lost_focus() && enter {
                        do_search = true;
                    }
                });
            });
            ui.label(
                RichText::new(
                    "Copy the card's bottom-left corner: the number (\"0105\", \"M 0105\" or \"105/281\") \
                     and the set code under it (\"SOS\"). Rarity isn't needed. It comes from the card.",
                )
                .small()
                .weak(),
            );
        }
    }

    ui.add_space(10.0);
    egui::Grid::new("add_opts")
        .num_columns(2)
        .spacing([12.0, 9.0])
        .show(ui, |ui| {
            // Set + number already identifies the exact printing, so rarity is only asked for by name.
            if a.mode == LookupMode::Name {
                ui.label("Rarity");
                egui::ComboBox::from_id_salt("rarity")
                    .selected_text(a.rarity.label())
                    .width(170.0)
                    .show_ui(ui, |ui| {
                        for r in RarityFilter::ALL {
                            ui.selectable_value(&mut a.rarity, r, r.label());
                        }
                    });
                ui.end_row();
            }

            ui.label("Finish");
            ui.horizontal(|ui| {
                for f in Finish::ALL {
                    ui.selectable_value(&mut a.finish, f, f.label());
                }
            });
            ui.end_row();

            ui.label("Condition");
            egui::ComboBox::from_id_salt("condition")
                .selected_text(format!(
                    "{} – {}",
                    a.condition.label(),
                    a.condition.long_label()
                ))
                .width(170.0)
                .show_ui(ui, |ui| {
                    for c in Condition::ALL {
                        ui.selectable_value(
                            &mut a.condition,
                            c,
                            format!("{} – {}", c.label(), c.long_label()),
                        );
                    }
                });
            ui.end_row();

            ui.label("Quantity");
            ui.horizontal(|ui| {
                if ui.button(" − ").clicked() && a.qty > 1 {
                    a.qty -= 1;
                }
                ui.add(egui::DragValue::new(&mut a.qty).range(1..=9999));
                if ui.button(" + ").clicked() {
                    a.qty += 1;
                }
            });
            ui.end_row();

            ui.label("Manual price");
            ui.horizontal(|ui| {
                ui.label("S$");
                ui.add(
                    TextEdit::singleline(&mut a.manual_sgd)
                        .hint_text("auto (Scryfall)")
                        .desired_width(120.0),
                );
            })
            .response
            .on_hover_text("Leave empty to use the Scryfall price converted to SGD.");
            ui.end_row();

            ui.label("Treatment");
            ui.add(
                TextEdit::singleline(&mut a.treatment_override)
                    .hint_text("auto-detected")
                    .desired_width(170.0),
            )
            .on_hover_text(
                "Only needed if the auto-detected treatment (Full Art, Borderless, …) is wrong.",
            );
            ui.end_row();

            ui.label("Notes");
            ui.add(
                TextEdit::singleline(&mut a.notes)
                    .hint_text("binder, box, …")
                    .desired_width(170.0),
            );
            ui.end_row();
        });

    ui.add_space(12.0);
    let can_search = match a.mode {
        LookupMode::Name => !a.name.trim().is_empty(),
        LookupMode::SetNumber => !a.set_code.trim().is_empty() && !a.number.trim().is_empty(),
    };
    ui.add_enabled_ui(can_search && !a.searching, |ui| {
        ui.with_layout(Layout::top_down_justified(Align::Center), |ui| {
            if widgets::primary_button(ui, "🔍  Look up").clicked() {
                do_search = true;
            }
        });
    });
    ui.add_space(10.0);
    ui.label(
        RichText::new(
            "Fast entry: type → Enter (look up) → check the card → Ctrl+Enter (add) → type the next one. \
             The cursor stays in the box. Up/Down picks a name suggestion.",
        )
            .small()
            .weak(),
    );

    if do_search && can_search {
        start_search(app);
    }
}

fn results(app: &mut App, ui: &mut Ui) {
    let rate = app.rate();
    let mut do_add = false;
    {
        let App {
            add: a,
            images,
            collection,
            ..
        } = app;

        if a.searching {
            ui.centered_and_justified(|ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Searching Scryfall…");
                });
            });
            return;
        }
        if let Some(err) = &a.error {
            ui.add_space(12.0);
            widgets::warning(ui, err);
        }
        let Some(sel) = a.selected else {
            if a.error.is_none() {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new("Type a card name (or set + number) and press Enter.\nThe best match shows up here so you can check it before adding.")
                            .size(16.0)
                            .weak(),
                    );
                });
            }
            return;
        };
        let card = a.prints[sel].clone();

        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_space(12.0);
                ui.horizontal_top(|ui| {
                    let h = 470.0;
                    widgets::card_image(
                        ui,
                        images,
                        &card.image("normal").unwrap_or_default(),
                        egui::vec2(h * CARD_ASPECT, h),
                    );
                    ui.add_space(20.0);
                    ui.vertical(|ui| {
                        ui.set_max_width(520.0);
                        details(ui, a, &card, collection, rate, &mut do_add);
                    });
                });

                if a.show_variants {
                    variants(ui, a, images, rate);
                }
                ui.add_space(20.0);
            });
    }
    if do_add {
        add_selected(app);
    }
}

fn details(
    ui: &mut Ui,
    a: &mut AddState,
    card: &Card,
    collection: &[Entry],
    rate: f64,
    do_add: &mut bool,
) {
    ui.label(bold(&card.name).size(26.0));
    ui.label(RichText::new(card.type_line_full()).weak());
    ui.add_space(10.0);

    let treat = treatment(card);
    egui::Grid::new("card_details")
        .num_columns(2)
        .spacing([16.0, 8.0])
        .show(ui, |ui| {
            ui.label(RichText::new("Set").weak());
            ui.label(format!("{} ({})", card.set_name, card.set.to_uppercase()));
            ui.end_row();
            ui.label(RichText::new("Number").weak());
            ui.label(format!("#{}", card.collector_number));
            ui.end_row();
            ui.label(RichText::new("Rarity").weak());
            widgets::rarity_chip(ui, &card.rarity);
            ui.end_row();
            ui.label(RichText::new("Treatment").weak());
            ui.horizontal_wrapped(|ui| {
                for t in treat.split(", ") {
                    let (fg, bg) = if t == "Normal" {
                        (ui.visuals().text_color(), ui.visuals().faint_bg_color)
                    } else {
                        (ACCENT, ACCENT.gamma_multiply(0.18))
                    };
                    widgets::chip(ui, t, fg, bg);
                }
            });
            ui.end_row();
            ui.label(RichText::new("Finishes").weak());
            let finishes: Vec<&str> = Finish::ALL
                .iter()
                .filter(|f| card.has_finish(**f))
                .map(|f| f.label())
                .collect();
            ui.label(finishes.join(", "));
            ui.end_row();
            ui.label(RichText::new("Released").weak());
            ui.label(&card.released_at);
            ui.end_row();
        });

    ui.add_space(8.0);
    let usd = price_usd(card, a.finish);
    if !a.rarity_filter().matches(&card.rarity) {
        widgets::warning(
            ui,
            &format!(
                "This printing is {}, but you entered {}. Check the other printings.",
                rarity_label(&card.rarity),
                a.rarity_filter().label()
            ),
        );
    }
    if !card.has_finish(a.finish) {
        widgets::warning(
            ui,
            &format!("This printing doesn't come in {}.", a.finish.label()),
        );
    } else if usd.is_none() {
        widgets::warning(
            ui,
            &format!(
                "Scryfall has no {} price for this printing. Set a manual price if you know it.",
                a.finish.label()
            ),
        );
    }

    let owned: Vec<&Entry> = collection
        .iter()
        .filter(|e| e.scryfall_id == card.id)
        .collect();
    if !owned.is_empty() {
        let parts: Vec<String> = owned
            .iter()
            .map(|e| format!("{}× {} {}", e.qty, e.finish.label(), e.condition.label()))
            .collect();
        ui.label(
            RichText::new(format!("Already in your collection: {}", parts.join(", ")))
                .color(ACCENT),
        );
    }

    ui.add_space(8.0);
    let manual = parse_money(&a.manual_sgd).ok().flatten();
    let unit = manual.or(usd.map(|u| u * rate));
    egui::Frame::default()
        .fill(ui.visuals().faint_bg_color)
        .corner_radius(10)
        .inner_margin(14)
        .show(ui, |ui| {
            ui.set_min_width(300.0);
            ui.label(
                RichText::new(format!(
                    "Unit price · {} · {}",
                    a.finish.label(),
                    a.condition.label()
                ))
                .weak(),
            );
            ui.label(
                bold(
                    unit.map(|u| format!("S$ {}", money(u)))
                        .unwrap_or_else(|| "—".into()),
                )
                .size(32.0)
                .color(ACCENT),
            );
            if let Some(u) = usd {
                ui.label(
                    RichText::new(format!(
                        "US$ {u:.2} × {rate:.4}  (TCGplayer market via Scryfall)"
                    ))
                    .small()
                    .weak(),
                );
            }
            if manual.is_some() {
                ui.label(
                    RichText::new("Using your manual price")
                        .small()
                        .color(ACCENT),
                );
            }
            if a.qty > 1 {
                if let Some(u) = unit {
                    ui.label(format!("× {} = S$ {}", a.qty, money(u * a.qty as f64)));
                }
            }
        });

    ui.add_space(12.0);
    ui.horizontal_wrapped(|ui| {
        let label = if a.qty > 1 {
            format!("✔  Add {} to collection", a.qty)
        } else {
            "✔  Add to collection".to_string()
        };
        if widgets::primary_button(ui, &label)
            .on_hover_text("Shortcut: Ctrl+Enter, then type the next card straight away")
            .clicked()
        {
            *do_add = true;
        }
        ui.label(RichText::new("Ctrl+Enter").weak());
        let others = a.prints.len().saturating_sub(1);
        let toggle = if a.show_variants {
            "Hide printings".to_string()
        } else {
            format!("Not this one? Show printings ({others})")
        };
        if ui
            .add_enabled(
                others > 0,
                egui::Button::new(toggle).min_size(egui::vec2(0.0, 34.0)),
            )
            .clicked()
        {
            a.show_variants = !a.show_variants;
            a.only_finish = true;
            a.only_rarity = a.rarity_filter() != RarityFilter::Any;
        }
    });
    ui.horizontal(|ui| {
        if !card.scryfall_uri.is_empty() && ui.button("🔗 Scryfall").clicked() {
            let _ = open::that_detached(&card.scryfall_uri);
        }
        if let Some(t) = card.tcgplayer_uri() {
            if ui.button("🔗 TCGplayer").clicked() {
                let _ = open::that_detached(t);
            }
        }
    });
}

fn variants(ui: &mut Ui, a: &mut AddState, images: &mut Images, rate: f64) {
    ui.add_space(18.0);
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(bold(format!("All printings ({})", a.prints.len())).size(18.0));
        ui.add_space(12.0);
        ui.checkbox(&mut a.only_finish, format!("Only {}", a.finish.label()));
        if a.rarity_filter() != RarityFilter::Any {
            let label = format!("Only {}", a.rarity_filter().label());
            ui.checkbox(&mut a.only_rarity, label);
        }
    });
    ui.label(
        RichText::new("Click a printing to select it.")
            .small()
            .weak(),
    );
    ui.add_space(6.0);

    let idxs: Vec<usize> = (0..a.prints.len())
        .filter(|&i| {
            let c = &a.prints[i];
            (!a.only_finish || c.has_finish(a.finish))
                && (!a.only_rarity || a.rarity_filter().matches(&c.rarity))
        })
        .collect();
    if idxs.is_empty() {
        ui.label(RichText::new("No printings match these filters.").weak());
        return;
    }

    let gap = 12.0;
    let cols = ((ui.available_width() + gap) / (TILE_W + gap))
        .floor()
        .max(1.0) as usize;
    let mut new_sel = None;
    for chunk in idxs.chunks(cols) {
        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;
            for &i in chunk {
                if variant_tile(
                    ui,
                    images,
                    &a.prints[i],
                    a.finish,
                    rate,
                    Some(i) == a.selected,
                )
                .clicked()
                {
                    new_sel = Some(i);
                }
            }
        });
        ui.add_space(gap);
    }
    if let Some(i) = new_sel {
        a.selected = Some(i);
        ui.scroll_to_cursor(Some(Align::TOP));
    }
}

const TILE_W: f32 = 158.0;

fn variant_tile(
    ui: &mut Ui,
    images: &mut Images,
    card: &Card,
    finish: Finish,
    rate: f64,
    selected: bool,
) -> Response {
    let pad = 7.0;
    let img_w = TILE_W - pad * 2.0;
    let img_h = img_w / CARD_ASPECT;
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(TILE_W, img_h + pad * 2.0 + 74.0), Sense::click());
    if ui.is_rect_visible(rect) {
        let v = ui.visuals().clone();
        let bg = if selected {
            ACCENT.gamma_multiply(0.22)
        } else if resp.hovered() {
            v.widgets.hovered.weak_bg_fill
        } else {
            v.faint_bg_color
        };
        ui.painter().rect_filled(rect, 10.0, bg);
        if selected {
            ui.painter().rect_stroke(
                rect,
                10.0,
                Stroke::new(2.0_f32, ACCENT),
                egui::StrokeKind::Inside,
            );
        }
        let img_rect =
            Rect::from_min_size(rect.min + egui::vec2(pad, pad), egui::vec2(img_w, img_h));
        widgets::paint_card_image(
            ui,
            images,
            &card.image("small").unwrap_or_default(),
            img_rect,
        );

        let x = rect.left() + pad;
        let mut y = img_rect.bottom() + 6.0;
        let painter = ui.painter();
        painter.text(
            egui::pos2(x, y),
            Align2::LEFT_TOP,
            format!("{} #{}", card.set.to_uppercase(), card.collector_number),
            FontId::proportional(13.5),
            v.strong_text_color(),
        );
        y += 18.0;
        painter.text(
            egui::pos2(x, y),
            Align2::LEFT_TOP,
            rarity_label(&card.rarity),
            FontId::proportional(12.5),
            widgets::rarity_color(&card.rarity),
        );
        y += 17.0;
        painter.text(
            egui::pos2(x, y),
            Align2::LEFT_TOP,
            truncate(&treatment(card), 24),
            FontId::proportional(12.0),
            v.weak_text_color(),
        );
        y += 17.0;
        let price = if !card.has_finish(finish) {
            format!("no {}", finish.label().to_lowercase())
        } else {
            match price_usd(card, finish) {
                Some(u) => format!("S$ {}", money(u * rate)),
                None => "no price".into(),
            }
        };
        painter.text(
            egui::pos2(x, y),
            Align2::LEFT_TOP,
            price,
            FontId::proportional(13.0),
            ACCENT,
        );
    }
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!(
            "{}\n{} ({}) #{}\n{}\nReleased {}",
            card.name,
            card.set_name,
            card.set.to_uppercase(),
            card.collector_number,
            treatment(card),
            card.released_at
        ))
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        s.chars().take(max - 1).collect::<String>() + "…"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scryfall::tests::{card, BORDERLESS, DFC};

    #[test]
    fn best_match_prefers_finish_rarity_and_plain_frame() {
        let mut plain = card(BORDERLESS);
        plain.id = "plain".into();
        plain.border_color = "black".into();
        plain.frame_effects = None;
        let fancy = card(BORDERLESS); // newer (index 0), borderless
        let prints = vec![fancy, plain];
        // Same finish/rarity: the plain-frame print wins over the newer borderless one.
        assert_eq!(
            best_index(&prints, None, RarityFilter::Mythic, Finish::Foil),
            Some(1)
        );
        // Exact id always wins.
        assert_eq!(
            best_index(&prints, Some("b1"), RarityFilter::Any, Finish::Foil),
            Some(0)
        );
    }

    #[test]
    fn ctrl_enter_only_adds_the_card_for_the_current_input() {
        let mut a = AddState::new();
        a.mode = LookupMode::SetNumber;
        a.set_code = "SOS".into();
        a.number = "0105".into();
        a.searched = a.input_key();
        assert!(!a.ctrl_enter_adds(), "nothing shown yet");

        a.prints = vec![card(BORDERLESS)];
        a.selected = Some(0);
        assert!(a.ctrl_enter_adds());

        // Typed the next number without looking it up: don't add the stale card.
        a.number = "0106".into();
        assert!(!a.ctrl_enter_adds());

        // After adding, the box is cleared and nothing is shown, but the set code stays.
        a.number = "0105".into();
        a.reset_after_add();
        assert!(!a.ctrl_enter_adds());
        assert_eq!(a.set_code, "SOS");
        assert!(a.number.is_empty());
        assert!(a.focus_input);
    }

    #[test]
    fn best_match_respects_finish() {
        let dfc = card(DFC);
        let mut etched_only = card(DFC);
        etched_only.id = "etched".into();
        etched_only.finishes = vec!["etched".into()];
        let prints = vec![dfc, etched_only];
        assert_eq!(
            best_index(&prints, None, RarityFilter::Any, Finish::Etched),
            Some(1)
        );
    }
}
