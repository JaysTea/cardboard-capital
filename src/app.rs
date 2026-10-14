use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

use egui::TextureHandle;

use crate::account::{self, Account};
use crate::model::{self, Entry, Game, Settings, Value};
use crate::scryfall;
use crate::storage::{self, Boot, Paths};
use crate::ui::add::AddState;
use crate::ui::collection::CollectionState;
use crate::ui::import::ImportState;
use crate::ui::update::UpdateState;
use crate::ui::widgets::{self, Toast, ToastKind};
use crate::worker::{self, Job, Reply, Worker};
use crate::{export, ui};

/// What fills the window.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    /// Game tiles, the value chart, Settings, Update and the account button.
    Home,
    /// Settings opened from Home.
    Settings,
    /// One game's tabs.
    Game(Game),
}

/// Tabs inside a game.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Add,
    Import,
    Collection,
    Settings,
}

pub enum ImgState {
    Loading,
    Ready(TextureHandle),
    Failed,
}

/// Texture cache keyed by image URL; misses are queued to the image download threads.
pub struct Images {
    map: HashMap<String, ImgState>,
    tx: Sender<String>,
}

impl Images {
    /// Returns the texture if loaded; otherwise starts loading it.
    pub fn get(&mut self, url: &str) -> Option<TextureHandle> {
        if url.is_empty() {
            return None;
        }
        match self.map.get(url) {
            Some(ImgState::Ready(t)) => Some(t.clone()),
            Some(_) => None,
            None => {
                self.map.insert(url.to_string(), ImgState::Loading);
                let _ = self.tx.send(url.to_string());
                None
            }
        }
    }

    pub fn failed(&self, url: &str) -> bool {
        url.is_empty() || matches!(self.map.get(url), Some(ImgState::Failed))
    }

    fn loaded(
        &mut self,
        ctx: &egui::Context,
        url: String,
        result: Result<egui::ColorImage, String>,
    ) {
        let state = match result {
            Ok(img) => ImgState::Ready(ctx.load_texture(&url, img, egui::TextureOptions::LINEAR)),
            Err(_) => ImgState::Failed,
        };
        self.map.insert(url, state);
    }
}

