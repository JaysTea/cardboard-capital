use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::Sender;

use egui::TextureHandle;

use crate::model::{self, Entry, Settings};
use crate::scryfall;
use crate::storage;
use crate::ui::add::AddState;
use crate::ui::collection::CollectionState;
use crate::ui::import::ImportState;
use crate::ui::update::UpdateState;
use crate::ui::widgets::{self, Toast, ToastKind};
use crate::worker::{self, Job, Reply, Worker};
use crate::{export, ui};

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
    /// Folder holding the exe.
    pub app_dir: PathBuf,
    /// `<app_dir>/data`: collection, settings, image cache.
    pub data_dir: PathBuf,
    pub collection: Vec<Entry>,
    pub settings: Settings,
    pub worker: Worker,
    pub images: Images,
    pub tab: Tab,
    pub add: AddState,
    pub coll: CollectionState,
    pub import: ImportState,
    pub update: UpdateState,
    pub toasts: Vec<Toast>,
    pub fx_loading: bool,
    pub refresh_loading: bool,
    next_uid: u64,
    /// Time of the first unsaved edit (edits are debounced before saving).
    dirty_since: Option<f64>,
    applied_dark: Option<bool>,
    pub logo: TextureHandle,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        widgets::install_fonts(&cc.egui_ctx);

        let app_dir = storage::app_dir();
        let moved = storage::prepare_dirs(&app_dir);
        let data_dir = storage::data_dir(&app_dir);
        crate::update::cleanup(&data_dir);
        let (collection, load_error) = storage::load_collection(&data_dir);
        let mut settings = storage::load_settings(&data_dir);
        // Old builds could point the export at the exe folder; that now lives in exports.
        let export_parent = std::path::Path::new(&settings.export_path).parent().map(|p| p.to_path_buf());
        if export_parent.as_deref() == Some(app_dir.as_path()) {
            settings.export_path.clear();
        }
        let worker = worker::spawn(cc.egui_ctx.clone(), storage::cache_dir(&data_dir));
        let images = Images {
            map: HashMap::new(),
            tx: worker.images.clone(),
        };
        let next_uid = collection.iter().map(|e| e.uid).max().unwrap_or(0) + 1;
        let logo = cc.egui_ctx.load_texture(
            "logo",
            egui::ColorImage::from_rgba_unmultiplied([64, 64], &crate::icon::rgba(64)),
            egui::TextureOptions::LINEAR,
        );

        let mut app = App {
            app_dir,
            data_dir,
            collection,
            settings,
            worker,
            images,
            tab: Tab::Add,
            add: AddState::new(),
            coll: CollectionState::default(),
            import: ImportState::default(),
            update: UpdateState::default(),
            toasts: Vec::new(),
            fx_loading: false,
            refresh_loading: false,
            next_uid,
            dirty_since: None,
            applied_dark: None,
            logo,
        };
        if let Some(e) = load_error {
            app.toast(ToastKind::Error, e);
        }
        if !moved.is_empty() {
            app.toast(
                ToastKind::Info,
                format!("Tidied the app folder: moved {}.", moved.join(", ")),
            );
        }
        app.fetch_fx();
        app.update.auto_install = std::env::args().any(|a| a == "--install-update-now");
        ui::update::check(&mut app, false);

        // `"Cardboard Capital.exe" "Card Name" [--printings]` starts with that lookup;
        // `--collection` opens on the Collection tab.
        let args: Vec<String> = std::env::args().skip(1).collect();
        if args.iter().any(|a| a == "--collection") {
            app.tab = Tab::Collection;
        }
        // `--import <deck link | path to a text file>` opens Import and reads it.
        if let Some(i) = args.iter().position(|a| a == "--import") {
            if let Some(src) = args.get(i + 1) {
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
            ui::add::lookup_on_start(&mut app, name, args.iter().any(|a| a == "--printings"));
        }
        app
    }

    pub fn rate(&self) -> f64 {
        self.settings.rate()
    }

    /// Where the spreadsheet is written: the chosen folder, else a pre-1.2 custom path,
    /// else `exportsCardboard Capital.xlsx`.
    pub fn export_path(&self) -> PathBuf {
        let s = &self.settings;
        if !s.export_dir.trim().is_empty() {
            PathBuf::from(s.export_dir.trim()).join(storage::EXPORT_FILE)
        } else if !s.export_path.trim().is_empty() {
            PathBuf::from(s.export_path.trim())
        } else {
            storage::default_export_path(&self.app_dir)
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

    /// Sets where the spreadsheet is saved (None = the app's exports folder) and re-exports.
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
        let _ = self.worker.jobs.send(Job::FetchFx);
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
        if let Err(e) = storage::save_collection(&self.data_dir, &self.collection) {
            self.toast(ToastKind::Error, e);
            return;
        }
        if self.settings.auto_export {
            self.export(false);
        }
    }

    pub fn save_settings(&mut self) {
        if let Err(e) = storage::save_settings(&self.data_dir, &self.settings) {
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
                Reply::Fx(result) => {
                    self.fx_loading = false;
                    match result {
                        Ok(q) => {
                            self.settings.fx_rate = q.rate;
                            self.settings.fx_updated = model::now_string();
                            self.settings.fx_source = if q.date.is_empty() {
                                q.source
                            } else {
                                format!("{} ({})", q.source, q.date)
                            };
                            self.save_settings();
                        }
                        Err(e) => self.toast(
                            ToastKind::Error,
                            format!("Couldn't fetch the live USD→SGD rate, using the last known one. {e}"),
                        ),
                    }
                }
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
        if ctx.input(|i| i.viewport().close_requested()) && self.dirty_since.is_some() {
            self.save_now();
        }

        ui::header(self, ctx);
        match self.tab {
            Tab::Add => ui::add::show(self, ctx),
            Tab::Import => ui::import::show(self, ctx),
            Tab::Collection => ui::collection::show(self, ctx),
            Tab::Settings => ui::settings::show(self, ctx),
        }
        ui::update::window(self, ctx);
        ui::settings::export_prompt(self, ctx);
        widgets::show_toasts(ctx, &mut self.toasts, now);
    }
}
