//! Release index lookups: what versions of a runtime actually exist upstream.
//!
//! Needed to resolve a *range* the way nvm, volta and mise do — to the newest
//! release that satisfies it. Resolving `>=20` to `20.0.0` (the old behaviour)
//! hands the user a runtime with a year of known CVEs.
//!
//! # Cost control
//!
//! Enumerating releases costs a network request, so it is avoided wherever
//! possible:
//!
//! * An **exact pin** (`20.11.0`, the overwhelmingly common case) never reaches
//!   this module. Resolution stays offline and instant.
//! * Results are cached on disk for [`INDEX_TTL_SECS`], so repeated runs in a
//!   day cost nothing.
//! * A lookup failure is **not fatal**. Callers fall back to the lowest
//!   satisfying version, so an offline or air-gapped machine keeps working
//!   rather than being unable to resolve a range at all.

use crate::cache;
use crate::version::Version;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

/// How long a cached release index stays fresh (6 hours).
///
/// Shorter than the 24h asset cache: a stale list here means silently *not*
/// picking up a release that already exists, which is the whole point of
/// latest-matching resolution.
pub const INDEX_TTL_SECS: u64 = 6 * 60 * 60;

/// Node publishes a complete release index as a single small JSON document.
const NODE_INDEX_URL: &str = "https://nodejs.org/dist/index.json";

/// Number of python-build-standalone release pages to scan when enumerating
/// versions. Ten pages of ten releases covers well over a year of releases.
const PYTHON_PAGES: u32 = 10;

