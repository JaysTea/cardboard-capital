//! "Collection" tab: filterable, sortable, editable table of everything added.

use std::collections::{BTreeMap, HashMap};

use egui::{Align, Align2, Layout, RichText, Sense, TextEdit, Ui};
use egui_extras::{Column, TableBuilder};

use crate::app::{App, Images};
use crate::model::{money, parse_money, Condition, Entry, Finish, RarityFilter};
use crate::ui::widgets::{self, bold, ToastKind, ACCENT, CARD_ASPECT};

#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    /// Newest first.
    #[default]
    Added,
    Name,
    Set,
    Number,
    Rarity,
    Finish,
    Treatment,
    Condition,
    Qty,
    Usd,
    Unit,
    Total,
}

#[derive(Default)]
pub struct CollectionState {
    pub search: String,
    pub set_filter: String,
    pub rarity: RarityFilter,
    pub finish: Option<Finish>,
    pub manual_only: bool,
    pub unchecked_only: bool,
    pub sort: SortKey,
    pub desc: bool,
    /// Text being typed into the "Manual S$" cells, keyed by entry uid.
    manual_bufs: HashMap<u64, String>,
    pending_delete: Option<u64>,
}

impl CollectionState {
    /// Call when entries change outside the table so edit buffers don't show stale values.
    pub fn invalidate_buffers(&mut self) {
        self.manual_bufs.clear();
    }

    fn filters_active(&self) -> bool {
        !self.search.is_empty()
            || !self.set_filter.is_empty()
            || self.rarity != RarityFilter::Any
            || self.finish.is_some()
            || self.manual_only
            || self.unchecked_only
    }
}

fn rarity_rank(r: &str) -> u8 {
    match r {
        "common" => 0,
        "uncommon" => 1,
        "rare" => 2,
        "mythic" => 3,
        _ => 4,
    }
}

/// Indexes into the collection that pass the filters, in display order.
fn visible_rows(entries: &[Entry], c: &CollectionState, rate: f64) -> Vec<usize> {
    let q = c.search.trim().to_lowercase();
    let mut rows: Vec<usize> = (0..entries.len())
        .filter(|&i| {
            let e = &entries[i];
            (q.is_empty()
                || e.name.to_lowercase().contains(&q)
                || e.set_name.to_lowercase().contains(&q)
                || e.set_code.to_lowercase() == q
                || e.notes.to_lowercase().contains(&q)
                || e.treatment_display().to_lowercase().contains(&q))
                && (c.set_filter.is_empty() || e.set_code == c.set_filter)
                && c.rarity.matches(&e.rarity)
                && c.finish.is_none_or(|f| e.finish == f)
                && (!c.manual_only || e.manual_sgd.is_some())
                && (!c.unchecked_only || !e.checked)
        })
        .collect();

    let f = |o: Option<f64>| o.unwrap_or(-1.0);
    rows.sort_by(|&a, &b| {
        let (x, y) = (&entries[a], &entries[b]);
        let ord = match c.sort {
            SortKey::Added => y.uid.cmp(&x.uid),
            SortKey::Name => x.name.to_lowercase().cmp(&y.name.to_lowercase()),
            SortKey::Set => x.set_name.cmp(&y.set_name),
            SortKey::Number => {
                natural_num(&x.collector_number).cmp(&natural_num(&y.collector_number))
            }
            SortKey::Rarity => rarity_rank(&x.rarity).cmp(&rarity_rank(&y.rarity)),
            SortKey::Finish => x.finish.label().cmp(y.finish.label()),
            SortKey::Treatment => x.treatment_display().cmp(y.treatment_display()),
            SortKey::Condition => (x.condition as u8).cmp(&(y.condition as u8)),
            SortKey::Qty => x.qty.cmp(&y.qty),
            SortKey::Usd => f(x.usd).total_cmp(&f(y.usd)),
            SortKey::Unit => f(x.unit_sgd(rate)).total_cmp(&f(y.unit_sgd(rate))),
            SortKey::Total => f(x.total_sgd(rate)).total_cmp(&f(y.total_sgd(rate))),
        }
        .then_with(|| x.name.cmp(&y.name));
        if c.desc {
            ord.reverse()
        } else {
            ord
        }
    });
    rows
}

