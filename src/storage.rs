//! JSON persistence for the collection and settings. The collection JSON is the
//! source of truth; the spreadsheet is generated from it.

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::model::{Entry, Settings};

const COLLECTION_FILE: &str = "collection.json";
const SETTINGS_FILE: &str = "settings.json";

pub const EXPORT_FILE: &str = "Cardboard Capital.xlsx";
/// Spreadsheet name used by versions before the rename.
const OLD_EXPORT_FILE: &str = "MTG Stock.xlsx";

/// The folder holding the exe. Everything lives under it so the install is portable:
/// `<app>\Cardboard Capital.exe`, `<app>\data\…`, `<app>\exports\…`.
pub fn app_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn data_dir(app_dir: &Path) -> PathBuf {
    app_dir.join("data")
}

pub fn exports_dir(app_dir: &Path) -> PathBuf {
    app_dir.join("exports")
}

pub fn cache_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("cache").join("images")
}

pub fn default_export_path(app_dir: &Path) -> PathBuf {
    exports_dir(app_dir).join(EXPORT_FILE)
}

/// Creates `data\` and `exports\`, and moves files from the old flat layout (everything
/// next to the exe) into them. Returns a description of each thing moved.
pub fn prepare_dirs(app_dir: &Path) -> Vec<String> {
    let data = data_dir(app_dir);
    let exports = exports_dir(app_dir);
    let _ = std::fs::create_dir_all(&data);
    let _ = std::fs::create_dir_all(&exports);

    let mut moved = Vec::new();
    let mut try_move = |from: PathBuf, to: PathBuf, label: String| {
        if from.exists() && !to.exists() && std::fs::rename(&from, &to).is_ok() {
            moved.push(label);
        }
    };

    let mut names: Vec<String> = vec![
        COLLECTION_FILE.into(),
        "collection.backup.json".into(),
        SETTINGS_FILE.into(),
        "cache".into(),
    ];
    // Collections that were set aside after a load error.
    if let Ok(rd) = std::fs::read_dir(app_dir) {
        names.extend(
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.starts_with("collection.corrupt-") && n.ends_with(".json")),
        );
    }
    for name in names {
        try_move(app_dir.join(&name), data.join(&name), format!("{name} → data"));
    }
    try_move(
        app_dir.join(OLD_EXPORT_FILE),
        exports.join(EXPORT_FILE),
        format!("{OLD_EXPORT_FILE} → exports\\{EXPORT_FILE}"),
    );
    try_move(
        app_dir.join(EXPORT_FILE),
        exports.join(EXPORT_FILE),
        format!("{EXPORT_FILE} → exports"),
    );
    moved
}

fn load<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| format!("{} is unreadable: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("Couldn't read {}: {e}", path.display())),
    }
}

/// Writes to a temp file first, then renames over the target, so a crash can't truncate it.
fn save<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let json = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| format!("Couldn't write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("Couldn't save {}: {e}", path.display()))
}

/// Loads the collection. On a parse error the bad file is set aside rather than overwritten.
pub fn load_collection(dir: &Path) -> (Vec<Entry>, Option<String>) {
    let path = dir.join(COLLECTION_FILE);
    match load::<Vec<Entry>>(&path) {
        Ok(Some(mut entries)) => {
            // Keep a copy of what we started the session with.
            let _ = std::fs::copy(&path, dir.join("collection.backup.json"));
            fix_uids(&mut entries);
            (entries, None)
        }
        Ok(None) => (Vec::new(), None),
        Err(e) => {
            let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
            let aside = dir.join(format!("collection.corrupt-{stamp}.json"));
            let _ = std::fs::rename(&path, &aside);
            (
                Vec::new(),
                Some(format!(
                    "{e}. It was moved to {} and a new collection was started.",
                    aside.display()
                )),
            )
        }
    }
}

pub fn save_collection(dir: &Path, entries: &[Entry]) -> Result<(), String> {
    save(&dir.join(COLLECTION_FILE), &entries)
}

pub fn load_settings(dir: &Path) -> Settings {
    load(&dir.join(SETTINGS_FILE))
        .ok()
        .flatten()
        .unwrap_or_default()
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

    #[test]
    fn roundtrip_and_uid_repair() {
        let dir = std::env::temp_dir().join(format!("cardboard_capital_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let entries = vec![
            Entry {
                uid: 0,
                name: "A".into(),
                qty: 1,
                ..Default::default()
            },
            Entry {
                uid: 5,
                name: "B".into(),
                qty: 2,
                ..Default::default()
            },
            Entry {
                uid: 5,
                name: "C".into(),
                qty: 3,
                ..Default::default()
            },
        ];
        save_collection(&dir, &entries).unwrap();
        let (loaded, err) = load_collection(&dir);
        assert!(err.is_none());
        assert_eq!(loaded.len(), 3);
        let mut uids: Vec<u64> = loaded.iter().map(|e| e.uid).collect();
        uids.sort();
        uids.dedup();
        assert_eq!(uids.len(), 3);
        assert!(!uids.contains(&0));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migrates_flat_layout_into_folders() {
        let app = std::env::temp_dir().join(format!("cardboard_capital_migrate_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&app);
        std::fs::create_dir_all(app.join("cache").join("images")).unwrap();
        std::fs::write(app.join("collection.json"), "[]").unwrap();
        std::fs::write(app.join("settings.json"), "{}").unwrap();
        std::fs::write(app.join("MTG Stock.xlsx"), "x").unwrap();
        std::fs::write(app.join("collection.corrupt-20260101-000000.json"), "?").unwrap();

        let moved = prepare_dirs(&app);
        assert_eq!(moved.len(), 5, "{moved:?}");
        let data = data_dir(&app);
        assert!(data.join("collection.json").exists());
        assert!(data.join("settings.json").exists());
        assert!(data.join("cache").join("images").is_dir());
        assert!(data.join("collection.corrupt-20260101-000000.json").exists());
        assert!(default_export_path(&app).exists());
        assert!(!app.join("collection.json").exists());
        assert!(!app.join("MTG Stock.xlsx").exists());

        // Running again does nothing, and existing data is never overwritten.
        std::fs::write(app.join("collection.json"), "[1]").unwrap();
        assert!(prepare_dirs(&app).is_empty());
        assert_eq!(std::fs::read_to_string(data.join("collection.json")).unwrap(), "[]");
        let _ = std::fs::remove_dir_all(&app);
    }
}
