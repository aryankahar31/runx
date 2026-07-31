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

/// Go publishes every stable release, with digests, in one JSON document.
///
/// `include=all` is required: without it go.dev only lists the latest couple
/// of minor versions, so an older `go.mod` directive (e.g. `go 1.22`) would
/// resolve to nothing.
pub const GO_INDEX_URL: &str = "https://go.dev/dl/?mode=json&include=all";

/// How many pages of Bun releases to scan. `per_page=100` and a page that
/// comes back empty stops the scan, so this is a generous ceiling rather than
/// a fixed cost.
const BUN_PAGES: u32 = 5;

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

/// One entry of Go's `dl/?mode=json` release index.
///
/// Shared with [`crate::runtime`], which reads the per-file digests from the
/// same document to verify Go downloads.
#[derive(Debug, Deserialize)]
pub struct GoRelease {
    pub version: String,
    pub stable: bool,
    #[serde(default)]
    pub files: Vec<GoFile>,
}

#[derive(Debug, Deserialize)]
pub struct GoFile {
    pub filename: String,
    #[serde(default)]
    pub sha256: String,
}

#[derive(Debug, Deserialize)]
struct BunRelease {
    tag_name: String,
}

/// How a version *range* is turned into a concrete release.
///
/// Exact pins are unaffected by this setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Resolution {
    /// Newest release satisfying the range — what nvm, volta and mise do, and
    /// the default. Resolving `>=20` to `20.0.0` would hand the user a runtime
    /// with a year of known CVEs.
    #[default]
    Latest,
    /// Lowest release satisfying the range. Fully offline and time-independent,
    /// which is why it remains available as a strict mode.
    Minimum,
}

impl Resolution {
    /// Read the mode from `RUNX_RESOLUTION`.
    ///
    /// An unrecognised value falls back to the default rather than failing: a
    /// typo in an environment variable should not make runx unusable.
    pub fn from_env() -> Self {
        match std::env::var("RUNX_RESOLUTION") {
            Ok(value) => Self::parse(&value).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Parse a mode name, case-insensitively.
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "latest" => Some(Self::Latest),
            "minimum" | "min" | "lowest" => Some(Self::Minimum),
            _ => None,
        }
    }
}

/// The outcome of resolving one requirement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chosen {
    /// The concrete `MAJOR.MINOR.PATCH` version to install.
    pub version: String,
    /// True when the requirement was a range rather than an exact pin.
    pub was_range: bool,
    /// Set when resolution did not go as intended, e.g. the release list could
    /// not be fetched and a lower bound was used instead.
    pub note: Option<String>,
}

/// Pick a concrete version for `requirement`.
///
/// Pure policy, separated from the network so it can be tested exhaustively:
/// `available` is `None` when no release list could be obtained.
///
/// Falling back to the lowest satisfying version on a failed lookup is
/// deliberate. An offline or air-gapped machine, or a GitHub API rate limit,
/// should degrade to the old behaviour with a warning rather than refuse to
/// resolve a range at all.
pub fn choose_version(
    requirement: &str,
    mode: Resolution,
    available: Option<&[Version]>,
) -> Result<Chosen> {
    let req = crate::version::Req::parse(requirement).ok_or_else(|| {
        anyhow::anyhow!(
            "`{requirement}` is not a recognised version or range \
             (aliases like `lts/*` are not supported)"
        )
    })?;

    // An exact pin needs no release list, so this path stays offline.
    if req.exact {
        let version = req
            .minimum()
            .ok_or_else(|| anyhow::anyhow!("Could not determine a version from `{requirement}`"))?;
        return Ok(Chosen {
            version: version.to_three_parts(),
            was_range: false,
            note: None,
        });
    }

    let minimum = req.minimum();

    if mode == Resolution::Minimum {
        let version = minimum.ok_or_else(|| {
            anyhow::anyhow!(
                "`{requirement}` has no lowest satisfying version; \
                 use the default `latest` resolution to pick from published releases"
            )
        })?;
        return Ok(Chosen {
            version: version.to_three_parts(),
            was_range: true,
            note: None,
        });
    }

    match available {
        Some(versions) => match req.best_match(versions) {
            Some(best) => Ok(Chosen {
                version: best.to_three_parts(),
                was_range: true,
                note: None,
            }),
            // The list was fetched but nothing in it satisfies the range.
            None => Err(anyhow::anyhow!(
                "No published release satisfies `{requirement}`"
            )),
        },
        None => {
            let version = minimum.ok_or_else(|| {
                anyhow::anyhow!(
                    "Could not fetch the release list, and `{requirement}` has no \
                     lowest satisfying version to fall back to"
                )
            })?;

            // An open-ended requirement such as `*` or `>=0` has a nominal floor
            // of 0.0.0, which no runtime publishes. Falling back to it would
            // produce a baffling 404 rather than an explanation.
            if version.to_three_parts() == "0.0.0" {
                anyhow::bail!(
                    "Could not fetch the release list, and `{requirement}` is too \
                     open-ended to resolve offline. Pin a version or a bounded \
                     range (e.g. `^20`)."
                );
            }

            Ok(Chosen {
                version: version.to_three_parts(),
                was_range: true,
                note: Some(format!(
                    "could not fetch the release list; using the lowest version \
                     satisfying `{requirement}`"
                )),
            })
        }
    }
}

