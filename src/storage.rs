//! Where data lives, moving it there from older layouts, and JSON persistence. The collection
//! JSON is the source of truth; the spreadsheet is generated from it.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::model::{Entry, Game, Settings};

const COLLECTION_FILE: &str = "collection.json";
const BACKUP_FILE: &str = "collection.backup.json";
const SETTINGS_FILE: &str = "settings.json";

pub const EXPORT_FILE: &str = "Cardboard Capital.xlsx";
/// Overrides where data lives (absolute paths only); for tests and trial runs.
pub const HOME_ENV: &str = "CARDBOARD_CAPITAL_HOME";
/// A file next to the exe that keeps everything next to the exe instead (e.g. on a USB stick).
pub const PORTABLE_MARKER: &str = "portable.txt";
/// Left in an old data folder after its files were moved.
const NOTE_FILE: &str = "WHERE IS MY DATA.txt";

/// Where everything lives. Normally `Documents\Cardboard Capital\` holds
/// `data\settings.json`, `data\mtg\collection.json`, `exports\` and `account\`, and
/// `%LOCALAPPDATA%\Cardboard Capital\` holds machine-local files (image cache, instance lock).
#[derive(Clone, Debug, PartialEq)]
pub struct Paths {
    /// Folder holding the exe.
    pub exe_dir: PathBuf,
    /// The folder the user backs up.
    pub root: PathBuf,
    /// Holds settings.json.
    pub settings_dir: PathBuf,
    /// Holds the MTG collection, its start-of-session backup and set-aside unreadable copies.
    pub mtg_dir: PathBuf,
    /// Default spreadsheet folder.
    pub exports: PathBuf,
    /// developer.key (and later the online account).
    pub account: PathBuf,
    /// Downloaded card images.
    pub cache: PathBuf,
    /// Machine-local files that shouldn't be synced or backed up.
    pub local: PathBuf,
    /// Using the old files next to the exe because the usual folder couldn't be used.
    pub legacy: bool,
}

impl Paths {
    pub fn resolve() -> Paths {
        let exe_dir = exe_dir();
        let env = std::env::var_os(HOME_ENV).map(PathBuf::from);
        let portable = exe_dir.join(PORTABLE_MARKER).is_file();
        Paths::choose(
            &exe_dir,
            env,
            portable,
            dirs::document_dir(),
            dirs::data_local_dir(),
            cfg!(debug_assertions),
        )
    }

    /// An absolute `CARDBOARD_CAPITAL_HOME` wins; then next to the exe if `portable.txt` is
    /// there; then `Documents\Cardboard Capital` (`… (dev)` for debug builds, so trying out a
    /// change never touches real data); then next to the exe.
    fn choose(
        exe_dir: &Path,
        env: Option<PathBuf>,
        portable: bool,
        documents: Option<PathBuf>,
        local_app_data: Option<PathBuf>,
        dev: bool,
    ) -> Paths {
        let folder = if dev { "Cardboard Capital (dev)" } else { "Cardboard Capital" };
        if let Some(root) = env.filter(|p| p.is_absolute()) {
            return Paths::from_root(exe_dir, &root, &root.join("local"));
        }
        match documents {
            Some(docs) if !portable => {
                let root = docs.join(folder);
                let local = local_app_data
                    .map(|l| l.join(folder))
                    .unwrap_or_else(|| root.join("local"));
                Paths::from_root(exe_dir, &root, &local)
            }
            _ => Paths::from_root(exe_dir, exe_dir, &exe_dir.join("local")),
        }
    }

    pub fn from_root(exe_dir: &Path, root: &Path, local: &Path) -> Paths {
        let data = root.join("data");
        Paths {
            exe_dir: exe_dir.to_path_buf(),
            root: root.to_path_buf(),
            mtg_dir: data.join(Game::Mtg.id()),
            settings_dir: data,
            exports: root.join("exports"),
            account: root.join("account"),
            cache: local.join("cache").join("images"),
            local: local.to_path_buf(),
            legacy: false,
        }
    }

    /// The same paths, but reading and writing an older install's files next to the exe.
    fn legacy_at(&self, src: &Source) -> Paths {
        Paths {
            root: self.exe_dir.clone(),
            settings_dir: src.settings.clone(),
            mtg_dir: src.coll.clone(),
            exports: self.exe_dir.join("exports"),
            legacy: true,
            ..self.clone()
        }
    }

    pub fn default_export_path(&self) -> PathBuf {
        self.exports.join(EXPORT_FILE)
    }
}

/// The folder holding the exe.
pub fn exe_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// What start-up found and did, for the user.
#[derive(Debug)]
pub struct Boot {
    pub paths: Paths,
    /// Old data folders whose collection was moved into `paths.root` this time.
    pub moved_from: Vec<PathBuf>,
    /// Things the user should know (a collection that wasn't merged, a fallback).
    pub notices: Vec<String>,
}

/// Leftovers to delete once nothing holds them any more.
#[derive(Debug, Default, PartialEq)]
struct Cleanup {
    files: Vec<PathBuf>,
    /// Folders of cached images (only `*.img` files) to delete with their contents.
    image_dirs: Vec<PathBuf>,
    /// Folders to remove if they end up empty.
    empty_dirs: Vec<PathBuf>,
}