/// On-disk cache envelope.
#[derive(Debug, Deserialize, serde::Serialize)]
struct CachedIndex {
    fetched_at_secs: u64,
    versions: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct NodeRelease {
    version: String,
}

#[derive(Debug, Deserialize)]
struct PythonRelease {
    #[serde(default)]
    assets: Vec<PythonAssetName>,
}

#[derive(Debug, Deserialize)]
struct PythonAssetName {
    name: String,
}

/// Every version of `tool` available upstream, newest last.
///
/// Consults the on-disk cache first. A network failure propagates so the caller
/// can decide whether to degrade gracefully.
pub fn available_versions(tool: &str, platform: &str) -> Result<Vec<Version>> {
    let path = index_cache_path(tool, platform)?;

    if let Some(versions) = read_cache(&path, now_secs()) {
        return Ok(versions);
    }

    let versions = match tool {
        "node" => fetch_node_versions()?,
        "python" => fetch_python_versions(platform)?,
        other => anyhow::bail!("No release index available for runtime `{other}`"),
    };

    write_cache(&path, &versions);
    Ok(versions)
}

/// Path of the cached index for `tool`.
///
/// Python asset availability varies by platform (a version may be published for
/// linux before macOS), so the platform is part of the key. Node ships every
/// platform in one release, but keying uniformly keeps the layout simple.
fn index_cache_path(tool: &str, platform: &str) -> Result<PathBuf> {
    Ok(cache::runx_home()?
        .join("registry")
        .join(format!("{tool}-{platform}.json")))
}

fn fetch_node_versions() -> Result<Vec<Version>> {
    let body = crate::http::get(NODE_INDEX_URL)
        .call()
        .with_context(|| format!("Failed to fetch the Node release index from {NODE_INDEX_URL}"))?
        .into_string()
        .context("Failed to read the Node release index")?;

    Ok(parse_node_index(&body))
}

/// Extract versions from Node's `index.json`.
///
/// Unparseable entries are skipped rather than failing the whole lookup: one odd
/// record should not make every range unresolvable.
fn parse_node_index(body: &str) -> Vec<Version> {
    let releases: Vec<NodeRelease> = match serde_json::from_str(body) {
        Ok(releases) => releases,
        Err(_) => return Vec::new(),
    };

    sorted_unique(
        releases
            .iter()
            .filter_map(|release| Version::parse(release.version.trim_start_matches(['v', 'V']))),
    )
}

fn fetch_python_versions(platform: &str) -> Result<Vec<Version>> {
    let mut found: Vec<Version> = Vec::new();

    for page in 1..=PYTHON_PAGES {
        let url = format!(
            "https://api.github.com/repos/astral-sh/python-build-standalone/releases?per_page=10&page={page}"
        );
        let body = crate::http::get(&url)
            .call()
            .with_context(|| format!("Failed to fetch python-build-standalone releases: {url}"))?
            .into_string()
            .context("Failed to read python-build-standalone release metadata")?;

        let releases: Vec<PythonRelease> = serde_json::from_str(&body)
            .context("Failed to decode python-build-standalone release metadata")?;
        if releases.is_empty() {
            break;
        }

        found.extend(collect_python_versions(&releases, platform));
    }

    Ok(sorted_unique(found.into_iter()))
}

/// Pull versions out of python-build-standalone asset names.
///
/// Only `install_only` archives for `platform` count: those are the builds runx
/// installs, and a version present for another platform or build flavour would
/// resolve to a download that does not exist.
fn collect_python_versions(releases: &[PythonRelease], platform: &str) -> Vec<Version> {
    let mut versions = Vec::new();
    for release in releases {
        for asset in &release.assets {
            let name = &asset.name;
            if !name.contains(platform)
                || !name.contains("install_only")
                || !name.ends_with(".tar.gz")
            {
                continue;
            }
            if let Some(version) = python_version_from_asset(name) {
                versions.push(version);
            }
        }
    }
    versions
}

/// `cpython-3.11.7+20240107-x86_64-...` -> `3.11.7`
fn python_version_from_asset(name: &str) -> Option<Version> {
    let rest = name.strip_prefix("cpython-")?;
    let version = rest.split(['+', '-']).next()?;
    Version::parse(version)
}

/// Sort ascending and drop duplicates.
fn sorted_unique(versions: impl Iterator<Item = Version>) -> Vec<Version> {
    let unique: BTreeSet<Version> = versions.collect();
    unique.into_iter().collect()
}

/// Read a still-fresh cached index.
fn read_cache(path: &Path, now: u64) -> Option<Vec<Version>> {
    let raw = fs::read_to_string(path).ok()?;
    let cached: CachedIndex = serde_json::from_str(&raw).ok()?;

    if now.saturating_sub(cached.fetched_at_secs) >= INDEX_TTL_SECS {
        return None;
    }

    let versions: Vec<Version> = cached
        .versions
        .iter()
        .filter_map(|raw| Version::parse(raw))
        .collect();

    // An entry that parsed to nothing is treated as a miss rather than as
    // "no versions exist", which would make every range unresolvable.
    (!versions.is_empty()).then_some(versions)
}

/// Persist an index. Failures are non-fatal: the cache is an optimisation.
fn write_cache(path: &Path, versions: &[Version]) {
    let payload = CachedIndex {
        fetched_at_secs: now_secs(),
        versions: versions.iter().map(Version::to_string).collect(),
    };

    let Ok(serialized) = serde_json::to_string(&payload) else {
        return;
    };
    if let Some(parent) = path.parent() {
        if fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    let _ = fs::write(path, serialized);
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(raw: &str) -> Version {
        Version::parse(raw).expect("valid version")
    }

    // ── Node index parsing ───────────────────────────────────────────────────

    #[test]
    fn parses_node_index_and_sorts_ascending() {
        let body = r#"[
            {"version":"v22.1.0","lts":false},
            {"version":"v20.11.0","lts":"Iron"},
            {"version":"v18.20.3","lts":"Hydrogen"}
        ]"#;

        let versions = parse_node_index(body);
        assert_eq!(versions, vec![v("18.20.3"), v("20.11.0"), v("22.1.0")]);
    }

    #[test]
    fn node_index_skips_unparseable_entries() {
        let body = r#"[
            {"version":"v20.11.0"},
            {"version":"nightly"},
            {"version":"v22.1.0"}
        ]"#;

        let versions = parse_node_index(body);
        assert_eq!(
            versions,
            vec![v("20.11.0"), v("22.1.0")],
            "one bad record must not discard the rest"
        );
    }

    #[test]
    fn node_index_handles_garbage_without_panicking() {
        assert!(parse_node_index("<html>503</html>").is_empty());
        assert!(parse_node_index("").is_empty());
    }

