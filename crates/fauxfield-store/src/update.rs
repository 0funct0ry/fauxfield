//! Opt-in update check (M19). Off by default (`check_for_updates = false` in settings.toml);
//! never downloads or installs anything: it only learns the latest release tag and reports it.
//! The release lookup shells out to `curl` so no HTTP client is linked for it.

use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

pub const LATEST_URL: &str = "https://api.github.com/repos/0funct0ry/fauxfield/releases/latest";
pub const RELEASES_PAGE: &str = "https://github.com/0funct0ry/fauxfield/releases/latest";
/// Cached answers are reused for this long.
pub const CACHE_TTL_SECS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateInfo {
    pub current: String,
    pub latest: String,
    pub update_available: bool,
    pub url: String,
    /// Unix seconds when the release was looked up.
    pub checked_at: u64,
}

/// Parse `1.2.3`, `v1.2.3` or `1.2.3-rc.1` into (major, minor, patch, is_stable).
fn parse(v: &str) -> Option<(u64, u64, u64, bool)> {
    let v = v.trim().trim_start_matches('v');
    let (core, pre) = match v.split_once(['-', '+']) {
        Some((c, _)) => (c, false),
        None => (v, true),
    };
    let mut it = core.split('.');
    let a = it.next()?.parse().ok()?;
    let b = it.next()?.parse().ok()?;
    let c = it.next()?.parse().ok()?;
    if it.next().is_some() {
        return None;
    }
    Some((a, b, c, pre))
}

/// True when `latest` is strictly newer than `current`. A pre-release sorts below its release.
/// Unparseable versions never report an update.
pub fn is_newer(current: &str, latest: &str) -> bool {
    match (parse(current), parse(latest)) {
        (Some(c), Some(l)) => l > c,
        _ => false,
    }
}

/// Whether the user opted in (`check_for_updates = true`). Missing or unreadable settings: off.
pub fn enabled() -> bool {
    crate::settings::Settings::load_default()
        .ok()
        .and_then(|s| s.check_for_updates)
        .unwrap_or(false)
}

/// Where the last answer is cached (`<data dir>/update-check.json`).
pub fn cache_path() -> Option<std::path::PathBuf> {
    if let Some(d) = std::env::var_os("FAUXFIELD_DATA_DIR").filter(|d| !d.is_empty()) {
        return Some(std::path::PathBuf::from(d).join("update-check.json"));
    }
    crate::settings::settings_path()?
        .parent()
        .map(|p| p.join("update-check.json"))
}

pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn build_info(current: &str, latest_tag: &str, now: u64) -> UpdateInfo {
    let latest = latest_tag.trim_start_matches('v').to_string();
    UpdateInfo {
        update_available: is_newer(current, &latest),
        current: current.to_string(),
        latest,
        url: RELEASES_PAGE.to_string(),
        checked_at: now,
    }
}

/// Extract `tag_name` from the GitHub "latest release" JSON.
pub fn tag_from_json(body: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()?
        .get("tag_name")?
        .as_str()
        .map(str::to_string)
}

/// Fetch the latest release tag with `curl`. Errors are plain strings for display.
pub fn fetch_latest_tag() -> Result<String, String> {
    let out = Command::new("curl")
        .args([
            "-fsSL",
            "--max-time",
            "10",
            "-H",
            "Accept: application/vnd.github+json",
        ])
        .args(["-A", "fauxfield-update-check", LATEST_URL])
        .output()
        .map_err(|e| format!("could not run curl: {e}"))?;
    if !out.status.success() {
        return Err("could not reach the releases page".into());
    }
    tag_from_json(&String::from_utf8_lossy(&out.stdout))
        .ok_or_else(|| "unexpected response from the releases API".into())
}

pub fn read_cache(path: &Path) -> Option<UpdateInfo> {
    serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
}

pub fn write_cache(path: &Path, info: &UpdateInfo) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(s) = serde_json::to_string(info) {
        let _ = std::fs::write(path, s);
    }
}

/// Cached answer if fresh enough, else a network lookup (cached on success). `force` skips the
/// cache. `current` always comes from the running binary, so a stale cache is re-evaluated.
pub fn check(current: &str, cache: Option<&Path>, force: bool) -> Result<UpdateInfo, String> {
    let now = now_secs();
    if !force
        && let Some(c) = cache.and_then(read_cache)
        && now.saturating_sub(c.checked_at) < CACHE_TTL_SECS
    {
        return Ok(build_info(current, &c.latest, c.checked_at));
    }
    let info = build_info(current, &fetch_latest_tag()?, now);
    if let Some(p) = cache {
        write_cache(p, &info);
    }
    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("1.0.0", "1.0.1"));
        assert!(is_newer("0.9.9", "v1.0.0"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("1.2.0", "1.1.9"));
        assert!(is_newer("1.0.0-rc.1", "1.0.0"));
        assert!(!is_newer("1.0.0", "1.0.0-rc.1"));
        assert!(!is_newer("1.0.0", "garbage"));
    }

    #[test]
    fn parses_tag() {
        assert_eq!(
            tag_from_json(r#"{"tag_name":"v1.2.3"}"#).as_deref(),
            Some("v1.2.3")
        );
        assert_eq!(tag_from_json("nope"), None);
    }

    #[test]
    fn fresh_cache_skips_network() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("update.json");
        write_cache(&p, &build_info("1.0.0", "v1.1.0", now_secs()));
        let i = check("1.0.0", Some(&p), false).unwrap();
        assert!(i.update_available);
        assert_eq!(i.latest, "1.1.0");
        // A newer running binary re-evaluates against the cached latest.
        assert!(!check("1.1.0", Some(&p), false).unwrap().update_available);
    }
}