/// Makes sure the data folders are writable and moves data from older layouts into them.
/// If either fails, this session uses the old files next to the exe instead.
pub fn boot(paths: Paths) -> Boot {
    let exe = std::env::current_exe().unwrap_or_else(|_| paths.exe_dir.join("Cardboard-Capital.exe"));
    let (boot, mut cleanup) = match probe(&paths) {
        Ok(()) => migrate(paths),
        Err(e) => {
            // Blocked folders can usually still be read: if the collection is already there,
            // keep using it (saves will report errors) rather than showing an empty one.
            let in_place = paths.mtg_dir.join(COLLECTION_FILE).exists();
            // Without old files, use the same flat layout as the old files, so the next start
            // (and the migration once the folder is allowed) finds what this session saves.
            let fallback = match legacy_sources(&paths.exe_dir, true).first() {
                _ if in_place => paths.clone(),
                Some(src) => paths.legacy_at(src),
                None => paths.legacy_at(&Source::flat(&paths.exe_dir.join("data"))),
            };
            let notice = format!(
                "Couldn't write to {}: {e}. If Windows' ransomware protection (Controlled folder access) is on, \
                 allow the app in Windows Security → Virus & threat protection → Ransomware protection → \
                 Allow an app through Controlled folder access ({}). Using {} for now.",
                paths.root.display(),
                exe.display(),
                fallback.settings_dir.display()
            );
            let boot = Boot { paths: fallback, moved_from: Vec::new(), notices: vec![notice] };
            (boot, Cleanup::default())
        }
    };
    // Self-update leftovers: 1.2 kept them in data\update, 1.3+ next to the exe.
    let (staged_new, staged_old) = crate::update::staging(&exe);
    let old_update = boot.paths.exe_dir.join("data").join("update");
    cleanup.files.extend([staged_new, staged_old, old_update.join("old.exe"), old_update.join("new.exe")]);
    cleanup.empty_dirs.push(old_update);
    tidy_in_background(cleanup);
    boot
}

/// Creates the data folders and checks a file can be written there.
fn probe(paths: &Paths) -> Result<(), String> {
    fs::create_dir_all(&paths.mtg_dir).map_err(|e| e.to_string())?;
    let test = paths.settings_dir.join(".write-test");
    write_synced(&test, b"ok").map_err(|e| e.to_string())?;
    let _ = fs::remove_file(&test);
    Ok(())
}

/// Where an older install next to the exe keeps its collection and its settings.
#[derive(Clone, Debug, PartialEq)]
struct Source {
    coll: PathBuf,
    settings: PathBuf,
}

impl Source {
    /// Both in one folder (1.0–1.2).
    fn flat(dir: &Path) -> Source {
        Source { coll: dir.to_path_buf(), settings: dir.to_path_buf() }
    }
}

/// Older installs next to the exe: a 1.3 layout left there (`portable.txt` removed, or a session
/// that couldn't use Documents) when `nested`, else `<exe>\data` (1.1–1.2); then the exe folder (1.0).
fn legacy_sources(exe_dir: &Path, nested: bool) -> Vec<Source> {
    let data = exe_dir.join("data");
    let mtg = data.join(Game::Mtg.id());
    let mut out = Vec::new();
    if nested && mtg.join(COLLECTION_FILE).is_file() {
        out.push(Source { coll: mtg, settings: data });
    } else if data.join(COLLECTION_FILE).is_file() || data.join(SETTINGS_FILE).is_file() {
        out.push(Source::flat(&data));
    }
    if exe_dir.join(COLLECTION_FILE).is_file() {
        out.push(Source::flat(exe_dir));
    }
    out
}

/// The app's own files in a source: the collection, its backup and set-aside copies, and settings.
fn known_files(src: &Source) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = [src.coll.join(COLLECTION_FILE), src.coll.join(BACKUP_FILE), src.settings.join(SETTINGS_FILE)]
        .into_iter()
        .filter(|p| p.is_file())
        .collect();
    if let Ok(rd) = fs::read_dir(&src.coll) {
        files.extend(rd.flatten().map(|e| e.path()).filter(|p| {
            let n = file_name(p);
            n.starts_with("collection.corrupt-") && n.ends_with(".json")
        }));
    }
    files
}

fn file_name(p: &Path) -> String {
    p.file_name().unwrap_or_default().to_string_lossy().to_string()
}

/// Where an older file goes: settings beside the game folders, everything else into the MTG folder.
fn destination(paths: &Paths, from: &Path) -> PathBuf {
    let name = file_name(from);
    if name == SETTINGS_FILE {
        paths.settings_dir.join(name)
    } else {
        paths.mtg_dir.join(name)
    }
}

