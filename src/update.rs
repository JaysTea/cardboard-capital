//! Checks GitHub Releases for a newer version on the user's channel, and swaps the
//! running exe for the new one.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use semver::Version;
use serde::Deserialize;

use crate::model::UpdateChannel;

pub const REPO: &str = "JaysTea/cardboard-capital";
/// The release asset that is the app itself.
const ASSET: &str = "Cardboard-Capital.exe";

pub fn releases_page() -> String {
    format!("https://github.com/{REPO}/releases")
}

static PRETEND: OnceLock<Version> = OnceLock::new();

/// Makes the app behave as if it were an older version (`--pretend-version`), to test updates.
pub fn pretend_version(v: &str) -> Result<(), String> {
    let v = Version::parse(v.trim_start_matches('v')).map_err(|e| format!("bad version {v}: {e}"))?;
    let _ = PRETEND.set(v);
    Ok(())
}

pub fn current_version() -> Version {
    PRETEND
        .get()
        .cloned()
        .unwrap_or_else(|| Version::parse(env!("CARGO_PKG_VERSION")).expect("Cargo version is SemVer"))
}

#[derive(Deserialize, Clone, Debug)]
pub struct Release {
    pub tag_name: String,
    #[serde(default)]
    pub body: Option<String>,
    pub html_url: String,
    #[serde(default)]
    pub draft: bool,
    #[serde(default)]
    pub prerelease: bool,
    #[serde(default)]
    pub assets: Vec<Asset>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Asset {
    pub name: String,
    pub browser_download_url: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UpdateInfo {
    pub version: Version,
    pub tag: String,
    pub notes: String,
    pub html_url: String,
    pub asset_url: String,
    pub asset_size: u64,
    pub prerelease: bool,
}

/// The newest release on `channel` that's newer than `current` and has an .exe attached.
pub fn pick(releases: &[Release], channel: UpdateChannel, current: &Version) -> Option<UpdateInfo> {
    if channel == UpdateChannel::Off {
        return None;
    }
    releases
        .iter()
        .filter(|r| !r.draft && (channel == UpdateChannel::Beta || !r.prerelease))
        .filter_map(|r| {
            let version = Version::parse(r.tag_name.trim_start_matches('v')).ok()?;
            let asset = r.assets.iter().find(|a| a.name.eq_ignore_ascii_case(ASSET))?;
            Some(UpdateInfo {
                version,
                tag: r.tag_name.clone(),
                notes: r.body.clone().unwrap_or_default(),
                html_url: r.html_url.clone(),
                asset_url: asset.browser_download_url.clone(),
                asset_size: asset.size,
                prerelease: r.prerelease,
            })
        })
        .filter(|u| u.version > *current)
        .max_by(|a, b| a.version.cmp(&b.version))
}

pub fn check(agent: &ureq::Agent, channel: UpdateChannel) -> Result<Option<UpdateInfo>, String> {
    if channel == UpdateChannel::Off {
        return Ok(None);
    }
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=50");
    let releases: Vec<Release> = match agent
        .get(&url)
        .set("Accept", "application/vnd.github+json")
        .call()
    {
        Ok(resp) => resp.into_json().map_err(|e| format!("Unexpected reply from GitHub: {e}"))?,
        Err(ureq::Error::Status(403 | 429, _)) => {
            return Err("GitHub is rate-limiting update checks right now. Try again in an hour.".into())
        }
        Err(ureq::Error::Status(code, _)) => return Err(format!("GitHub returned HTTP {code}.")),
        Err(e) => return Err(format!("Couldn't reach GitHub: {e}")),
    };
    Ok(pick(&releases, channel, &current_version()))
}

/// Where an update is staged: `(<exe>.new, <exe>.old)`, next to the exe. Windows only lets a
/// running exe be renamed within its own drive, so these can't live in the data folder.
/// Leftovers are deleted on the next start (see `storage::boot`).
pub fn staging(exe: &Path) -> (PathBuf, PathBuf) {
    let with = |suffix: &str| {
        let mut name = exe.file_name().unwrap_or_default().to_os_string();
        name.push(suffix);
        exe.with_file_name(name)
    };
    (with(".new"), with(".old"))
}

/// Downloads the update and puts it in place of the running exe. Returns the exe path to launch.
/// The running exe is renamed to `<exe>.old` (Windows allows renaming a running exe) and
/// deleted on the next start. Data and exports aren't touched.
pub fn install(agent: &ureq::Agent, info: &UpdateInfo) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("Can't find the running app: {e}"))?;
    let (new_path, old_path) = staging(&exe);
    let folder = exe.parent().unwrap_or(Path::new(".")).display().to_string();

    // Check the folder can be written before downloading anything.
    if let Err(e) = std::fs::write(&new_path, b"") {
        return Err(if e.kind() == std::io::ErrorKind::PermissionDenied {
            format!(
                "The app's folder ({folder}) can't be changed, so it can't update itself. Download the new \
                 version from the Releases page, or move the app to a folder you own."
            )
        } else {
            format!("Can't write to {folder}: {e}")
        });
    }