/// "107a" -> (107, "107a") so collector numbers sort numerically.
fn natural_num(s: &str) -> (u32, String) {
    let digits: String = s.chars().take_while(|c| c.is_ascii_digit()).collect();
    (digits.parse().unwrap_or(u32::MAX), s.to_string())
}

#[derive(Default)]
struct Actions {
    refresh: bool,
    export: bool,
    open_xlsx: bool,
    open_folder: bool,
    dirty: bool,
}

pub fn show(app: &mut App, ctx: &egui::Context) {
    let rate = app.rate();
    let now = ctx.input(|i| i.time);
    let mut act = Actions::default();

    egui::TopBottomPanel::top("coll_toolbar")
        .frame(
            egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(14, 8)),
        )
        .show(ctx, |ui| toolbar(app, ui, &mut act));

    let rows = visible_rows(&app.collection, &app.coll, rate);

    egui::TopBottomPanel::bottom("coll_footer")
        .frame(
            egui::Frame::side_top_panel(&ctx.style()).inner_margin(egui::Margin::symmetric(14, 7)),
        )
        .show(ctx, |ui| {
            let sum = |idx: &mut dyn Iterator<Item = &Entry>| -> (u32, f64, usize) {
                let mut q = 0;
                let mut s = 0.0;
                let mut missing = 0;
                for e in idx {
                    q += e.qty;
                    match e.total_sgd(rate) {
                        Some(t) => s += t,
                        None => missing += 1,
                    }
                }
                (q, s, missing)
            };
            let (fq, fs, fmissing) = sum(&mut rows.iter().map(|&i| &app.collection[i]));
            let (tq, ts, _) = sum(&mut app.collection.iter());
            ui.horizontal(|ui| {
                ui.label(format!(
                    "Showing {} of {} entries · {} cards · ",
                    rows.len(),
                    app.collection.len(),
                    fq
                ));
                ui.label(bold(format!("S$ {}", money(fs))).color(ACCENT));
                if fmissing > 0 {
                    ui.label(RichText::new(format!("· {fmissing} without a price")).weak());
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.label(bold(format!(
                        "Collection total: {tq} cards · S$ {}",
                        money(ts)
                    )));
                });
            });
        });

    egui::CentralPanel::default().show(ctx, |ui| {
        if app.collection.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("Your collection is empty. Add cards from the “Add card” tab.")
                        .size(16.0)
                        .weak(),
                );
            });
            return;
        }
        table(app, ui, &rows, rate, &mut act);
    });

    delete_dialog(app, ctx);

    if act.dirty {
        app.mark_dirty(now);
    }
    if act.refresh {
        app.refresh_prices();
    }
    if act.export {
        app.export(true);
    }
    if act.open_xlsx {
        app.open_export();
    }
    if act.open_folder {
        let path = app.export_path();
        let folder = path.parent().map(|p| p.to_path_buf()).unwrap_or(app.app_dir.clone());
        let _ = std::fs::create_dir_all(&folder);
        let _ = open::that_detached(&folder);
    }
}