    /// Node's index contains very old 0.x releases; ordering must stay numeric.
    #[test]
    fn node_index_orders_numerically_not_lexicographically() {
        let body = r#"[{"version":"v0.10.48"},{"version":"v9.11.2"},{"version":"v10.0.0"}]"#;
        let versions = parse_node_index(body);
        assert_eq!(versions, vec![v("0.10.48"), v("9.11.2"), v("10.0.0")]);
    }

    // ── Python asset parsing ─────────────────────────────────────────────────

    #[test]
    fn extracts_python_version_from_asset_name() {
        assert_eq!(
            python_version_from_asset(
                "cpython-3.11.7+20240107-x86_64-unknown-linux-gnu-install_only.tar.gz"
            ),
            Some(v("3.11.7"))
        );
    }

    #[test]
    fn ignores_non_cpython_asset_names() {
        assert_eq!(python_version_from_asset("README.md"), None);
        assert_eq!(python_version_from_asset("cpython-nightly+x.tar.gz"), None);
    }

    #[test]
    fn collects_only_matching_platform_and_install_only_builds() {
        let json = r#"[{"assets":[
            {"name":"cpython-3.11.7+2024-x86_64-unknown-linux-gnu-install_only.tar.gz"},
            {"name":"cpython-3.12.1+2024-x86_64-unknown-linux-gnu-install_only.tar.gz"},
            {"name":"cpython-3.13.0+2024-aarch64-apple-darwin-install_only.tar.gz"},
            {"name":"cpython-3.9.1+2024-x86_64-unknown-linux-gnu-debug-full.tar.zst"}
        ]}]"#;
        let releases: Vec<PythonRelease> = serde_json::from_str(json).expect("fixture parses");

        let versions = collect_python_versions(&releases, "x86_64-unknown-linux-gnu");
        let versions = sorted_unique(versions.into_iter());

        assert_eq!(
            versions,
            vec![v("3.11.7"), v("3.12.1")],
            "only install_only builds for the requested platform should count"
        );
    }

    #[test]
    fn deduplicates_versions_across_releases() {
        let json = r#"[
            {"assets":[{"name":"cpython-3.11.7+a-x86_64-unknown-linux-gnu-install_only.tar.gz"}]},
            {"assets":[{"name":"cpython-3.11.7+b-x86_64-unknown-linux-gnu-install_only.tar.gz"}]}
        ]"#;
        let releases: Vec<PythonRelease> = serde_json::from_str(json).expect("fixture parses");

        let versions = sorted_unique(
            collect_python_versions(&releases, "x86_64-unknown-linux-gnu").into_iter(),
        );
        assert_eq!(versions, vec![v("3.11.7")]);
    }

    // ── Disk cache ───────────────────────────────────────────────────────────

    #[test]
    fn cache_round_trips_within_the_ttl() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("node.json");

        write_cache(&path, &[v("20.11.0"), v("22.1.0")]);
        let loaded = read_cache(&path, now_secs()).expect("fresh cache should hit");

        assert_eq!(loaded, vec![v("20.11.0"), v("22.1.0")]);
    }

    #[test]
    fn expired_cache_is_a_miss() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("node.json");
        write_cache(&path, &[v("20.11.0")]);

        let long_after = now_secs() + INDEX_TTL_SECS + 1;
        assert!(
            read_cache(&path, long_after).is_none(),
            "a stale index must be refetched, or new releases are never seen"
        );
    }

    #[test]
    fn corrupt_or_missing_cache_is_a_miss() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("node.json");

        assert!(read_cache(&path, now_secs()).is_none(), "missing file");

        fs::write(&path, b"{not json").unwrap();
        assert!(read_cache(&path, now_secs()).is_none(), "corrupt file");
    }

    /// A cache whose entries all fail to parse must be treated as a miss, not as
    /// "this runtime has no releases".
    #[test]
    fn cache_with_no_usable_entries_is_a_miss() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("node.json");
        fs::write(
            &path,
            serde_json::to_string(&CachedIndex {
                fetched_at_secs: now_secs(),
                versions: vec!["nightly".to_string()],
            })
            .unwrap(),
        )
        .unwrap();

        assert!(read_cache(&path, now_secs()).is_none());
    }

    #[test]
    fn unknown_runtime_has_no_index() {
        assert!(available_versions("ruby", "x86_64").is_err());
    }
}