pub struct App {
    /// Where the data, exports, cache and account key live.
    pub paths: Paths,
    pub account: Account,
    /// The MTG collection.
    pub collection: Vec<Entry>,
    pub settings: Settings,
    /// Set when collection.json exists but couldn't be read: nothing is saved over it.
    pub collection_locked: Option<String>,
    /// Same for settings.json.
    settings_locked: Option<String>,
    /// The last collection save failed. It's retried, and closing asks first.
    pub unsaved_error: Option<String>,
    pub worker: Worker,
    pub images: Images,
    pub screen: Screen,
    pub tab: Tab,
    pub account_open: bool,
    pub add: AddState,
    pub coll: CollectionState,
    pub import: ImportState,
    pub update: UpdateState,
    pub toasts: Vec<Toast>,
    pub fx_loading: bool,
    /// Currency being switched to while its rate is fetched.
    pub pending_currency: Option<String>,
    pub refresh_loading: bool,
    next_uid: u64,
    /// Time of the first unsaved edit (edits are debounced before saving).
    dirty_since: Option<f64>,
    applied_dark: Option<bool>,
    /// "Couldn't save" window shown instead of closing.
    close_prompt: bool,
    allow_close: bool,
    pub logo: TextureHandle,
    /// Held until the app exits (see `lock_instance` in main.rs).
    _instance_lock: Option<std::fs::File>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, boot: Boot, instance_lock: Option<std::fs::File>) -> Self {
        widgets::install_fonts(&cc.egui_ctx);

        let Boot { paths, moved_from, notices } = boot;
        let collection = storage::load_collection(&paths.mtg_dir);
        let loaded_settings = storage::load_settings(&paths.settings_dir);
        let mut settings = loaded_settings.value;
        // Old builds could point the export at the exe folder; the default now follows the data.
        let export_fixed = !paths.legacy && storage::fix_export_settings(&mut settings, &paths.exe_dir);
        let worker = worker::spawn(cc.egui_ctx.clone(), paths.cache.clone());
        let images = Images {
            map: HashMap::new(),
            tx: worker.images.clone(),
        };
        let next_uid = collection.value.iter().map(|e| e.uid).max().unwrap_or(0) + 1;
        let logo = cc.egui_ctx.load_texture(
            "logo",
            egui::ColorImage::from_rgba_unmultiplied([64, 64], &crate::icon::rgba(64)),
            egui::TextureOptions::LINEAR,
        );
        let account = account::load(&paths.account);

        let mut app = App {
            paths,
            account,
            collection: collection.value,
            settings,
            collection_locked: collection.locked,
            settings_locked: loaded_settings.locked,
            unsaved_error: None,
            worker,
            images,
            screen: Screen::Home,
            tab: Tab::Add,
            account_open: false,
            add: AddState::new(),
            coll: CollectionState::default(),
            import: ImportState::default(),
            update: UpdateState::default(),
            toasts: Vec::new(),
            fx_loading: false,
            pending_currency: None,
            refresh_loading: false,
            next_uid,
            dirty_since: None,
            applied_dark: None,
            close_prompt: false,
            allow_close: false,
            logo,
            _instance_lock: instance_lock,
        };
        for msg in notices
            .into_iter()
            .chain(collection.notice)
            .chain(loaded_settings.notice)
            .chain(app.collection_locked.clone())
            .chain(app.settings_locked.clone())
        {
            app.toast(ToastKind::Error, msg);
        }
        if !moved_from.is_empty() {
            let from: Vec<String> = moved_from.iter().map(|p| p.display().to_string()).collect();
            app.toast(
                ToastKind::Success,
                format!(
                    "Your collection and settings now live in {}. The old files in {} were kept as *.moved.",
                    app.paths.root.display(),
                    from.join(", ")
                ),
            );
        }
        if export_fixed {
            app.save_settings();
        }
        // Put the spreadsheet where it now belongs.
        if (!moved_from.is_empty() || export_fixed) && app.settings.auto_export && !app.collection.is_empty() {
            app.export(false);
        }
        if let Some(p) = app.account.problem.clone() {
            app.toast(ToastKind::Info, p);
        }
        app.fetch_fx();
        app.update.auto_install = std::env::args().any(|a| a == "--install-update-now");
        ui::update::check(&mut app, false);

        // `"Cardboard Capital.exe" "Card Name" [--printings]` starts with that lookup;
        // `--collection` opens on the Collection tab. All of them skip Home.
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.iter().any(|a| a == "--collection") {
            app.screen = Screen::Game(Game::Mtg);
            app.tab = Tab::Collection;
        }
        // `--import <deck link | path to a text file>` opens Import and reads it.
        if let Some(i) = args.iter().position(|a| a == "--import") {
            if let Some(src) = args.get(i + 1) {
                app.screen = Screen::Game(Game::Mtg);
                ui::import::import_on_start(&mut app, src);
            }
            return app;
        }
        // The first argument that isn't a flag or a flag's value is a card name to look up.
        let positional = args
            .iter()
            .enumerate()
            .find(|(i, a)| !a.starts_with("--") && (*i == 0 || args[i - 1] != "--pretend-version"))
            .map(|(_, a)| a);
        if let Some(name) = positional {
            app.screen = Screen::Game(Game::Mtg);
            app.tab = Tab::Add;
            ui::add::lookup_on_start(&mut app, name, args.iter().any(|a| a == "--printings"));
        }
        app
    }

    pub fn rate(&self) -> f64 {
        self.settings.rate()
    }

    /// Card count and value of one game's collection.
    pub fn game_total(&self, game: Game) -> Value {
        match game {
            Game::Mtg => model::collection_value(&self.collection, self.rate()),
        }
    }

    /// Where the spreadsheet is written: the chosen folder, else a pre-1.2 custom path,
    /// else `exports\Cardboard Capital.xlsx` in the data folder.
    pub fn export_path(&self) -> PathBuf {
        let s = &self.settings;
        if !s.export_dir.trim().is_empty() {
            PathBuf::from(s.export_dir.trim()).join(storage::EXPORT_FILE)
        } else if !s.export_path.trim().is_empty() {
            PathBuf::from(s.export_path.trim())
        } else {
            self.paths.default_export_path()
        }
    }

    /// Saves a one-off copy of the spreadsheet somewhere else (doesn't change the export location).
    pub fn export_as(&mut self) {
        let Some(mut path) = rfd::FileDialog::new()
            .add_filter("Excel workbook", &["xlsx"])
            .set_file_name(storage::EXPORT_FILE)
            .save_file()
        else {
            return;
        };
        if path.extension().is_none() {
            path.set_extension("xlsx");
        }
        match export::export(&path, &self.collection, &self.settings) {
            Ok(()) => self.toast(ToastKind::Success, format!("Saved a copy to {}", path.display())),
            Err(e) => self.toast(ToastKind::Error, e),
        }
    }

    /// Sets where the spreadsheet is saved (None = the default exports folder) and re-exports.
    pub fn set_export_dir(&mut self, dir: Option<PathBuf>) {
        self.settings.export_dir = dir.map(|d| d.display().to_string()).unwrap_or_default();
        self.settings.export_path.clear();
        self.settings.export_prompted = true;
        self.save_settings();
        if self.settings.auto_export {
            self.export(true);
        }
    }

    pub fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.toasts.push(Toast::new(kind, text.into()));
    }

    pub fn fetch_fx(&mut self) {
        self.fx_loading = true;
        let currency = self.settings.currency.clone();
        let _ = self.worker.jobs.send(Job::FetchFx { currency });
    }

    /// Starts switching to another currency. The switch happens once its rate arrives,
    /// so manual prices can be converted at real rates.
    pub fn change_currency(&mut self, code: &str) {
        if code == self.settings.currency {
            self.pending_currency = None;
            return;
        }
        // Manual prices are stored in the settings' currency: both files must be saved together.
        if let Some(why) = self.collection_locked.as_ref().or(self.settings_locked.as_ref()) {
            let msg = format!("Can't change the currency right now: {why}");
            self.toast(ToastKind::Error, msg);
            return;
        }
        self.pending_currency = Some(code.to_string());
        self.fx_loading = true;
        let _ = self.worker.jobs.send(Job::FetchFx {
            currency: code.to_string(),
        });
    }

    fn on_fx(&mut self, result: Result<crate::fx::FxQuote, String>) {
        self.fx_loading = false;
        let switching = self.pending_currency.take();
        match result {
            Ok(q) if switching.as_deref() == Some(q.currency.as_str())
                && self.collection_locked.is_none()
                && self.settings_locked.is_none() =>
            {
                let old = self.settings.currency();
                let old_rate = self.rate();
                let factor = if old_rate > 0.0 { q.rate / old_rate } else { 1.0 };
                let mut converted = 0;
                for e in &mut self.collection {
                    if let Some(m) = e.manual_price.as_mut() {
                        *m = (*m * factor * 100.0).round() / 100.0;
                        converted += 1;
                    }
                }
                self.settings.highlight_value = (self.settings.highlight_value * factor).round();
                self.settings.currency = q.currency.clone();
                self.settings.fx_manual = false;
                self.settings.fx_manual_rate = q.rate;
                self.apply_quote(q);
                self.coll.invalidate_buffers();
                self.save_now();
                let new = self.settings.currency();
                let mut msg = format!("Prices are now shown in {} ({}).", new.name, new.code);
                if converted > 0 {
                    msg += &format!(" Converted {converted} manual prices from {}.", old.code);
                }
                self.toast(ToastKind::Success, msg);
            }
            Ok(q) if q.currency == self.settings.currency => self.apply_quote(q),
            Ok(_) => {} // a reply for a currency we're no longer using
            Err(e) => {
                let msg = match switching {
                    Some(code) => format!("Couldn't switch to {code}: no exchange rate available. {e}"),
                    None => format!(
                        "Couldn't fetch the live USD→{} rate, using the last known one. {e}",
                        self.settings.currency
                    ),
                };
                self.toast(ToastKind::Error, msg);
            }
        }
    }

    fn apply_quote(&mut self, q: crate::fx::FxQuote) {
        self.settings.fx_rate = q.rate;
        self.settings.fx_updated = model::now_string();
        self.settings.fx_source = if q.date.is_empty() {
            q.source
        } else {
            format!("{} ({})", q.source, q.date)
        };
        self.save_settings();
    }

    pub fn refresh_prices(&mut self) {
        let mut ids: Vec<String> = self
            .collection
            .iter()
            .map(|e| e.scryfall_id.clone())
            .collect();
        ids.sort();
        ids.dedup();
        if ids.is_empty() {
            self.toast(ToastKind::Info, "Nothing to refresh yet.");
            return;
        }
        self.refresh_loading = true;
        let _ = self.worker.jobs.send(Job::RefreshPrices { ids });
    }

    pub fn alloc_uid(&mut self) -> u64 {
        let uid = self.next_uid;
        self.next_uid += 1;
        uid
    }

    /// Adds (or merges) an entry, then saves and exports right away.
    pub fn add_entry(&mut self, mut entry: Entry) -> (bool, u32) {
        entry.uid = self.alloc_uid();
        let result = model::add_or_merge(&mut self.collection, entry);
        self.coll.invalidate_buffers();
        self.save_now();
        result
    }

    /// Adds many entries (merging duplicates) and saves once. Returns (new rows, merged rows).
    pub fn add_entries(&mut self, entries: Vec<Entry>) -> (usize, usize) {
        let (mut new_rows, mut merged) = (0, 0);
        for mut entry in entries {
            entry.uid = self.alloc_uid();
            if model::add_or_merge(&mut self.collection, entry).0 {
                merged += 1;
            } else {
                new_rows += 1;
            }
        }
        self.coll.invalidate_buffers();
        self.save_now();
        (new_rows, merged)
    }

    pub fn remove_entry(&mut self, uid: u64) {
        self.collection.retain(|e| e.uid != uid);
        self.coll.invalidate_buffers();
        self.save_now();
    }

    /// For edits that may come in quick succession (typing in a cell).
    pub fn mark_dirty(&mut self, now: f64) {
        self.dirty_since.get_or_insert(now);
    }

    pub fn save_now(&mut self) {
        self.dirty_since = None;
        let result = match &self.collection_locked {
            Some(why) => Err(format!("Not saved: {why}")),
            None => storage::save_collection(&self.paths.mtg_dir, &self.collection),
        };
        if let Err(e) = result {
            self.unsaved_error = Some(e.clone());
            self.toast(ToastKind::Error, e);
            return;
        }
        self.unsaved_error = None;
        if self.settings.auto_export {
            self.export(false);
        }
    }

    /// Settings that couldn't be read at start-up aren't overwritten (the toast said so then).
    pub fn save_settings(&mut self) {
        if self.settings_locked.is_some() {
            return;
        }
        if let Err(e) = storage::save_settings(&self.paths.settings_dir, &self.settings) {
            self.toast(ToastKind::Error, e);
        }
    }

    pub fn export(&mut self, announce: bool) {
        let path = self.export_path();
        match export::export(&path, &self.collection, &self.settings) {
            Ok(()) if announce => self.toast(
                ToastKind::Success,
                format!("Exported to {}", path.display()),
            ),
            Ok(()) => {}
            Err(e) => self.toast(ToastKind::Error, e),
        }
    }

    pub fn open_export(&mut self) {
        let path = self.export_path();
        if !path.exists() {
            self.export(false);
        }
        if let Err(e) = open::that_detached(&path) {
            self.toast(
                ToastKind::Error,
                format!("Couldn't open {}: {e}", path.display()),
            );
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(reply) = self.worker.replies.try_recv() {
            match reply {
                Reply::Autocomplete { seq, result } => self.add.on_autocomplete(seq, result),
                Reply::Search { seq, result } => self.add.on_search(seq, result),
                Reply::Import { seq, result } => self.import.on_result(seq, result),
                Reply::Update { manual, result } => ui::update::on_checked(self, manual, result),
                Reply::Installed(result) => ui::update::on_installed(self, ctx, result),
                Reply::Image { url, result } => self.images.loaded(ctx, url, result),
                Reply::Fx(result) => self.on_fx(result),
                Reply::Prices(result) => {
                    self.refresh_loading = false;
                    match result {
                        Ok(cards) => {
                            let by_id: HashMap<&str, &scryfall::Card> =
                                cards.iter().map(|c| (c.id.as_str(), c)).collect();
                            let mut updated = 0;
                            for e in &mut self.collection {
                                if let Some(card) = by_id.get(e.scryfall_id.as_str()) {
                                    e.update_from_card(card);
                                    updated += 1;
                                }
                            }
                            self.coll.invalidate_buffers();
                            self.save_now();
                            self.toast(
                                ToastKind::Success,
                                format!(
                                    "Refreshed prices for {updated} of {} entries.",
                                    self.collection.len()
                                ),
                            );
                        }
                        Err(e) => {
                            self.toast(ToastKind::Error, format!("Price refresh failed: {e}"))
                        }
                    }
                }
            }
        }
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        model::set_display_currency(&self.settings.currency);
        if self.applied_dark != Some(self.settings.dark_mode) {
            widgets::apply_theme(ctx, self.settings.dark_mode);
            self.applied_dark = Some(self.settings.dark_mode);
        }

        self.poll(ctx);
        let now = ctx.input(|i| i.time);
        ui::add::tick(self, ctx, now);

        // Debounced save of inline edits.
        if let Some(since) = self.dirty_since {
            if now - since > 0.8 {
                self.save_now();
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(300));
            }
        }
        // A failed save is tried again every 10 s.
        if self.unsaved_error.is_some() && self.dirty_since.is_none() && self.collection_locked.is_none() {
            self.dirty_since = Some(now + 9.2);
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            if self.dirty_since.is_some() {
                self.save_now();
            }
            if self.unsaved_error.is_some() && !self.allow_close {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.close_prompt = true;
            }
        }

        match self.screen {
            Screen::Home => ui::home::show(self, ctx),
            Screen::Settings => {
                ui::home::top_bar(self, ctx);
                ui::settings::show(self, ctx);
            }
            Screen::Game(_) => {
                ui::header(self, ctx);
                ui::update::bar(self, ctx);
                match self.tab {
                    Tab::Add => ui::add::show(self, ctx),
                    Tab::Import => ui::import::show(self, ctx),
                    Tab::Collection => ui::collection::show(self, ctx),
                    Tab::Settings => ui::settings::show(self, ctx),
                }
                ui::settings::export_prompt(self, ctx);
            }
        }
        ui::update::window(self, ctx);
        self.unsaved_window(ctx);
        let toast_lift = match self.screen {
            Screen::Home => ui::home::BAR_HEIGHT,
            Screen::Game(_) if self.update.available.is_some() => ui::update::BAR_HEIGHT,
            _ => 0.0,
        };
        widgets::show_toasts(ctx, &mut self.toasts, now, toast_lift);
    }
}

impl App {
    /// Shown instead of closing while the latest changes aren't saved.
    fn unsaved_window(&mut self, ctx: &egui::Context) {
        if !self.close_prompt {
            return;
        }
        let Some(err) = self.unsaved_error.clone() else {
            self.close_prompt = false;
            return;
        };
        let (mut retry, mut open_folder, mut quit) = (false, false, false);
        egui::Window::new("Your latest changes aren't saved")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.set_max_width(460.0);
                ui.label(egui::RichText::new(err).small());
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    retry = widgets::primary_button(ui, "Try again").clicked();
                    open_folder = ui.button("📁 Open folder").clicked();
                    quit = ui.button("Quit without saving").clicked();
                });
            });
        if retry {
            self.save_now();
            if self.unsaved_error.is_none() {
                self.close_prompt = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if open_folder {
            let _ = open::that_detached(&self.paths.mtg_dir);
        }
        if quit {
            self.allow_close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}
