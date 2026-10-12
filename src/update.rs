//! Checks GitHub Releases for a newer version on the user's channel, and swaps the
//! running exe for the new one.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use semver::Version;
use serde::Deserialize;

use crate::model::UpdateChannel;

pub const REPO: &str = "JaysTea/cardboard-capital";

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
            let asset = r.assets.iter().find(|a| a.name.to_lowercase().ends_with(".exe"))?;
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

fn update_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("update")
}

/// Removes the previous exe left behind by the last update (it can't be deleted while running).
pub fn cleanup(data_dir: &Path) {
    let _ = std::fs::remove_file(update_dir(data_dir).join("old.exe"));
    let _ = std::fs::remove_file(update_dir(data_dir).join("new.exe"));
}

/// Downloads the update and puts it in place of the running exe. Returns the exe path to launch.
/// The running exe is moved to `data\update\old.exe` (Windows allows renaming a running exe)
/// and deleted on the next start. Data and exports aren't touched.
pub fn install(agent: &ureq::Agent, info: &UpdateInfo, data_dir: &Path) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("Can't find the running app: {e}"))?;
    let dir = update_dir(data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Can't create {}: {e}", dir.display()))?;
    let new_path = dir.join("new.exe");
    let old_path = dir.join("old.exe");

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
        return Err("The downloaded file doesn't look like a complete app. Nothing was changed.".into());
    }
    std::fs::write(&new_path, &bytes).map_err(|e| format!("Can't save the download: {e}"))?;

    let _ = std::fs::remove_file(&old_path);
    std::fs::rename(&exe, &old_path).map_err(|e| format!("Can't move the current app aside: {e}"))?;
    if let Err(e) = std::fs::rename(&new_path, &exe) {
        let _ = std::fs::rename(&old_path, &exe); // put the original back
        return Err(format!("Can't put the new version in place: {e}"));
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