/// Moves pre-1.3 data into `paths`. Never overwrites or deletes user data: moved originals are
/// renamed to `*.moved` and kept, and a collection that differs from the one already there is
/// left alone (with a notice). On a failed copy, returns paths pointing at the old files.
fn migrate(paths: Paths) -> (Boot, Cleanup) {
    let mut boot = Boot { paths, moved_from: Vec::new(), notices: Vec::new() };
    let mut cleanup = Cleanup::default();
    let exe_dir = boot.paths.exe_dir.clone();
    if same_dir(&exe_dir, &boot.paths.root) {
        // The exe lives in the data folder (the old README's suggestion, or portable.txt):
        // just sort the old files into the new places.
        for src in legacy_sources(&exe_dir, false) {
            match restructure_in_place(&boot.paths, &src) {
                Ok(Some(notice)) => boot.notices.push(notice),
                Ok(None) => {}
                Err(e) => {
                    boot.notices.push(format!(
                        "Couldn't sort your collection into {}: {e}. Still using {} for now; it will be tried \
                         again next time.",
                        boot.paths.mtg_dir.display(),
                        src.coll.display()
                    ));
                    boot.paths = boot.paths.legacy_at(&src);
                    return (boot, cleanup);
                }
            }
        }
    } else {
        for src in legacy_sources(&exe_dir, true) {
            match migrate_from(&boot.paths, &src) {
                Ok(Step::Moved) => boot.moved_from.push(src.settings.clone()),
                Ok(Step::AlreadyThere) | Ok(Step::Nothing) => {}
                Ok(Step::Different) => boot.notices.push(format!(
                    "Found another collection in {} that's different from the one in {}. It wasn't merged \
                     or changed. To combine them, run the old copy's spreadsheet through Import → From a \
                     spreadsheet.",
                    src.coll.display(),
                    boot.paths.mtg_dir.display()
                )),
                Err(e) => {
                    boot.notices.push(format!(
                        "Couldn't move your collection to {}: {e}. Still using {} for now; it will be tried \
                         again next time.",
                        boot.paths.root.display(),
                        src.coll.display()
                    ));
                    boot.paths = boot.paths.legacy_at(&src);
                    return (boot, cleanup);
                }
            }
        }
    }
    // Cached images are re-downloadable: move them if that's a quick rename, otherwise drop them.
    for old in [exe_dir.join("data").join("cache"), exe_dir.join("cache")] {
        let images = old.join("images");
        // Only the app's own cache (nothing but `*.img` files) is touched.
        if !images.is_dir() || images == boot.paths.cache || !only_cached_images(&images) {
            continue;
        }
        let moved = !boot.paths.cache.exists()
            && boot.paths.cache.parent().is_some_and(|p| fs::create_dir_all(p).is_ok())
            && fs::rename(&images, &boot.paths.cache).is_ok();
        if !moved {
            cleanup.image_dirs.push(images);
        }
        cleanup.empty_dirs.push(old);
    }
    (boot, cleanup)
}

#[derive(Debug, PartialEq)]
enum Step {
    /// No collection or settings in the old folder.
    Nothing,
    Moved,
    /// The old collection was moved before (identical copy already in place).
    AlreadyThere,
    /// Both places have a collection and they differ; nothing was touched.
    Different,
}

fn migrate_from(paths: &Paths, src: &Source) -> Result<Step, String> {
    let files = known_files(src);
    let has = |n: &str| files.iter().any(|p| file_name(p) == n);
    if !has(COLLECTION_FILE) && !has(SETTINGS_FILE) {
        return Ok(Step::Nothing);
    }
    let dest_coll = paths.mtg_dir.join(COLLECTION_FILE);
    if dest_coll.exists() {
        if !has(COLLECTION_FILE) {
            return Ok(Step::Nothing);
        }
        if fs::read(src.coll.join(COLLECTION_FILE)).ok() != fs::read(&dest_coll).ok() {
            return Ok(Step::Different);
        }
        retire(src, &files, &paths.root);
        return Ok(Step::AlreadyThere);
    }

    // Collection and settings move together: manual prices are in the settings' currency.
    // Copy everything to temp files first, so a failure leaves the destination as it was.
    let copies: Vec<(PathBuf, PathBuf)> = files
        .iter()
        .filter(|p| file_name(p) != BACKUP_FILE)
        .filter(|p| file_name(p) == SETTINGS_FILE || !destination(paths, p).exists())
        .map(|p| (p.clone(), destination(paths, p)))
        .collect();
    let mut staged = Vec::new();
    for (from, to) in &copies {
        let tmp = tmp_path(to);
        staged.push(tmp.clone());
        if let Err(e) = copy_verified(from, &tmp) {
            for t in &staged {
                let _ = fs::remove_file(t);
            }
            return Err(e);
        }
    }
    // Settings from a session without a collection are kept aside rather than overwritten.
    let dest_settings = paths.settings_dir.join(SETTINGS_FILE);
    if has(SETTINGS_FILE) && dest_settings.exists() {
        rename_retry(&dest_settings, &free_name(&paths.settings_dir, "settings.replaced", "json"))
            .map_err(|e| format!("Couldn't set aside {}: {e}", dest_settings.display()))?;
    }
    // The collection goes in last: its presence means "migrated".
    let mut order: Vec<&(PathBuf, PathBuf)> = copies.iter().collect();
    order.sort_by_key(|(_, to)| to.file_name().is_some_and(|n| n == COLLECTION_FILE));
    for (_, to) in order {
        rename_retry(&tmp_path(to), to).map_err(|e| format!("Couldn't save {}: {e}", to.display()))?;
    }
    retire(src, &files, &paths.root);
    Ok(Step::Moved)
}