fn toolbar(app: &mut App, ui: &mut Ui, act: &mut Actions) {
    let App {
        collection,
        coll: c,
        refresh_loading,
        ..
    } = app;

    ui.horizontal(|ui| {
        ui.label(bold("Collection").size(19.0));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            act.open_folder |= ui
                .button("📁 Folder")
                .on_hover_text("Open the folder the spreadsheet is saved in")
                .clicked();
            act.open_xlsx |= ui.button("📄 Open sheet").clicked();
            act.export |= widgets::primary_button(ui, "⬇  Export XLSX").clicked();
            if *refresh_loading {
                ui.spinner();
            }
            act.refresh |= ui
                .add_enabled(!*refresh_loading, egui::Button::new("🔄 Refresh prices"))
                .on_hover_text("Re-fetch every card's price and details from Scryfall")
                .clicked();
        });
    });
    ui.add_space(2.0);
    ui.horizontal_wrapped(|ui| {
        ui.add(
            TextEdit::singleline(&mut c.search)
                .hint_text("🔍 Search name, set, treatment, notes…")
                .desired_width(250.0),
        );

        let mut sets: BTreeMap<String, String> = BTreeMap::new();
        for e in collection.iter() {
            sets.entry(e.set_name.clone())
                .or_insert_with(|| e.set_code.clone());
        }
        let set_label = if c.set_filter.is_empty() {
            "All sets".to_string()
        } else {
            c.set_filter.clone()
        };
        egui::ComboBox::from_id_salt("f_set")
            .selected_text(set_label)
            .width(150.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut c.set_filter, String::new(), "All sets");
                for (name, code) in &sets {
                    ui.selectable_value(
                        &mut c.set_filter,
                        code.clone(),
                        format!("{name} ({code})"),
                    );
                }
            });

        egui::ComboBox::from_id_salt("f_rarity")
            .selected_text(if c.rarity == RarityFilter::Any {
                "All rarities"
            } else {
                c.rarity.label()
            })
            .width(120.0)
            .show_ui(ui, |ui| {
                for r in RarityFilter::ALL {
                    let label = if r == RarityFilter::Any {
                        "All rarities"
                    } else {
                        r.label()
                    };
                    ui.selectable_value(&mut c.rarity, r, label);
                }
            });

        egui::ComboBox::from_id_salt("f_finish")
            .selected_text(c.finish.map_or("All finishes", |f| f.label()))
            .width(110.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut c.finish, None, "All finishes");
                for f in Finish::ALL {
                    ui.selectable_value(&mut c.finish, Some(f), f.label());
                }
            });

        ui.checkbox(&mut c.manual_only, "Manual price only");
        ui.checkbox(&mut c.unchecked_only, "Not yet checked");
        if c.filters_active() && ui.button("✖ Clear filters").clicked() {
            c.search.clear();
            c.set_filter.clear();
            c.rarity = RarityFilter::Any;
            c.finish = None;
            c.manual_only = false;
            c.unchecked_only = false;
        }
    });
}

fn sort_header(ui: &mut Ui, c: &mut CollectionState, key: SortKey, label: &str) {
    let arrow = if c.sort == key {
        if c.desc {
            " ▼"
        } else {
            " ▲"
        }
    } else {
        ""
    };
    let r = ui.add(egui::Button::new(bold(format!("{label}{arrow}"))).frame(false));
    if r.clicked() {
        if c.sort == key {
            c.desc = !c.desc;
        } else {
            c.sort = key;
            c.desc = matches!(
                key,
                SortKey::Qty | SortKey::Usd | SortKey::Unit | SortKey::Total
            );
        }
    }
}

fn hover_card(ui: &mut Ui, images: &mut Images, e: &Entry, rate: f64) {
    let w = 250.0;
    widgets::card_image(ui, images, &e.image_normal, egui::vec2(w, w / CARD_ASPECT));
    ui.label(bold(&e.name));
    ui.label(format!(
        "{} ({}) #{}",
        e.set_name, e.set_code, e.collector_number
    ));
    ui.label(format!(
        "{} · {} · {}",
        e.treatment_display(),
        e.finish.label(),
        e.condition.long_label()
    ));
    if let Some(u) = e.unit_sgd(rate) {
        ui.label(format!("S$ {} each ({})", money(u), e.price_source()));
    }
}

