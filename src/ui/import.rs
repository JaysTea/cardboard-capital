//! "Import" tab: read a deck link or a pasted list, review the matches, add them all.

use egui::{Align, Layout, RichText, Sense, TextEdit, Ui};
use egui_extras::{Column, TableBuilder};

use crate::app::{App, Tab};
use crate::import::{ImportOptions, ImportResult, ImportSource};
use crate::model::{money, Condition, Entry, Finish};
use crate::scryfall::price_usd;
use crate::ui::widgets::{self, bold, ToastKind, ACCENT, CARD_ASPECT};
use crate::worker::Job;

#[derive(Clone, Copy, PartialEq, Eq, Default)]
enum Kind {
    #[default]
    Link,
    Text,
    Sheet,
}

#[derive(Default)]
pub struct ImportState {
    kind: Kind,
    url: String,
    text: String,
    sheet_path: Option<std::path::PathBuf>,
    condition: Condition,
    default_finish: Finish,
    include_side: bool,
    seq: u64,
    loading: bool,
    error: Option<String>,
    result: Option<ImportResult>,
}

impl ImportState {
    pub fn on_result(&mut self, seq: u64, result: Result<ImportResult, String>) {
        if seq != self.seq {
            return;
        }
        self.loading = false;
        match result {
            Ok(r) => {
                self.result = Some(r);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn can_start(&self) -> bool {
        !self.loading
            && match self.kind {
                Kind::Link => !self.url.trim().is_empty(),
                Kind::Text => !self.text.trim().is_empty(),
                Kind::Sheet => self.sheet_path.is_some(),
            }
    }
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    let rate = app.rate();
    let mut start = false;
    let mut add = false;
    let mut lookup: Option<String> = None;

    egui::SidePanel::left("import_form")
        .resizable(false)
        .exact_width(380.0)
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| form(&mut app.import, ui, &mut start));
        });
    egui::CentralPanel::default().show(ctx, |ui| results(app, ui, rate, &mut add, &mut lookup));

    if start {
        start_import(app);
    }
    if add {
        add_rows(app);
    }
    if let Some(name) = lookup {
        app.tab = Tab::Add;
        crate::ui::add::lookup_on_start(app, &name, false);
    }
}