/// Renames moved originals to `*.moved` (kept as a backup) and leaves a note saying where the
/// data went. Older versions then start empty instead of silently editing a stale copy.
fn retire(src: &Source, files: &[PathBuf], root: &Path) {
    for f in files {
        if let Some(dir) = f.parent() {
            let _ = rename_retry(f, &free_name(dir, &file_name(f), "moved"));
        }
    }
    let note = format!(
        "Cardboard Capital 1.3 moved your collection and settings to:\r\n\r\n    {}\r\n\r\n\
         The files here were renamed to *.moved and are kept only as a backup.\r\n\
         Versions before 1.3 look for data in this folder, so they would start with an empty collection.\r\n",
        root.display()
    );
    let _ = fs::write(src.settings.join(NOTE_FILE), note);
}

/// When the exe folder is the data folder: rename old files into their new places on the same
/// drive. Err if the collection itself couldn't be moved (the caller keeps using it where it is).
fn restructure_in_place(paths: &Paths, src: &Source) -> Result<Option<String>, String> {
    let files = known_files(src);
    let _ = fs::create_dir_all(&paths.mtg_dir);
    let from = src.coll.join(COLLECTION_FILE);
    let dest_coll = paths.mtg_dir.join(COLLECTION_FILE);
    if from.is_file() && dest_coll.exists() && from != dest_coll {
        if fs::read(&from).ok() != fs::read(&dest_coll).ok() {
            return Ok(Some(format!(
                "Found an older collection at {} next to the one in {}. It wasn't merged or changed.",
                from.display(),
                paths.mtg_dir.display()
            )));
        }
        let _ = rename_retry(&from, &free_name(&src.coll, COLLECTION_FILE, "moved"));
    }
    // The collection first: if it can't move, nothing else does either.
    let mut files: Vec<PathBuf> = files.into_iter().filter(|f| f.exists()).collect();
    files.sort_by_key(|f| file_name(f) != COLLECTION_FILE);
    for f in &files {
        let to = destination(paths, f);
        if &to == f || to.exists() {
            continue;
        }
        let moved = rename_retry(f, &to);
        if file_name(f) == COLLECTION_FILE {
            moved.map_err(|e| format!("couldn't move {}: {e}", f.display()))?;
        }
    }
    Ok(None)
}

