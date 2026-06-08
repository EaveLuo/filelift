//! Background-ish update notifications, inspired by the Codex CLI.
//!
//! On each run filelift compares the running version against the latest GitHub
//! release and, when a newer one exists, prints a one-line notice to stderr. The
//! latest version is cached in `~/.filelift/version_check.json` and only
//! refreshed over the network at most once per [`CHECK_INTERVAL_MS`], and only
//! from an interactive terminal, so scripts and CI stay fast and quiet.
//!
//! Set `FILELIFT_NO_UPDATE_CHECK` to disable the feature entirely.

use std::{
    io::IsTerminal,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::{i18n, output, target::filelift_home_dir};

const REPO: &str = "EaveLuo/filelift";
const CACHE_FILE: &str = "version_check.json";
const CHECK_INTERVAL_MS: u128 = 24 * 60 * 60 * 1000;
const FETCH_TIMEOUT_MS: u64 = 1500;

/// Environment variable that disables the update check entirely.
pub const OPT_OUT_ENV: &str = "FILELIFT_NO_UPDATE_CHECK";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct VersionCache {
    #[serde(default)]
    last_checked_ms: u128,
    #[serde(default)]
    latest_version: Option<String>,
}

/// Prints an upgrade notice to stderr when a newer release is known. Best
/// effort: any error (no cache, offline, parse failure) is silently ignored.
pub fn maybe_print_notice(current: &str) {
    if std::env::var_os(OPT_OUT_ENV).is_some() {
        return;
    }

    let Ok(path) = cache_path() else {
        return;
    };

    let mut cache = load_cache(&path);

    if std::io::stderr().is_terminal() && is_stale(cache.last_checked_ms, now_ms()) {
        let fetched = fetch_latest_version();
        cache.last_checked_ms = now_ms();
        if fetched.is_some() {
            cache.latest_version = fetched;
        }
        let _ = save_cache(&path, &cache);
    }

    if let Some(latest) = &cache.latest_version
        && is_newer(latest, current)
    {
        anstream::eprintln!(
            "{}",
            output::info(&i18n::t_args(
                "update-available",
                &[
                    ("current", current),
                    ("latest", latest),
                    ("command", "filelift upgrade"),
                ],
            ))
        );
    }
}

fn cache_path() -> Result<PathBuf> {
    Ok(filelift_home_dir()?.join(CACHE_FILE))
}

fn load_cache(path: &PathBuf) -> VersionCache {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|content| serde_json::from_str(&content).ok())
        .unwrap_or_default()
}

fn save_cache(path: &PathBuf, cache: &VersionCache) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_string(cache)?)?;
    Ok(())
}

fn fetch_latest_version() -> Option<String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_millis(FETCH_TIMEOUT_MS))
        .build();
    let body = agent
        .get(&url)
        .set("User-Agent", "filelift-cli")
        .set("Accept", "application/vnd.github+json")
        .call()
        .ok()?
        .into_string()
        .ok()?;

    let json: serde_json::Value = serde_json::from_str(&body).ok()?;
    let tag = json.get("tag_name")?.as_str()?;
    let version = tag.trim().trim_start_matches('v').trim();
    if version.is_empty() {
        None
    } else {
        Some(version.to_string())
    }
}

fn is_stale(last_checked_ms: u128, now_ms: u128) -> bool {
    now_ms.saturating_sub(last_checked_ms) >= CHECK_INTERVAL_MS
}

/// Returns true when `latest` is a strictly higher semantic version than
/// `current`. Unparseable versions are treated as "not newer".
fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_version(latest), parse_version(current)) {
        (Some(latest), Some(current)) => latest > current,
        _ => false,
    }
}

fn parse_version(value: &str) -> Option<semver::Version> {
    semver::Version::parse(value.trim().trim_start_matches('v')).ok()
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_patch_minor_and_major_are_detected() {
        assert!(is_newer("0.2.5", "0.2.4"));
        assert!(is_newer("0.3.0", "0.2.4"));
        assert!(is_newer("1.0.0", "0.9.9"));
    }

    #[test]
    fn same_or_older_is_not_newer() {
        assert!(!is_newer("0.2.4", "0.2.4"));
        assert!(!is_newer("0.2.3", "0.2.4"));
    }

    #[test]
    fn leading_v_is_tolerated() {
        assert!(is_newer("v0.3.0", "0.2.4"));
        assert!(!is_newer("v0.2.4", "v0.2.4"));
    }

    #[test]
    fn unparseable_versions_are_not_newer() {
        assert!(!is_newer("not-a-version", "0.2.4"));
        assert!(!is_newer("0.3.0", "garbage"));
    }

    #[test]
    fn prerelease_is_older_than_release() {
        assert!(!is_newer("0.3.0-rc.1", "0.3.0"));
        assert!(is_newer("0.3.0", "0.3.0-rc.1"));
    }

    #[test]
    fn staleness_uses_interval() {
        let now = 10 * CHECK_INTERVAL_MS;
        assert!(is_stale(0, now));
        assert!(is_stale(now - CHECK_INTERVAL_MS, now));
        assert!(!is_stale(now - 1, now));
        assert!(!is_stale(now, now));
    }

    #[test]
    fn cache_roundtrips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(CACHE_FILE);
        let cache = VersionCache {
            last_checked_ms: 123,
            latest_version: Some("9.9.9".to_string()),
        };
        save_cache(&path, &cache).unwrap();

        let loaded = load_cache(&path);
        assert_eq!(loaded.last_checked_ms, 123);
        assert_eq!(loaded.latest_version.as_deref(), Some("9.9.9"));
    }

    #[test]
    fn loading_missing_cache_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let cache = load_cache(&dir.path().join("absent.json"));
        assert_eq!(cache.last_checked_ms, 0);
        assert!(cache.latest_version.is_none());
    }
}