fn form(s: &mut ImportState, ui: &mut Ui, start: &mut bool) {
    ui.add_space(8.0);
    ui.heading("Import cards");
    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(&mut s.kind, Kind::Link, "🔗 From a link");
        ui.selectable_value(&mut s.kind, Kind::Text, "📝 From text");
        ui.selectable_value(&mut s.kind, Kind::Sheet, "📄 From a spreadsheet");
    });
    ui.add_space(6.0);

    match s.kind {
        Kind::Link => {
            ui.label(RichText::new("Deck link").weak());
            let r = ui.add(
                TextEdit::singleline(&mut s.url)
                    .hint_text("https://archidekt.com/decks/…")
                    .desired_width(f32::INFINITY),
            );
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) && s.can_start() {
                *start = true;
            }
            ui.label(
                RichText::new(
                    "Archidekt links work directly (the deck must be public). Moxfield blocks apps from reading \
                     decks: on Moxfield use Export → \"Copy for Moxfield\" and paste it under From text.",
                )
                .small()
                .weak(),
            );
        }
        Kind::Text => {
            ui.label(RichText::new("Card list").weak());
            ui.add(
                TextEdit::multiline(&mut s.text)
                    .hint_text("19 Swamp (UND) 91\n1 Sol Ring (CMM) 400 *F*\n4 Lightning Bolt")
                    .font(egui::TextStyle::Monospace)
                    .desired_rows(16)
                    .desired_width(f32::INFINITY),
            );
            ui.label(
                RichText::new(
                    "One card per line: quantity, name, (SET) and number. *F* = foil, *E* = etched. \
                     Exports from Moxfield, Archidekt and MTG Arena all work. The set and number are optional, \
                     but without them you get Scryfall's default printing.",
                )
                .small()
                .weak(),
            );
        }
        Kind::Sheet => {
            ui.label(RichText::new("Spreadsheet from another install").weak());
            ui.horizontal(|ui| {
                if ui.button("📂 Choose file…").clicked() {
                    if let Some(p) = rfd::FileDialog::new()
                        .add_filter("Spreadsheet", &["xlsx", "xlsm", "xls", "ods"])
                        .pick_file()
                    {
                        s.sheet_path = Some(p);
                    }
                }
                match &s.sheet_path {
                    Some(p) => ui.add(egui::Label::new(
                        RichText::new(p.file_name().unwrap_or_default().to_string_lossy()).strong(),
                    ).truncate()).on_hover_text(p.display().to_string()),
                    None => ui.label(RichText::new("No file chosen").weak()),
                };
            });
            ui.label(
                RichText::new(
                    "Pick a spreadsheet exported by Cardboard Capital (or MTG Stock Sheet), e.g. from a friend or \
                     another PC. Quantity, finish, condition, manual prices, notes and checked marks come across; \
                     prices are refreshed from Scryfall. Importing the same sheet twice adds the quantities twice.",
                )
                .small()
                .weak(),
            );
        }
    }

    ui.add_space(10.0);
    egui::Grid::new("import_opts")
        .num_columns(2)
        .spacing([12.0, 9.0])
        .show(ui, |ui| {
            let sheet = s.kind == Kind::Sheet;
            ui.label(if sheet { "Condition (if missing)" } else { "Condition" })
                .on_hover_text(if sheet { "Spreadsheet rows keep their own condition; this is only used for rows without one." } else { "Applies to every imported card." });
            egui::ComboBox::from_id_salt("import_cond")
                .selected_text(format!(
                    "{} – {}",
                    s.condition.label(),
                    s.condition.long_label()
                ))
                .width(180.0)
                .show_ui(ui, |ui| {
                    for c in Condition::ALL {
                        ui.selectable_value(
                            &mut s.condition,
                            c,
                            format!("{} – {}", c.label(), c.long_label()),
                        );
                    }
                });
            ui.end_row();

            if !sheet {
                ui.label("Unmarked finish")
                    .on_hover_text("Used for lines that don't say foil or etched.");
                ui.horizontal(|ui| {
                    for f in Finish::ALL {
                        ui.selectable_value(&mut s.default_finish, f, f.label());
                    }
                });
                ui.end_row();
            }
        });
    if s.kind != Kind::Sheet {
        ui.checkbox(&mut s.include_side, "Include sideboard and maybeboard");
    }

    ui.add_space(12.0);
    ui.add_enabled_ui(s.can_start(), |ui| {
        ui.with_layout(Layout::top_down_justified(Align::Center), |ui| {
            if widgets::primary_button(ui, "📥  Read cards").clicked() {
                *start = true;
            }
        });
    });
    ui.label(
        RichText::new("Nothing is added until you review the list and click Add.")
            .small()
            .weak(),
    );
}

pub fn import_on_start(app: &mut App, src: &str) {
    app.tab = Tab::Import;
    let s = &mut app.import;
    let lower = src.to_lowercase();
    if src.starts_with("http") || src.contains(".com/") {
        s.kind = Kind::Link;
        s.url = src.to_string();
    } else if [".xlsx", ".xlsm", ".xls", ".ods"].iter().any(|e| lower.ends_with(e)) {
        s.kind = Kind::Sheet;
        s.sheet_path = Some(std::path::PathBuf::from(src));
    } else {
        s.kind = Kind::Text;
        s.text = std::fs::read_to_string(src).unwrap_or_default();
    }
    start_import(app);
}

fn start_import(app: &mut App) {
    let s = &mut app.import;
    if !s.can_start() {
        return;
    }
    s.seq += 1;
    s.loading = true;
    s.error = None;
    s.result = None;
    let source = match s.kind {
        Kind::Link => ImportSource::Link(s.url.trim().to_string()),
        Kind::Text => ImportSource::Text(s.text.clone()),
        Kind::Sheet => match &s.sheet_path {
            Some(p) => ImportSource::Sheet(p.clone()),
            None => return,
        },
    };
    let opts = ImportOptions {
        default_finish: s.default_finish,
        include_side: s.include_side,
    };
    let _ = app.worker.jobs.send(Job::Import {
        seq: s.seq,
        source,
        opts,
    });
}

/// The collection entry for an import row, carrying over details from a spreadsheet import.
fn entry_for(card: &crate::scryfall::Card, row: &crate::import::ImportRow, default_condition: Condition) -> Entry {
    let extras = row.line.extras.as_ref();
    let condition = extras.and_then(|x| x.condition).unwrap_or(default_condition);
    let mut e = Entry::from_card(card, row.finish, condition, row.qty);
    if let Some(x) = extras {
        e.manual_sgd = x.manual_sgd;
        e.notes = x.notes.clone();
        e.checked = x.checked;
        // The sheet shows the override if there was one; keep it only if it differs from auto-detection.
        if !x.treatment.is_empty() && x.treatment != e.treatment {
            e.treatment_override = x.treatment.clone();
        }
    }
    e
}