/// Deletes update leftovers and dropped caches, retrying for a while: right after an update the
/// previous exe may still be running.
fn tidy_in_background(mut c: Cleanup) {
    c.files.retain(|f| f.exists());
    c.image_dirs.retain(|d| d.is_dir() && only_cached_images(d));
    c.empty_dirs.retain(|d| d.is_dir());
    if c.files.is_empty() && c.image_dirs.is_empty() && c.empty_dirs.is_empty() {
        return;
    }
    std::thread::spawn(move || {
        for _ in 0..30 {
            c.files.retain(|f| fs::remove_file(f).is_err() && f.exists());
            c.image_dirs.retain(|d| fs::remove_dir_all(d).is_err() && d.exists());
            if c.files.is_empty() && c.image_dirs.is_empty() {
                break;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
        // Only removed if nothing else is left in them.
        for d in &c.empty_dirs {
            let _ = fs::remove_dir(d);
        }
    });
}

/// Only delete an image folder if it holds nothing but the app's `*.img` files.
fn only_cached_images(dir: &Path) -> bool {
    fs::read_dir(dir).is_ok_and(|rd| {
        rd.flatten().all(|e| {
            e.file_type().is_ok_and(|t| t.is_file()) && e.file_name().to_string_lossy().ends_with(".img")
        })
    })
}

/// Clears export locations that point at the pre-1.3 folders next to the exe, so the
/// spreadsheet follows the data. A folder the user chose elsewhere is kept. Returns whether
/// anything changed.
pub fn fix_export_settings(s: &mut Settings, exe_dir: &Path) -> bool {
    let old = |p: &Path| same_dir(p, exe_dir) || same_dir(p, &exe_dir.join("exports"));
    let mut changed = false;
    if !s.export_dir.trim().is_empty() && same_dir(Path::new(s.export_dir.trim()), &exe_dir.join("exports")) {
        s.export_dir.clear();
        changed = true;
    }
    let legacy_parent = Path::new(s.export_path.trim()).parent().map(Path::to_path_buf);
    if !s.export_path.trim().is_empty() && legacy_parent.as_deref().is_some_and(old) {
        s.export_path.clear();
        changed = true;
    }
    changed
}

/// Whether two paths are the same folder (case-insensitive, like Windows).
fn same_dir(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        fs::canonicalize(p)
            .unwrap_or_else(|_| p.to_path_buf())
            .to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .to_lowercase()
    };
    norm(a) == norm(b)
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

/// `dir\name.<suffix>`, or with a timestamp if that's taken.
fn free_name(dir: &Path, name: &str, suffix: &str) -> PathBuf {
    let plain = dir.join(format!("{name}.{suffix}"));
    if !plain.exists() {
        return plain;
    }
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    (1..)
        .map(|i| dir.join(format!("{name}.{suffix}-{stamp}-{i}")))
        .find(|p| !p.exists())
        .expect("some name is free")
}

/// Copies a file and checks the copy byte for byte.
fn copy_verified(from: &Path, to: &Path) -> Result<(), String> {
    let bytes = fs::read(from).map_err(|e| format!("Couldn't read {}: {e}", from.display()))?;
    write_synced(to, &bytes).map_err(|e| format!("Couldn't write {}: {e}", to.display()))?;
    match fs::read(to) {
        Ok(back) if back == bytes => Ok(()),
        _ => Err(format!("The copy at {} didn't match the original", to.display())),
    }
}

fn write_synced(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut f = fs::File::create(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}

/// Errors that usually clear up in a moment: antivirus, the search indexer or OneDrive briefly
/// holding the file.
fn transient(e: &io::Error) -> bool {
    // 32 = sharing violation, 33 = lock violation.
    e.kind() == io::ErrorKind::PermissionDenied || matches!(e.raw_os_error(), Some(32 | 33))
}

/// Renames (replacing the target), retrying briefly while something else holds the file.
fn rename_retry(from: &Path, to: &Path) -> io::Result<()> {
    let mut result = fs::rename(from, to);
    for ms in [10, 25, 50, 100, 200, 400] {
        match &result {
            Err(e) if transient(e) => std::thread::sleep(Duration::from_millis(ms)),
            _ => break,
        }
        result = fs::rename(from, to);
    }
    result
}

/// A loaded file, plus what the user needs to know about it.
pub struct Loaded<T> {
    pub value: T,
    /// Something to tell the user (e.g. an unreadable file was set aside).
    pub notice: Option<String>,
    /// The file exists but can't be read right now (e.g. locked by another program, or
    /// OneDrive not running). Nothing may be saved over it this session.
    pub locked: Option<String>,
}

enum Read<T> {
    Missing,
    Parsed(T),
    /// Couldn't be read (I/O). Says nothing about the contents.
    Unreadable(String),
    /// Read fine but isn't valid JSON for this type.
    Invalid(String),
}

fn read<T: DeserializeOwned>(path: &Path) -> Read<T> {
    let mut last = None;
    for attempt in 0..5 {
        if attempt > 0 {
            std::thread::sleep(Duration::from_millis(100));
        }
        match fs::read_to_string(path) {
            Ok(text) => {
                return match serde_json::from_str(&text) {
                    Ok(v) => Read::Parsed(v),
                    Err(e) => Read::Invalid(format!("{} is unreadable: {e}", path.display())),
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Read::Missing,
            Err(e) if e.kind() == io::ErrorKind::InvalidData => {
                return Read::Invalid(format!("{} is unreadable: {e}", path.display()))
            }
            Err(e) => last = Some(e),
        }
    }
    Read::Unreadable(format!(
        "Couldn't read {}: {}",
        path.display(),
        last.map(|e| e.to_string()).unwrap_or_default()
    ))
}

/// Writes to a temp file first, then renames over the target, so a crash can't truncate it.
fn save<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let tmp = tmp_path(path);
    write_synced(&tmp, json.as_bytes()).map_err(|e| format!("Couldn't write {}: {e}", tmp.display()))?;
    rename_retry(&tmp, path).map_err(|e| format!("Couldn't save {}: {e}", path.display()))
}

/// Moves an unparseable file aside (never overwritten) and says where it went.
fn set_aside(path: &Path, what: &str, e: &str) -> Result<String, String> {
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
    let dir = path.parent().unwrap_or(Path::new("."));
    let aside = free_name(dir, &format!("{stem}.corrupt-{stamp}"), "json");
    match rename_retry(path, &aside) {
        Ok(()) => Ok(format!("{e}. It was moved to {} and {what}.", aside.display())),
        // Never save over a file that couldn't be set aside.
        Err(e2) => Err(format!("{e}. It couldn't be set aside ({e2}), so nothing will be saved over it; restart the app to try again.")),
    }
}

/// Loads the collection. A parse error sets the bad file aside rather than overwriting it.
pub fn load_collection(dir: &Path) -> Loaded<Vec<Entry>> {
    let path = dir.join(COLLECTION_FILE);
    let (value, notice, locked) = match read::<Vec<Entry>>(&path) {
        Read::Parsed(mut entries) => {
            // Keep a copy of what we started the session with.
            let _ = fs::copy(&path, dir.join(BACKUP_FILE));
            fix_uids(&mut entries);
            (entries, None, None)
        }
        Read::Missing => (Vec::new(), None, None),
        Read::Invalid(e) => match set_aside(&path, "a new collection was started", &e) {
            Ok(msg) => (Vec::new(), Some(msg), None),
            Err(msg) => (Vec::new(), None, Some(msg)),
        },
        Read::Unreadable(e) => (
            Vec::new(),
            None,
            Some(format!("{e}. Your collection wasn't changed; restart the app to try again.")),
        ),
    };
    Loaded { value, notice, locked }
}

pub fn save_collection(dir: &Path, entries: &[Entry]) -> Result<(), String> {
    save(&dir.join(COLLECTION_FILE), &entries)
}

pub fn load_settings(dir: &Path) -> Loaded<Settings> {
    let path = dir.join(SETTINGS_FILE);
    let (value, notice, locked) = match read::<Settings>(&path) {
        Read::Parsed(s) => (s, None, None),
        Read::Missing => (Settings::default(), None, None),
        Read::Invalid(e) => match set_aside(&path, "default settings are used", &e) {
            Ok(msg) => (Settings::default(), Some(msg), None),
            Err(msg) => (Settings::default(), None, Some(msg)),
        },
        Read::Unreadable(e) => (
            Settings::default(),
            None,
            Some(format!("{e}. Your settings weren't changed; restart the app to try again.")),
        ),
    };
    Loaded { value, notice, locked }
}

pub fn save_settings(dir: &Path, settings: &Settings) -> Result<(), String> {
    save(&dir.join(SETTINGS_FILE), settings)
}

/// Every entry needs a unique, non-zero uid for UI bookkeeping.
fn fix_uids(entries: &mut [Entry]) {
    let mut seen = std::collections::HashSet::new();
    let mut next = entries.iter().map(|e| e.uid).max().unwrap_or(0) + 1;
    for e in entries.iter_mut() {
        if e.uid == 0 || !seen.insert(e.uid) {
            e.uid = next;
            seen.insert(next);
            next += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh, empty folder for one test.
    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("cardboard_capital_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// An exe folder with a 1.2 layout, and paths for a separate home.
    fn v12_install(name: &str) -> (PathBuf, Paths) {
        let base = temp(name);
        let app = base.join("app");
        let data = app.join("data");
        fs::create_dir_all(data.join("cache").join("images")).unwrap();
        fs::create_dir_all(data.join("update")).unwrap();
        fs::write(data.join("collection.json"), r#"[{"uid":1,"name":"Bolt","qty":2}]"#).unwrap();
        fs::write(data.join("collection.backup.json"), "[]").unwrap();
        fs::write(data.join("collection.corrupt-20260101-000000.json"), "?").unwrap();
        fs::write(data.join("settings.json"), r#"{"currency":"EUR"}"#).unwrap();
        fs::write(data.join("cache").join("images").join("0123.img"), "x").unwrap();
        fs::write(data.join("update").join("old.exe"), "MZ").unwrap();
        let paths = Paths::from_root(&app, &base.join("home"), &base.join("local"));
        (base, paths)
    }

    #[test]
    fn roundtrip_and_uid_repair() {
        let dir = temp("roundtrip");
        let entries = vec![
            Entry { uid: 0, name: "A".into(), qty: 1, ..Default::default() },
            Entry { uid: 5, name: "B".into(), qty: 2, ..Default::default() },
            Entry { uid: 5, name: "C".into(), qty: 3, ..Default::default() },
        ];
        save_collection(&dir, &entries).unwrap();
        let loaded = load_collection(&dir);
        assert!(loaded.notice.is_none() && loaded.locked.is_none());
        assert_eq!(loaded.value.len(), 3);
        let mut uids: Vec<u64> = loaded.value.iter().map(|e| e.uid).collect();
        uids.sort();
        uids.dedup();
        assert_eq!(uids.len(), 3);
        assert!(!uids.contains(&0));
        assert!(dir.join(BACKUP_FILE).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn root_resolution_order() {
        let exe = Path::new(r"D:\Apps\CC");
        let docs = Some(PathBuf::from(r"C:\Users\A\OneDrive\Documents"));
        let local = Some(PathBuf::from(r"C:\Users\A\AppData\Local"));
        let p = Paths::choose(exe, None, false, docs.clone(), local.clone(), false);
        assert_eq!(p.root, Path::new(r"C:\Users\A\OneDrive\Documents\Cardboard Capital"));
        assert_eq!(p.settings_dir, p.root.join("data"));
        assert_eq!(p.mtg_dir, p.root.join("data").join("mtg"));
        assert_eq!(p.exports, p.root.join("exports"));
        assert_eq!(p.account, p.root.join("account"));
        assert_eq!(p.cache, Path::new(r"C:\Users\A\AppData\Local\Cardboard Capital\cache\images"));
        assert!(!p.legacy);
        // Debug builds keep away from real data.
        let dev = Paths::choose(exe, None, false, docs.clone(), local.clone(), true);
        assert!(dev.root.ends_with("Cardboard Capital (dev)"));
        // portable.txt keeps everything next to the exe.
        let portable = Paths::choose(exe, None, true, docs.clone(), local.clone(), false);
        assert_eq!(portable.root, exe);
        assert_eq!(portable.cache, exe.join("local").join("cache").join("images"));
        // An absolute override wins over everything; a relative one is ignored.
        let over = Paths::choose(exe, Some(PathBuf::from(r"E:\try")), true, docs.clone(), local.clone(), false);
        assert_eq!(over.root, Path::new(r"E:\try"));
        assert_eq!(over.cache, Path::new(r"E:\try\local\cache\images"));
        let rel = Paths::choose(exe, Some(PathBuf::from("try")), false, docs, local, false);
        assert!(rel.root.ends_with("Cardboard Capital"));
        // No Documents folder at all: next to the exe.
        assert_eq!(Paths::choose(exe, None, false, None, None, false).root, exe);
    }

    #[test]
    fn migrates_a_v12_install() {
        let (base, paths) = v12_install("migrate12");
        let app = paths.exe_dir.clone();
        let old = app.join("data");
        probe(&paths).unwrap();
        let (boot, cleanup) = migrate(paths.clone());
        assert_eq!(boot.paths, paths);
        assert_eq!(boot.moved_from, vec![old.clone()]);
        assert!(boot.notices.is_empty(), "{:?}", boot.notices);
        assert_eq!(
            fs::read_to_string(paths.mtg_dir.join("collection.json")).unwrap(),
            r#"[{"uid":1,"name":"Bolt","qty":2}]"#
        );
        assert!(paths.mtg_dir.join("collection.corrupt-20260101-000000.json").exists());
        assert_eq!(fs::read_to_string(paths.settings_dir.join("settings.json")).unwrap(), r#"{"currency":"EUR"}"#);
        // The start-of-session backup is recreated on load, not copied.
        assert!(!paths.mtg_dir.join(BACKUP_FILE).exists());
        // Originals are kept, renamed, with a note.
        assert!(old.join("collection.json.moved").exists());
        assert!(old.join("settings.json.moved").exists());
        assert!(old.join("collection.backup.json.moved").exists());
        assert!(!old.join("collection.json").exists());
        assert!(old.join(NOTE_FILE).exists());
        // Images moved with a rename; nothing created next to the exe.
        assert!(paths.cache.join("0123.img").exists());
        assert!(cleanup.image_dirs.is_empty());
        assert!(!app.join("exports").exists());
        // Running again does nothing.
        let (again, _) = migrate(paths.clone());
        assert!(again.moved_from.is_empty() && again.notices.is_empty());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn migrates_the_flat_v10_layout() {
        let base = temp("migrate10");
        let app = base.join("app");
        fs::create_dir_all(&app).unwrap();
        fs::write(app.join("collection.json"), "[]").unwrap();
        fs::write(app.join("settings.json"), "{}").unwrap();
        fs::write(app.join("unrelated.json"), "{}").unwrap();
        let paths = Paths::from_root(&app, &base.join("home"), &base.join("local"));
        probe(&paths).unwrap();
        let (boot, _) = migrate(paths.clone());
        assert_eq!(boot.moved_from, vec![app.clone()]);
        assert!(paths.mtg_dir.join("collection.json").exists());
        assert!(paths.settings_dir.join("settings.json").exists());
        assert!(app.join("unrelated.json").exists(), "only the app's own files are touched");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn exe_inside_the_data_folder_is_sorted_in_place() {
        let base = temp("inplace");
        let root = base.join("Cardboard Capital");
        let data = root.join("data");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("collection.json"), "[]").unwrap();
        fs::write(data.join("collection.backup.json"), "[]").unwrap();
        fs::write(data.join("settings.json"), "{}").unwrap();
        let paths = Paths::from_root(&root, &root, &root.join("local"));
        probe(&paths).unwrap();
        let (boot, _) = migrate(paths.clone());
        assert!(boot.moved_from.is_empty() && boot.notices.is_empty());
        assert!(paths.mtg_dir.join("collection.json").exists());
        assert!(paths.mtg_dir.join(BACKUP_FILE).exists());
        assert!(data.join("settings.json").exists(), "settings stay where they are");
        assert!(!data.join("collection.json").exists());
        assert!(!data.join(NOTE_FILE).exists());
        let _ = fs::remove_dir_all(&base);
    }

    /// portable.txt removed (or a session that couldn't use Documents): the 1.3 layout next to the
    /// exe moves as a whole, collection included.
    #[test]
    fn migrates_a_13_layout_left_next_to_the_exe() {
        let base = temp("nested");
        let app = base.join("app");
        let data = app.join("data");
        fs::create_dir_all(data.join("mtg")).unwrap();
        fs::write(data.join("mtg").join("collection.json"), r#"[{"uid":1,"qty":1}]"#).unwrap();
        fs::write(data.join("settings.json"), r#"{"currency":"EUR"}"#).unwrap();
        let paths = Paths::from_root(&app, &base.join("home"), &base.join("local"));
        probe(&paths).unwrap();
        let (boot, _) = migrate(paths.clone());
        assert_eq!(boot.moved_from.len(), 1, "{:?}", boot.notices);
        assert_eq!(fs::read_to_string(paths.mtg_dir.join("collection.json")).unwrap(), r#"[{"uid":1,"qty":1}]"#);
        assert_eq!(fs::read_to_string(paths.settings_dir.join("settings.json")).unwrap(), r#"{"currency":"EUR"}"#);
        assert!(data.join("mtg").join("collection.json.moved").exists());
        assert!(data.join(NOTE_FILE).exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn never_overwrites_a_different_collection() {
        let (base, paths) = v12_install("different");
        let old = paths.exe_dir.join("data");
        fs::create_dir_all(&paths.mtg_dir).unwrap();
        fs::write(paths.mtg_dir.join("collection.json"), "[]").unwrap();
        let (boot, _) = migrate(paths.clone());
        assert!(boot.moved_from.is_empty());
        assert_eq!(boot.notices.len(), 1);
        assert_eq!(fs::read_to_string(paths.mtg_dir.join("collection.json")).unwrap(), "[]");
        assert!(old.join("collection.json").exists(), "the other collection is left alone");
        assert!(!paths.settings_dir.join("settings.json").exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn identical_copy_is_just_retired() {
        let (base, paths) = v12_install("identical");
        let old = paths.exe_dir.join("data");
        fs::create_dir_all(&paths.mtg_dir).unwrap();
        fs::copy(old.join("collection.json"), paths.mtg_dir.join("collection.json")).unwrap();
        let (boot, _) = migrate(paths.clone());
        assert!(boot.moved_from.is_empty() && boot.notices.is_empty());
        assert!(old.join("collection.json.moved").exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn settings_without_a_collection_are_set_aside() {
        let (base, paths) = v12_install("replaced");
        fs::create_dir_all(&paths.mtg_dir).unwrap();
        fs::write(paths.settings_dir.join("settings.json"), r#"{"currency":"USD"}"#).unwrap();
        let (boot, _) = migrate(paths.clone());
        assert_eq!(boot.moved_from.len(), 1);
        assert_eq!(fs::read_to_string(paths.settings_dir.join("settings.json")).unwrap(), r#"{"currency":"EUR"}"#);
        assert_eq!(
            fs::read_to_string(paths.settings_dir.join("settings.replaced.json")).unwrap(),
            r#"{"currency":"USD"}"#
        );
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn failed_copy_falls_back_to_the_old_files() {
        let (base, paths) = v12_install("fallback");
        let old = paths.exe_dir.join("data");
        fs::create_dir_all(&paths.mtg_dir).unwrap();
        // A folder where the temp copy should go makes the copy fail.
        fs::create_dir_all(paths.mtg_dir.join("collection.json.tmp")).unwrap();
        let (boot, _) = migrate(paths.clone());
        assert!(boot.paths.legacy);
        assert_eq!(boot.paths.mtg_dir, old);
        assert_eq!(boot.paths.settings_dir, old);
        assert_eq!(boot.notices.len(), 1);
        assert!(old.join("collection.json").exists() && old.join("settings.json").exists());
        assert!(!paths.mtg_dir.join("collection.json").exists());
        assert!(!paths.settings_dir.join("settings.json.tmp").exists(), "staged copies are removed");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn export_settings_follow_the_data() {
        let exe = Path::new(r"D:\Apps\CC");
        let mut s = Settings { export_dir: r"D:\Apps\CC\exports".into(), ..Default::default() };
        assert!(fix_export_settings(&mut s, exe));
        assert!(s.export_dir.is_empty());
        let mut s = Settings { export_path: r"D:\Apps\CC\MTG Stock.xlsx".into(), ..Default::default() };
        assert!(fix_export_settings(&mut s, exe));
        assert!(s.export_path.is_empty());
        let mut s = Settings { export_dir: r"C:\Users\A\Documents\Cardboard Capital".into(), ..Default::default() };
        assert!(!fix_export_settings(&mut s, exe), "a folder the user chose is kept");
        assert!(!s.export_dir.is_empty());
    }

    #[test]
    fn unparseable_files_are_set_aside_and_missing_ones_are_empty() {
        let dir = temp("corrupt");
        assert!(load_collection(&dir).value.is_empty());
        fs::write(dir.join("collection.json"), "{nope").unwrap();
        let loaded = load_collection(&dir);
        assert!(loaded.value.is_empty() && loaded.locked.is_none());
        assert!(loaded.notice.is_some());
        assert!(!dir.join("collection.json").exists());
        fs::write(dir.join("settings.json"), "{nope").unwrap();
        let settings = load_settings(&dir);
        assert!(settings.notice.is_some() && settings.locked.is_none());
        let _ = fs::remove_dir_all(&dir);
    }

    /// A file another program has open without sharing can't be read: that's not corruption.
    #[cfg(windows)]
    #[test]
    fn locked_files_are_not_treated_as_corrupt() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = temp("locked");
        fs::write(dir.join("collection.json"), "[]").unwrap();
        let hold = fs::OpenOptions::new().read(true).share_mode(0).open(dir.join("collection.json")).unwrap();
        let loaded = load_collection(&dir);
        assert!(loaded.locked.is_some());
        assert!(loaded.notice.is_none());
        drop(hold);
        assert!(dir.join("collection.json").exists(), "the file wasn't set aside");
        let _ = fs::remove_dir_all(&dir);
    }

    /// Saving waits out a brief lock (antivirus, OneDrive) instead of failing.
    #[cfg(windows)]
    #[test]
    fn save_retries_while_the_file_is_briefly_locked() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = temp("retry");
        fs::write(dir.join("settings.json"), "{}").unwrap();
        let hold = fs::OpenOptions::new().read(true).share_mode(0).open(dir.join("settings.json")).unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(120));
            drop(hold);
        });
        save_settings(&dir, &Settings::default()).unwrap();
        release.join().unwrap();
        assert!(load_settings(&dir).value == Settings::default());
        let _ = fs::remove_dir_all(&dir);
    }
}