/// Resolve `requirement` for `tool`, fetching the release list when needed.
pub fn resolve_requirement(tool: &str, requirement: &str, mode: Resolution) -> Result<Chosen> {
    // Avoid the network entirely for exact pins and for strict mode.
    if mode == Resolution::Minimum || is_exact(requirement) {
        return choose_version(requirement, mode, None);
    }

    let platform = crate::runtime::registry_platform(tool)?;
    let available = available_versions(tool, &platform).ok();
    choose_version(requirement, mode, available.as_deref())
}

/// True when `requirement` pins a single version, needing no release list.
fn is_exact(requirement: &str) -> bool {
    crate::version::Req::parse(requirement).is_some_and(|req| req.exact)
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
        "bun" => fetch_bun_versions()?,
        "go" => fetch_go_versions()?,
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

/// Pull versions out of Bun's GitHub release tags (`bun-v1.3.14`).
///
/// Bun publishes every supported platform for every release, so the platform
/// does not need to filter the list. Canary and pre-release tags
/// (`bun-v1.3.14-canary.x`) fail to parse as versions and are skipped.
fn fetch_bun_versions() -> Result<Vec<Version>> {
    let mut found: Vec<Version> = Vec::new();

    for page in 1..=BUN_PAGES {
        let url = format!(
            "https://api.github.com/repos/oven-sh/bun/releases?per_page=100&page={page}"
        );
        let body = crate::http::get(&url)
            .call()
            .with_context(|| format!("Failed to fetch Bun releases: {url}"))?
            .into_string()
            .context("Failed to read Bun release metadata")?;

        let releases: Vec<BunRelease> = serde_json::from_str(&body)
            .context("Failed to decode Bun release metadata")?;
        if releases.is_empty() {
            break;
        }

        for release in releases {
            if let Some(version) = version_from_bun_tag(&release.tag_name) {
                found.push(version);
            }
        }
    }

    Ok(sorted_unique(found.into_iter()))
}

/// `bun-v1.3.14` -> `1.3.14`; `None` for canary, pre-release and malformed tags.
fn version_from_bun_tag(tag: &str) -> Option<Version> {
    let version = tag.strip_prefix("bun-v")?;
    Version::parse(version).filter(|parsed| parsed.to_three_parts() == version)
}

/// Pull versions out of Go's `dl/?mode=json` release index.
///
/// Only plain `goMAJOR.MINOR.PATCH` stable releases count: pre-releases
/// (`go1.27rc1`) and the two-part aliases Go lists alongside full releases
/// (`go1.25`) are not installable downloads runx resolves to.
fn fetch_go_versions() -> Result<Vec<Version>> {
    let body = crate::http::get(GO_INDEX_URL)
        .call()
        .with_context(|| format!("Failed to fetch the Go release index from {GO_INDEX_URL}"))?
        .into_string()
        .context("Failed to read the Go release index")?;

    let releases: Vec<GoRelease> = serde_json::from_str(&body)
        .context("Failed to decode the Go release index")?;

    Ok(go_versions_from_index(&releases))
}

/// Extract the stable, full `MAJOR.MINOR.PATCH` versions from a parsed index.
fn go_versions_from_index(releases: &[GoRelease]) -> Vec<Version> {
    sorted_unique(releases.iter().filter_map(|release| {
        if !release.stable {
            return None;
        }
        let version = release.version.strip_prefix("go")?;
        Version::parse(version).filter(|parsed| parsed.to_three_parts() == version)
    }))
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

    // ── Bun index parsing ────────────────────────────────────────────────────

    #[test]
    fn parses_bun_release_tags() {
        assert_eq!(
            version_from_bun_tag("bun-v1.3.14").expect("stable tag parses"),
            v("1.3.14")
        );
        assert_eq!(
            version_from_bun_tag("bun-v1.0.0").expect("oldest stable parses"),
            v("1.0.0")
        );
    }

    /// Canary, pre-release and unrelated tags must never become installable
    /// versions: a canary build is not what a stable range should resolve to.
    #[test]
    fn ignores_non_stable_bun_tags() {
        for tag in [
            "bun-v1.3.14-canary.123",
            "bun-v1.3.14-rc.1",
            "v1.3.14",
            "bun-v1.3",
            "nightly",
        ] {
            assert_eq!(
                version_from_bun_tag(tag),
                None,
                "{tag} must not count as a stable release"
            );
        }
    }

    #[test]
    fn bun_versions_are_deduplicated_and_sorted() {
        let releases = vec![
            BunRelease {
                tag_name: "bun-v1.3.14".to_string(),
            },
            BunRelease {
                tag_name: "bun-v1.3.14".to_string(),
            },
            BunRelease {
                tag_name: "bun-v1.2.0".to_string(),
            },
        ];

        let versions = sorted_unique(
            releases
                .iter()
                .filter_map(|release| version_from_bun_tag(&release.tag_name)),
        );
        assert_eq!(versions, vec![v("1.2.0"), v("1.3.14")]);
    }

    // ── Go index parsing ──────────────────────────────────────────────────────

    #[test]
    fn keeps_only_stable_full_versions_from_the_go_index() {
        let releases = vec![
            GoRelease {
                version: "go1.26.5".to_string(),
                stable: true,
                files: vec![],
            },
            GoRelease {
                version: "go1.26.5".to_string(),
                stable: true,
                files: vec![],
            },
            GoRelease {
                version: "go1.27rc1".to_string(),
                stable: false,
                files: vec![],
            },
            // The two-part alias Go lists beside the full release.
            GoRelease {
                version: "go1.25".to_string(),
                stable: true,
                files: vec![],
            },
            GoRelease {
                version: "go1.22.5".to_string(),
                stable: true,
                files: vec![],
            },
        ];

        assert_eq!(
            go_versions_from_index(&releases),
            vec![v("1.22.5"), v("1.26.5")],
            "pre-releases and two-part aliases must not be installable versions"
        );
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

    // ── Resolution policy ────────────────────────────────────────────────────

    fn releases(list: &[&str]) -> Vec<Version> {
        list.iter().map(|raw| v(raw)).collect()
    }

    /// An exact pin must behave identically in both modes and never need a
    /// release list, so the common case stays offline and instant.
    #[test]
    fn exact_pins_are_unaffected_by_mode() {
        for mode in [Resolution::Latest, Resolution::Minimum] {
            let chosen = choose_version("20.11.0", mode, None).expect("exact pin resolves");
            assert_eq!(chosen.version, "20.11.0");
            assert!(!chosen.was_range);
            assert!(chosen.note.is_none());
        }
        assert!(is_exact("20.11.0"));
        assert!(is_exact("==3.11.7"));
        assert!(!is_exact(">=20"));
    }

    /// The flip: a range now resolves to the newest satisfying release, matching
    /// nvm/volta/mise, instead of the lowest.
    #[test]
    fn latest_mode_picks_the_newest_satisfying_release() {
        let available = releases(&["18.20.3", "20.11.0", "20.12.1", "22.1.0"]);

        let chosen =
            choose_version(">=20", Resolution::Latest, Some(&available)).expect("resolves");
        assert_eq!(chosen.version, "22.1.0");
        assert!(chosen.was_range);
        assert!(chosen.note.is_none(), "a clean resolution needs no note");
    }

    #[test]
    fn latest_mode_respects_upper_bounds() {
        let available = releases(&["18.20.3", "20.11.0", "20.12.1", "22.1.0"]);

        let chosen = choose_version("^20", Resolution::Latest, Some(&available)).expect("resolves");
        assert_eq!(chosen.version, "20.12.1", "^20 must not cross into 22");
    }

    #[test]
    fn minimum_mode_still_picks_the_lowest() {
        let available = releases(&["20.11.0", "22.1.0"]);

        let chosen =
            choose_version(">=20", Resolution::Minimum, Some(&available)).expect("resolves");
        assert_eq!(chosen.version, "20.0.0");
        assert!(chosen.was_range);
    }

    /// An offline machine or a rate-limited API must degrade to the old
    /// behaviour with a warning, not refuse to resolve.
    #[test]
    fn failed_lookup_falls_back_to_the_lower_bound_with_a_note() {
        let chosen = choose_version(">=3.11", Resolution::Latest, None).expect("should degrade");

        assert_eq!(chosen.version, "3.11.0");
        let note = chosen.note.as_deref().unwrap_or_default();
        assert!(
            note.contains("release list"),
            "the user must be told resolution was degraded, got: {note}"
        );
    }

    /// Ranges with no lower bound were previously unresolvable; with a real
    /// release list they now resolve correctly.
    #[test]
    fn upper_bound_only_range_resolves_against_a_release_list() {
        let available = releases(&["18.20.3", "19.9.0", "20.11.0"]);

        let chosen = choose_version("<20", Resolution::Latest, Some(&available)).expect("resolves");
        assert_eq!(
            chosen.version, "19.9.0",
            "<20 should pick the newest release below 20"
        );
    }

    /// ...but in strict mode there is genuinely no defensible answer, so it must
    /// error rather than invent one (the old code returned 20.0.0 here).
    #[test]
    fn upper_bound_only_range_errors_in_minimum_mode() {
        assert!(choose_version("<20", Resolution::Minimum, None).is_err());
    }

    #[test]
    fn errors_when_no_release_satisfies_the_range() {
        let available = releases(&["18.20.3", "20.11.0"]);
        let err = choose_version(">=24", Resolution::Latest, Some(&available))
            .expect_err("nothing satisfies >=24");
        assert!(format!("{err:#}").contains(">=24"));
    }

    #[test]
    fn unparseable_requirements_error_clearly() {
        for bad in ["lts/iron", "nightly", "", "garbage"] {
            let err =
                choose_version(bad, Resolution::Latest, None).expect_err("should not resolve");
            assert!(
                format!("{err:#}").contains("not a recognised version"),
                "unexpected message for {bad:?}"
            );
        }
    }

    #[test]
    fn alternation_picks_the_newest_across_alternatives() {
        let available = releases(&["16.0.0", "18.20.3", "20.11.0", "22.1.0"]);
        let chosen =
            choose_version("18 || >=20", Resolution::Latest, Some(&available)).expect("resolves");
        assert_eq!(chosen.version, "22.1.0");
    }

    #[test]
    fn python_style_requirements_resolve_to_latest() {
        let available = releases(&["3.10.13", "3.11.7", "3.12.1", "3.13.0"]);

        let chosen =
            choose_version(">=3.11", Resolution::Latest, Some(&available)).expect("resolves");
        assert_eq!(chosen.version, "3.13.0");

        // PEP 440 compatible-release caps the major version.
        let capped =
            choose_version("~=3.11", Resolution::Latest, Some(&available)).expect("resolves");
        assert_eq!(capped.version, "3.13.0");
    }

    // ── Mode parsing ─────────────────────────────────────────────────────────

    #[test]
    fn latest_is_the_default_mode() {
        assert_eq!(Resolution::default(), Resolution::Latest);
    }

    #[test]
    fn parses_mode_names_case_insensitively() {
        assert_eq!(Resolution::parse("latest"), Some(Resolution::Latest));
        assert_eq!(Resolution::parse("LATEST"), Some(Resolution::Latest));
        assert_eq!(Resolution::parse("minimum"), Some(Resolution::Minimum));
        assert_eq!(Resolution::parse(" min "), Some(Resolution::Minimum));
        assert_eq!(Resolution::parse("lowest"), Some(Resolution::Minimum));
    }

    /// A typo in an environment variable must not make runx unusable.
    #[test]
    fn unknown_mode_names_are_rejected_by_parse() {
        assert_eq!(Resolution::parse("newest"), None);
        assert_eq!(Resolution::parse(""), None);
    }
}