    let resp = agent
        .get(&info.asset_url)
        .call()
        .map_err(|e| format!("Download failed: {e}"))?;
    let mut bytes = Vec::new();
    resp.into_reader()
        .take(300 * 1024 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("Download failed: {e}"))?;
    if !bytes.starts_with(b"MZ") || (info.asset_size > 0 && bytes.len() as u64 != info.asset_size) {
        let _ = std::fs::remove_file(&new_path);
        return Err("The downloaded file doesn't look like a complete app. Nothing was changed.".into());
    }
    let written = std::fs::File::create(&new_path).and_then(|mut f| {
        std::io::Write::write_all(&mut f, &bytes)?;
        f.sync_all()
    });
    if let Err(e) = written {
        let _ = std::fs::remove_file(&new_path);
        return Err(format!("Can't save the download: {e}"));
    }
    // Antivirus may have removed it in the meantime.
    if !new_path.is_file() {
        return Err("The downloaded update disappeared (an antivirus may have removed it). Nothing was changed.".into());
    }

    let _ = std::fs::remove_file(&old_path);
    std::fs::rename(&exe, &old_path).map_err(|e| format!("Can't move the current app aside: {e}"))?;
    if let Err(e) = std::fs::rename(&new_path, &exe) {
        // Put the original back.
        return Err(match std::fs::rename(&old_path, &exe) {
            Ok(()) => format!("Can't put the new version in place: {e}. Nothing was changed."),
            Err(e2) => format!(
                "Can't put the new version in place: {e}. Restoring the current version failed too ({e2}): \
                 it's now at {}. Rename it back to {} to use it.",
                old_path.display(),
                exe.file_name().unwrap_or_default().to_string_lossy()
            ),
        });
    }
    Ok(exe)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn releases() -> Vec<Release> {
        serde_json::from_str(
            r#"[
            {"tag_name": "v1.3.0-beta.1", "html_url": "u", "prerelease": true,
             "assets": [{"name": "Cardboard-Capital.exe", "browser_download_url": "b131", "size": 5}]},
            {"tag_name": "v1.2.1", "html_url": "u", "draft": true,
             "assets": [{"name": "Cardboard-Capital.exe", "browser_download_url": "draft", "size": 5}]},
            {"tag_name": "v1.2.0", "html_url": "u", "body": "notes",
             "assets": [{"name": "Cardboard-Capital.exe", "browser_download_url": "s120", "size": 5}]},
            {"tag_name": "v1.2.0-beta.2", "html_url": "u", "prerelease": true,
             "assets": [{"name": "Cardboard-Capital.exe", "browser_download_url": "b120", "size": 5}]},
            {"tag_name": "v1.1.5", "html_url": "u", "assets": [{"name": "source.zip", "browser_download_url": "x"}]},
            {"tag_name": "v1.1.0", "html_url": "u",
             "assets": [{"name": "Cardboard-Capital.exe", "browser_download_url": "s110", "size": 5}]}
        ]"#,
        )
        .unwrap()
    }

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn stable_channel_ignores_betas_and_drafts() {
        let u = pick(&releases(), UpdateChannel::Stable, &v("1.1.0")).unwrap();
        assert_eq!((u.tag.as_str(), u.asset_url.as_str(), u.notes.as_str()), ("v1.2.0", "s120", "notes"));
        assert_eq!(pick(&releases(), UpdateChannel::Stable, &v("1.2.0")), None);
    }

    #[test]
    fn beta_channel_gets_newest_including_betas() {
        let u = pick(&releases(), UpdateChannel::Beta, &v("1.1.0")).unwrap();
        assert_eq!(u.tag, "v1.3.0-beta.1");
        assert!(u.prerelease);
    }

    #[test]
    fn beta_users_get_the_final_release() {
        // 1.2.0 is newer than 1.2.0-beta.2, so a beta tester on Stable gets the release.
        let u = pick(&releases(), UpdateChannel::Stable, &v("1.2.0-beta.2")).unwrap();
        assert_eq!(u.tag, "v1.2.0");
    }

    #[test]
    fn off_and_no_exe() {
        assert_eq!(pick(&releases(), UpdateChannel::Off, &v("0.1.0")), None);
        // v1.1.5 has no exe so it's never offered.
        let u = pick(&releases()[4..], UpdateChannel::Stable, &v("1.1.0"));
        assert_eq!(u, None);
    }

    #[test]
    fn only_the_app_itself_is_downloaded() {
        let rel: Vec<Release> = serde_json::from_str(
            r#"[{"tag_name": "v2.0.0", "html_url": "u", "assets": [
                {"name": "devkey.exe", "browser_download_url": "wrong"},
                {"name": "cardboard-capital.EXE", "browser_download_url": "right"}]}]"#,
        )
        .unwrap();
        assert_eq!(pick(&rel, UpdateChannel::Stable, &v("1.0.0")).unwrap().asset_url, "right");
    }

    #[test]
    fn staged_next_to_the_exe() {
        let (new, old) = staging(Path::new(r"D:\Apps\Cardboard-Capital.exe"));
        assert_eq!(new, Path::new(r"D:\Apps\Cardboard-Capital.exe.new"));
        assert_eq!(old, Path::new(r"D:\Apps\Cardboard-Capital.exe.old"));
    }

    /// `cargo test -- --ignored live`
    #[test]
    #[ignore]
    fn live_check() {
        let client = crate::scryfall::Client::new();
        let r = check(client.agent(), UpdateChannel::Beta);
        println!("{:?}", r.as_ref().map(|o| o.as_ref().map(|u| (&u.tag, &u.asset_url))));
        assert!(r.is_ok());
    }
}