fn add_rows(app: &mut App) {
    let Some(mut result) = app.import.result.take() else {
        return;
    };
    let condition = app.import.condition;
    let mut entries = Vec::new();
    let mut cards = 0;
    result.rows.retain(|row| {
        let take = row.include && row.card.is_some();
        if let (true, Some(card)) = (take, &row.card) {
            entries.push(entry_for(card, row, condition));
            cards += row.qty;
        }
        !take
    });
    if entries.is_empty() {
        app.import.result = Some(result);
        return;
    }
    let (new_rows, merged) = app.add_entries(entries);
    let merged_text = if merged > 0 {
        format!(", {merged} merged into rows you already had")
    } else {
        String::new()
    };
    app.toast(
        ToastKind::Success,
        format!(
            "Imported {cards} cards from {}: {new_rows} new rows{merged_text}.",
            result.source
        ),
    );
    if result.rows.is_empty() {
        app.import.url.clear();
        app.import.text.clear();
        app.import.sheet_path = None;
    } else {
        // Leave the rows that weren't imported so they can be fixed or looked up.
        app.import.result = Some(result);
    }
}

fn results(app: &mut App, ui: &mut Ui, rate: f64, add: &mut bool, lookup: &mut Option<String>) {
    let App {
        import: s, images, ..
    } = app;

    if s.loading {
        ui.centered_and_justified(|ui| {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Reading cards and matching them on Scryfall…");
            });
        });
        return;
    }
    if let Some(err) = &s.error {
        ui.add_space(12.0);
        widgets::warning(ui, err);
    }
    let Some(result) = s.result.as_mut() else {
        if s.error.is_none() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("Paste a deck link or a card list on the left, then click Read cards.\nYou'll get to check every card before anything is added.")
                        .size(16.0)
                        .weak(),
                );
            });
        }
        return;
    };

    let selected: Vec<&crate::import::ImportRow> = result
        .rows
        .iter()
        .filter(|r| r.include && r.card.is_some())
        .collect();
    let sel_cards: u32 = selected.iter().map(|r| r.qty).sum();
    let sel_value: f64 = selected
        .iter()
        .filter_map(|r| price_usd(r.card.as_ref()?, r.finish).map(|u| u * rate * r.qty as f64))
        .sum();
    let problems = result.rows.iter().filter(|r| r.note.is_some()).count();
    let unmatched = result.rows.iter().filter(|r| r.card.is_none()).count();

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(bold(&result.source).size(19.0));
        ui.label(RichText::new(format!("· {} lines", result.rows.len())).weak());
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let label = format!("✔  Add {sel_cards} cards to collection");
            if ui
                .add_enabled(
                    sel_cards > 0,
                    egui::Button::new(
                        bold(label)
                            .color(egui::Color32::from_rgb(24, 20, 12))
                            .size(15.5),
                    )
                    .fill(ACCENT)
                    .min_size(egui::vec2(0.0, 34.0)),
                )
                .clicked()
            {
                *add = true;
            }
            if ui.button("Select none").clicked() {
                result.rows.iter_mut().for_each(|r| r.include = false);
            }
            if ui.button("Select all").clicked() {
                result
                    .rows
                    .iter_mut()
                    .for_each(|r| r.include = r.card.is_some());
            }
        });
    });
    ui.horizontal(|ui| {
        ui.label(format!("Selected: {sel_cards} cards · "));
        ui.label(bold(format!("S$ {}", money(sel_value))).color(ACCENT));
        if problems > 0 {
            ui.label(
                RichText::new(format!(
                    "   ⚠ {problems} rows worth checking ({unmatched} not found)"
                ))
                .color(egui::Color32::from_rgb(235, 170, 40)),
            );
        }
    });
    ui.add_space(4.0);

    TableBuilder::new(ui)
        .id_salt("import_table")
        .striped(true)
        .resizable(true)
        .cell_layout(Layout::left_to_right(Align::Center))
        .column(Column::exact(26.0))
        .column(Column::exact(64.0))
        .column(Column::initial(190.0).at_least(110.0).clip(true))
        .column(Column::initial(120.0).at_least(50.0).clip(true))
        .column(Column::initial(58.0).clip(true))
        .column(Column::initial(90.0))
        .column(Column::initial(66.0))
        .column(Column::initial(74.0))
        .column(Column::remainder().at_least(220.0).clip(true))
        .header(28.0, |mut h| {
            for t in [
                "", "Qty", "Name", "Set", "#", "Finish", "Unit S$", "Total S$", "Check",
            ] {
                h.col(|ui| {
                    ui.label(bold(t));
                });
            }
        })
        .body(|body| {
            body.rows(30.0, result.rows.len(), |mut row| {
                let idx = row.index();
                let r = &mut result.rows[idx];
                let card = r.card.clone();
                row.col(|ui| {
                    ui.add_enabled(card.is_some(), egui::Checkbox::without_text(&mut r.include));
                });
                row.col(|ui| {
                    ui.add(egui::DragValue::new(&mut r.qty).range(1..=9999));
                });
                row.col(|ui| {
                    let name = card
                        .as_ref()
                        .map_or(r.line.name.clone(), |c| c.name.clone());
                    let resp = ui.add(
                        egui::Label::new(bold(&name))
                            .truncate()
                            .sense(Sense::hover()),
                    );
                    if let Some(c) = &card {
                        resp.on_hover_ui(|ui| {
                            let w = 240.0;
                            widgets::card_image(
                                ui,
                                images,
                                &c.image("normal").unwrap_or_default(),
                                egui::vec2(w, w / CARD_ASPECT),
                            );
                            ui.label(format!(
                                "{} ({}) #{}",
                                c.set_name,
                                c.set.to_uppercase(),
                                c.collector_number
                            ));
                            ui.label(crate::scryfall::treatment(c));
                        });
                    } else if !r.line.raw.is_empty() {
                        resp.on_hover_text(&r.line.raw);
                    }
                });
                row.col(|ui| {
                    let text = card.as_ref().map_or_else(
                        || r.line.set.clone().unwrap_or_default().to_uppercase(),
                        |c| format!("{} · {}", c.set.to_uppercase(), c.set_name),
                    );
                    ui.add(egui::Label::new(text).truncate());
                });
                row.col(|ui| {
                    let n = card.as_ref().map_or_else(
                        || r.line.number.clone().unwrap_or_default(),
                        |c| c.collector_number.clone(),
                    );
                    ui.label(n);
                });
                row.col(|ui| match &card {
                    Some(c) => {
                        egui::ComboBox::from_id_salt(("imp_finish", idx))
                            .selected_text(r.finish.label())
                            .width(76.0)
                            .show_ui(ui, |ui| {
                                for f in Finish::ALL.into_iter().filter(|f| c.has_finish(*f)) {
                                    ui.selectable_value(&mut r.finish, f, f.label());
                                }
                            });
                    }
                    None => {
                        ui.label(RichText::new("—").weak());
                    }
                });
                let unit = card
                    .as_ref()
                    .and_then(|c| price_usd(c, r.finish))
                    .map(|u| u * rate);
                row.col(|ui| {
                    ui.label(unit.map(money).unwrap_or_else(|| "—".into()));
                });
                row.col(|ui| {
                    ui.label(bold(
                        unit.map(|u| money(u * r.qty as f64))
                            .unwrap_or_else(|| "—".into()),
                    ));
                });
                row.col(|ui| {
                    if r.line.board != "Main deck" {
                        ui.label(RichText::new(&r.line.board).weak());
                    }
                    if card.is_none() || r.note.is_some() {
                        if ui
                            .small_button("🔍 Look up")
                            .on_hover_text("Find this card on the Add card tab")
                            .clicked()
                        {
                            *lookup = Some(r.line.name.clone());
                        }
                    }
                    match &r.note {
                        Some(note) => {
                            let color = if card.is_none() {
                                egui::Color32::from_rgb(230, 80, 70)
                            } else {
                                egui::Color32::from_rgb(235, 170, 40)
                            };
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!("⚠ {}", note.short)).color(color),
                                )
                                .truncate(),
                            )
                            .on_hover_text(&note.detail);
                        }
                        None => {
                            ui.label(
                                RichText::new("✔").color(egui::Color32::from_rgb(70, 190, 110)),
                            );
                        }
                    }
                });
            });
        });
}