fn table(app: &mut App, ui: &mut Ui, rows: &[usize], rate: f64, act: &mut Actions) {
    let App {
        collection,
        coll: c,
        images,
        ..
    } = app;
    let mut delete = None;

    let header_cols: [(SortKey, &str); 13] = [
        (SortKey::Name, "Name"),
        (SortKey::Set, "Set"),
        (SortKey::Number, "#"),
        (SortKey::Rarity, "Rarity"),
        (SortKey::Finish, "Finish"),
        (SortKey::Treatment, "Treatment"),
        (SortKey::Condition, "Cond."),
        (SortKey::Qty, "Qty"),
        (SortKey::Usd, "US$"),
        (SortKey::Unit, "Manual S$"),
        (SortKey::Unit, "Unit S$"),
        (SortKey::Total, "Total S$"),
        (SortKey::Added, "Notes"),
    ];

    TableBuilder::new(ui)
        .id_salt("collection_table")
        .striped(true)
        .resizable(true)
        .cell_layout(Layout::left_to_right(Align::Center))
        .column(Column::exact(26.0))
        .column(Column::initial(230.0).at_least(120.0).clip(true))
        .column(Column::initial(170.0).at_least(60.0).clip(true))
        .column(Column::initial(52.0).clip(true))
        .column(Column::initial(84.0))
        .column(Column::initial(72.0))
        .column(Column::initial(140.0).clip(true))
        .column(Column::initial(66.0))
        .column(Column::initial(66.0))
        .column(Column::initial(70.0))
        .column(Column::initial(86.0))
        .column(Column::initial(80.0))
        .column(Column::initial(90.0))
        .column(Column::remainder().at_least(110.0).clip(true))
        .column(Column::exact(64.0))
        .header(30.0, |mut header| {
            header.col(|ui| {
                ui.label(bold("✔")).on_hover_text("Cross-checked");
            });
            for (key, label) in header_cols {
                header.col(|ui| {
                    if label == "Manual S$" || label == "Notes" {
                        ui.label(bold(label));
                    } else {
                        sort_header(ui, c, key, label);
                    }
                });
            }
            header.col(|_| {});
        })
        .body(|body| {
            body.rows(32.0, rows.len(), |mut row| {
                let e = &mut collection[rows[row.index()]];

                row.col(|ui| {
                    act.dirty |= ui
                        .checkbox(&mut e.checked, "")
                        .on_hover_text("Mark as cross-checked")
                        .changed();
                });
                row.col(|ui| {
                    let r = ui
                        .add(
                            egui::Label::new(bold(&e.name))
                                .truncate()
                                .sense(Sense::click()),
                        )
                        .on_hover_ui(|ui| hover_card(ui, images, e, rate));
                    if r.double_clicked() && !e.scryfall_uri.is_empty() {
                        let _ = open::that_detached(&e.scryfall_uri);
                    }
                    r.context_menu(|ui| {
                        ui.label(RichText::new("Treatment override").weak());
                        act.dirty |= ui
                            .add(
                                TextEdit::singleline(&mut e.treatment_override)
                                    .hint_text(e.treatment.as_str()),
                            )
                            .changed();
                        ui.separator();
                        if ui.button("🔗 Open on Scryfall").clicked() {
                            let _ = open::that_detached(&e.scryfall_uri);
                            ui.close_menu();
                        }
                        if !e.tcgplayer_uri.is_empty()
                            && ui.button("🔗 Open on TCGplayer").clicked()
                        {
                            let _ = open::that_detached(&e.tcgplayer_uri);
                            ui.close_menu();
                        }
                        if ui.button("🗑 Remove…").clicked() {
                            delete = Some(e.uid);
                            ui.close_menu();
                        }
                    });
                });
                row.col(|ui| {
                    ui.add(egui::Label::new(format!("{} · {}", e.set_code, e.set_name)).truncate())
                        .on_hover_text(&e.set_name);
                });
                row.col(|ui| {
                    ui.label(&e.collector_number);
                });
                row.col(|ui| {
                    widgets::rarity_chip(ui, &e.rarity);
                });
                row.col(|ui| {
                    let t = RichText::new(e.finish.label());
                    ui.label(match e.finish {
                        Finish::NonFoil => t,
                        _ => t.color(egui::Color32::from_rgb(176, 124, 216)).strong(),
                    });
                });
                row.col(|ui| {
                    let t = e.treatment_display();
                    let text = if t == "Normal" {
                        RichText::new(t).weak()
                    } else {
                        RichText::new(t)
                    };
                    ui.add(egui::Label::new(text).truncate()).on_hover_text(t);
                });
                row.col(|ui| {
                    egui::ComboBox::from_id_salt(("cond", e.uid))
                        .selected_text(e.condition.label())
                        .width(50.0)
                        .show_ui(ui, |ui| {
                            for cond in Condition::ALL {
                                act.dirty |= ui
                                    .selectable_value(
                                        &mut e.condition,
                                        cond,
                                        format!("{} – {}", cond.label(), cond.long_label()),
                                    )
                                    .changed();
                            }
                        });
                });
                row.col(|ui| {
                    act.dirty |= ui
                        .add(egui::DragValue::new(&mut e.qty).range(1..=9999))
                        .changed();
                });
                row.col(|ui| match e.usd {
                    Some(u) => {
                        ui.label(format!("${u:.2}"));
                    }
                    None => {
                        ui.label(RichText::new("—").weak())
                            .on_hover_text("No Scryfall price for this finish");
                    }
                });
                row.col(|ui| {
                    let buf = c.manual_bufs.entry(e.uid).or_insert_with(|| {
                        e.manual_sgd.map(|v| format!("{v:.2}")).unwrap_or_default()
                    });
                    let r = ui.add(TextEdit::singleline(buf).hint_text("—").desired_width(70.0));
                    if r.lost_focus() {
                        match parse_money(buf) {
                            Ok(v) => {
                                if v != e.manual_sgd {
                                    e.manual_sgd = v;
                                    act.dirty = true;
                                }
                                *buf = v.map(|v| format!("{v:.2}")).unwrap_or_default();
                            }
                            Err(()) => {
                                *buf = e.manual_sgd.map(|v| format!("{v:.2}")).unwrap_or_default()
                            }
                        }
                    }
                    r.on_hover_text(
                        "Type a price in S$ to override Scryfall. Clear it to go back to Scryfall.",
                    );
                });
                row.col(|ui| {
                    let text = e.unit_sgd(rate).map(money).unwrap_or_else(|| "—".into());
                    let rt = if e.manual_sgd.is_some() {
                        RichText::new(text).color(ACCENT)
                    } else {
                        RichText::new(text)
                    };
                    ui.label(rt).on_hover_text(e.price_source());
                });
                row.col(|ui| {
                    let text = e.total_sgd(rate).map(money).unwrap_or_else(|| "—".into());
                    ui.label(bold(text));
                });
                row.col(|ui| {
                    act.dirty |= ui
                        .add(TextEdit::singleline(&mut e.notes).desired_width(f32::INFINITY))
                        .changed();
                });
                row.col(|ui| {
                    if ui
                        .small_button("🔗")
                        .on_hover_text("Open on Scryfall")
                        .clicked()
                    {
                        let _ = open::that_detached(&e.scryfall_uri);
                    }
                    if ui.small_button("🗑").on_hover_text("Remove").clicked() {
                        delete = Some(e.uid);
                    }
                });
            });
        });

    if delete.is_some() {
        c.pending_delete = delete;
    }
}

fn delete_dialog(app: &mut App, ctx: &egui::Context) {
    let Some(uid) = app.coll.pending_delete else {
        return;
    };
    let Some(e) = app.collection.iter().find(|e| e.uid == uid) else {
        app.coll.pending_delete = None;
        return;
    };
    let label = format!(
        "{}× {} ({} #{}, {}, {})",
        e.qty,
        e.name,
        e.set_code,
        e.collector_number,
        e.finish.label(),
        e.condition.label()
    );
    let mut confirm = false;
    let mut cancel = false;
    egui::Window::new("Remove from collection?")
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(&label);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if widgets::primary_button(ui, "Remove").clicked() {
                    confirm = true;
                }
                if ui.button("Cancel").clicked() {
                    cancel = true;
                }
            });
        });
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        cancel = true;
    }
    if confirm {
        app.coll.pending_delete = None;
        app.remove_entry(uid);
        app.toast(ToastKind::Info, format!("Removed {label}."));
    } else if cancel {
        app.coll.pending_delete = None;
    }
}
