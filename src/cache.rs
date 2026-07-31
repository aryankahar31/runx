use crate::runtime::RuntimeSpec;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

/// Written inside a runtime directory once extraction has fully completed.
///
/// Its presence is what makes an install atomic in practice: a runtime is only
/// ever visible at its canonical path after a complete staging directory (marker
/// included) is renamed into place, so a killed or failed extraction can never
/// leave a half-extracted tree that looks like a valid cache entry.
pub const COMPLETION_MARKER: &str = ".runx-complete.json";

/// Prefix for in-progress staging directories.
///
/// The leading dot keeps them out of `cache list`, and they are never a valid
/// version (versions are digits and dots), so they cannot collide with a real
/// runtime directory.
const STAGING_PREFIX: &str = ".staging-";

/// Distinguishes concurrent staging directories within one process.
static STAGING_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct CachedRuntime {
    pub root: PathBuf,
    pub bin_dirs: Vec<PathBuf>,
}

/// Recorded alongside every completed install.
///
/// The `sha256` and `source_url` fields exist so a lockfile can be generated
/// from what was actually installed rather than by re-resolving.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallReceipt {
    pub tool: String,
    pub version: String,
    pub installed_at_secs: u64,
    pub runx_version: String,
    #[serde(default)]
    pub source_url: String,
    #[serde(default)]
    pub sha256: Option<String>,
}

/// Root of the runx cache.
///
/// `RUNX_HOME` overrides the default so a project or CI job can keep an
/// isolated cache without touching `HOME` (which affects unrelated tooling).
pub fn runx_home() -> Result<PathBuf> {
    if let Some(dir) = env::var_os("RUNX_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(dir));
    }
    let home = dirs::home_dir().context("Could not determine your home directory")?;
    Ok(home.join(".runx"))
}

/// Directory holding all cached runtimes.
pub fn runtimes_dir(home: &Path) -> PathBuf {
    home.join("runtimes")
}

/// Canonical path for one runtime version inside `home`.
pub fn runtime_root_in(home: &Path, tool: &str, version: &str) -> PathBuf {
    runtimes_dir(home).join(tool).join(version)
}

pub fn runtime_root(spec: &RuntimeSpec) -> Result<PathBuf> {
    Ok(runtime_root_in(&runx_home()?, &spec.tool, &spec.version))
}

/// Return the cached runtime for `spec`, if a usable one is installed.
pub fn cached_runtime(spec: &RuntimeSpec) -> Result<Option<CachedRuntime>> {
    cached_runtime_in(&runx_home()?, spec)
}

/// [`cached_runtime`] against an explicit cache root.
///
/// A runtime counts as usable when its expected executable is present. When the
/// completion marker is missing the entry is *adopted* — the marker is written
/// rather than the directory being deleted and re-downloaded.
///
/// Adoption matters for upgrades: every runtime installed by an earlier runx
/// predates the marker, and treating those as corrupt would force a
/// multi-hundred-megabyte re-download for every existing user. Adoption applies
/// exactly the same check the previous version used to accept a cache entry, so
/// it is not a regression in strictness; genuinely truncated legacy installs are
/// what `runx doctor` is for.
pub fn cached_runtime_in(home: &Path, spec: &RuntimeSpec) -> Result<Option<CachedRuntime>> {
    let root = runtime_root_in(home, &spec.tool, &spec.version);
    let exe = expected_executable(&root, spec);
    if !exe.is_file() {
        return Ok(None);
    }

    if !root.join(COMPLETION_MARKER).is_file() {
        // Best effort: an unwritable cache should not block using a runtime
        // that is otherwise present and working.
        let _ = write_receipt(&root, spec, None);
    }

    Ok(Some(CachedRuntime {
        root: root.clone(),
        bin_dirs: absolute_bin_dirs(&root, spec),
    }))
}

/// True when a runtime directory carries a valid completion marker.
pub fn is_complete(root: &Path) -> bool {
    let marker = root.join(COMPLETION_MARKER);
    fs::read_to_string(marker)
        .ok()
        .and_then(|raw| serde_json::from_str::<InstallReceipt>(&raw).ok())
        .is_some()
}

/// Read the install receipt for a runtime directory, if present and valid.
pub fn read_receipt(root: &Path) -> Option<InstallReceipt> {
    let raw = fs::read_to_string(root.join(COMPLETION_MARKER)).ok()?;
    serde_json::from_str(&raw).ok()
}

/// Create a fresh staging directory to extract into.
///
/// The name includes the process id and a per-process counter so two runx
/// processes (or two threads) installing the same runtime never write into each
/// other's tree. This is what makes a lockfile unnecessary: concurrent installs
/// are wasteful but correct, and the final [`commit_runtime`] rename decides a
/// single winner atomically.
pub fn staging_dir(home: &Path, spec: &RuntimeSpec) -> Result<PathBuf> {
    let nonce = STAGING_COUNTER.fetch_add(1, Ordering::Relaxed);
    let unique = format!(
        "{STAGING_PREFIX}{}-{}-{}-{}",
        spec.version,
        process::id(),
        now_secs(),
        nonce
    );
    let dir = runtimes_dir(home).join(&spec.tool).join(unique);

    if dir.exists() {
        fs::remove_dir_all(&dir)
            .with_context(|| format!("Failed to clear staging directory {}", dir.display()))?;
    }
    fs::create_dir_all(&dir)
        .with_context(|| format!("Failed to create staging directory {}", dir.display()))?;
    Ok(dir)
}

/// Promote a fully extracted staging directory to its canonical location.
///
/// Verification happens *before* the rename, so an archive missing its expected
/// executable never replaces a working install. The previous implementation
/// deleted the destination before extracting, which meant a failed download
/// destroyed the existing cache and left a partial tree behind at the real path.
pub fn commit_runtime(
    home: &Path,
    staging: &Path,
    spec: &RuntimeSpec,
    sha256: Option<String>,
) -> Result<CachedRuntime> {
    normalize_runtime(staging, spec)?;

    let exe = expected_executable(staging, spec);
    if !exe.is_file() {
        anyhow::bail!(
            "Downloaded {} {} but did not find the expected executable at {}",
            spec.tool,
            spec.version,
            exe.display()
        );
    }

    write_receipt(staging, spec, sha256)?;

    let final_root = runtime_root_in(home, &spec.tool, &spec.version);
    if let Some(parent) = final_root.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create {}", parent.display()))?;
    }

    // Replace whatever is at the destination. Windows `rename` refuses an
    // existing directory, so it has to go first either way.
    if final_root.exists() {
        fs::remove_dir_all(&final_root).with_context(|| {
            format!(
                "Failed to replace the existing runtime at {}",
                final_root.display()
            )
        })?;
    }

    fs::rename(staging, &final_root).with_context(|| {
        format!(
            "Failed to move {} into place at {}",
            staging.display(),
            final_root.display()
        )
    })?;

    Ok(CachedRuntime {
        root: final_root.clone(),
        bin_dirs: absolute_bin_dirs(&final_root, spec),
    })
}

/// Record an install receipt inside `root`.
pub fn write_receipt(root: &Path, spec: &RuntimeSpec, sha256: Option<String>) -> Result<()> {
    let receipt = InstallReceipt {
        tool: spec.tool.clone(),
        version: spec.version.clone(),
        installed_at_secs: now_secs(),
        runx_version: env!("CARGO_PKG_VERSION").to_string(),
        source_url: spec.url.clone(),
        sha256,
    };
    let serialized = serde_json::to_string_pretty(&receipt)?;
    fs::write(root.join(COMPLETION_MARKER), serialized)
        .with_context(|| format!("Failed to write install marker in {}", root.display()))
}

/// Written next to the completion marker whenever a runtime is used.
///
/// Kept *separate* from the completion marker on purpose. Recording last-use by
/// rewriting the receipt would mean a process killed mid-write could leave
/// unparseable JSON, and [`is_complete`] would then report a perfectly good
/// runtime as incomplete. A dedicated file cannot damage validity: if it is
/// missing or corrupt, callers simply fall back to the install date.
pub const LAST_USED_MARKER: &str = ".runx-last-used";

/// One cached runtime, as reported by `runx cache list`.
#[derive(Debug, Clone)]
pub struct CacheEntry {
    pub tool: String,
    pub version: String,
    pub root: PathBuf,
    pub size_bytes: u64,
    /// False when the completion marker is absent or unparseable.
    pub complete: bool,
    pub installed_at_secs: Option<u64>,
    pub last_used_secs: Option<u64>,
}

impl CacheEntry {
    /// Best available age signal, preferring last use over install date.
    pub fn last_activity_secs(&self) -> Option<u64> {
        self.last_used_secs.or(self.installed_at_secs)
    }

    /// Age in whole days relative to `now`, if any timestamp is known.
    pub fn age_days(&self, now: u64) -> Option<u64> {
        let last = self.last_activity_secs()?;
        Some(now.saturating_sub(last) / 86_400)
    }
}

/// Record that a runtime was just used. Best effort; failures are ignored so a
/// read-only cache (a shared CI mount, for instance) still works.
pub fn touch_last_used(root: &Path) {
    let _ = fs::write(root.join(LAST_USED_MARKER), now_secs().to_string());
}

/// Read the last-used timestamp, if one was recorded and is parseable.
pub fn read_last_used(root: &Path) -> Option<u64> {
    fs::read_to_string(root.join(LAST_USED_MARKER))
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// Every cached runtime under `home`, sorted by tool then version.
///
/// In-progress staging directories are excluded: they are not installed
/// runtimes, and listing them would imply the user could use them.
pub fn list_cached(home: &Path) -> Result<Vec<CacheEntry>> {
    let runtimes = runtimes_dir(home);
    let mut entries = Vec::new();

    let Ok(tools) = fs::read_dir(&runtimes) else {
        // A cache that was never populated is empty, not an error.
        return Ok(entries);
    };

    for tool_dir in tools.flatten() {
        if !tool_dir.path().is_dir() {
            continue;
        }
        let tool = tool_dir.file_name().to_string_lossy().into_owned();

        let Ok(versions) = fs::read_dir(tool_dir.path()) else {
            continue;
        };
        for version_dir in versions.flatten() {
            let root = version_dir.path();
            if !root.is_dir() {
                continue;
            }
            let version = version_dir.file_name().to_string_lossy().into_owned();
            if is_staging_name(&version) {
                continue;
            }

            let receipt = read_receipt(&root);
            entries.push(CacheEntry {
                tool: tool.clone(),
                version,
                size_bytes: directory_size(&root),
                complete: receipt.is_some(),
                installed_at_secs: receipt.map(|r| r.installed_at_secs),
                last_used_secs: read_last_used(&root),
                root,
            });
        }
    }

    entries.sort_by(|a, b| a.tool.cmp(&b.tool).then_with(|| a.version.cmp(&b.version)));
    Ok(entries)
}

/// Abandoned staging directories, left behind by a killed install.
pub fn list_staging(home: &Path) -> Result<Vec<PathBuf>> {
    let runtimes = runtimes_dir(home);
    let mut found = Vec::new();

    let Ok(tools) = fs::read_dir(&runtimes) else {
        return Ok(found);
    };

    for tool_dir in tools.flatten() {
        let Ok(versions) = fs::read_dir(tool_dir.path()) else {
            continue;
        };
        for version_dir in versions.flatten() {
            let name = version_dir.file_name().to_string_lossy().into_owned();
            if is_staging_name(&name) && version_dir.path().is_dir() {
                found.push(version_dir.path());
            }
        }
    }

    found.sort();
    Ok(found)
}

/// How long an abandoned staging directory is left alone before cleanup
/// considers it garbage.
///
/// A staging directory may belong to an install running in *another process*
/// right now. Deleting it would break that install, so cleanup only touches
/// staging directories older than this grace period. An hour is far longer than
/// any runtime download.
pub const STAGING_GRACE_SECS: u64 = 3600;

/// Seconds since `path` was last modified, if that can be determined.
fn dir_age_secs(path: &Path) -> Option<u64> {
    let modified = fs::metadata(path).ok()?.modified().ok()?;
    let secs = modified.duration_since(UNIX_EPOCH).ok()?.as_secs();
    Some(now_secs().saturating_sub(secs))
}

/// Staging directories old enough to be considered abandoned.
///
/// Excludes anything within [`STAGING_GRACE_SECS`] so a concurrent install is
/// never destroyed.
pub fn stale_staging(home: &Path) -> Result<Vec<PathBuf>> {
    Ok(list_staging(home)?
        .into_iter()
        .filter(|path| dir_age_secs(path).is_some_and(|age| age >= STAGING_GRACE_SECS))
        .collect())
}

/// Total size of `path`, following no symlinks.
///
/// `symlink_metadata` keeps link targets from being counted twice — Python
/// runtimes alias `python -> python3.11` — and stops a link out of the tree from
/// inflating the total. Unreadable entries are skipped: a size report must not
/// fail because of one bad permission.
pub fn directory_size(path: &Path) -> u64 {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return 0;
    };

    if metadata.file_type().is_symlink() {
        return metadata.len();
    }
    if metadata.is_file() {
        return metadata.len();
    }

    let Ok(entries) = fs::read_dir(path) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| directory_size(&entry.path()))
        .sum()
}

/// Human-readable byte count using binary units.
pub fn format_size(bytes: u64) -> String {
    const UNITS: [(&str, u64); 4] = [
        ("GiB", 1024 * 1024 * 1024),
        ("MiB", 1024 * 1024),
        ("KiB", 1024),
        ("B", 1),
    ];

    for (label, scale) in UNITS {
        if bytes >= scale {
            // One decimal place except for whole bytes, where it is noise.
            if scale == 1 {
                return format!("{bytes} {label}");
            }
            return format!("{:.1} {label}", bytes as f64 / scale as f64);
        }
    }
    "0 B".to_string()
}

/// Delete a cached runtime directory.
pub fn remove_entry(root: &Path) -> Result<()> {
    fs::remove_dir_all(root).with_context(|| format!("Failed to remove {}", root.display()))
}

/// Remove a staging directory, ignoring failures.
///
/// Called on the error path, where the extraction error is what the user needs
/// to see; a cleanup failure on top of it is noise. Anything left behind is
/// named with [`STAGING_PREFIX`] and collected by `runx cache prune`.
pub fn discard_staging(staging: &Path) {
    let _ = fs::remove_dir_all(staging);
}

/// True when a directory name is an abandoned staging directory.
pub fn is_staging_name(name: &str) -> bool {
    name.starts_with(STAGING_PREFIX)
}

/// Current Unix timestamp in seconds, saturating to 0 before the epoch.
///
/// Exposed so callers computing cache ages use the same clock as the timestamps
/// recorded in receipts and last-used markers.
pub fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

fn expected_executable(root: &Path, spec: &RuntimeSpec) -> PathBuf {
    let relative_bin = spec
        .bin_dirs
        .first()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."));
    root.join(relative_bin).join(&spec.executable)
}

fn absolute_bin_dirs(root: &Path, spec: &RuntimeSpec) -> Vec<PathBuf> {
    spec.bin_dirs.iter().map(|dir| root.join(dir)).collect()
}

fn normalize_runtime(root: &Path, spec: &RuntimeSpec) -> Result<()> {
    if spec.tool == "python" {
        ensure_python_alias(root, spec)?;
    }
    Ok(())
}

/// Ensure a plain `python` entry point exists.
///
/// python-build-standalone ships `python3` and `python3.11` but not always a
/// bare `python`, which is the name runx advertises on PATH.
fn ensure_python_alias(root: &Path, spec: &RuntimeSpec) -> Result<()> {
    let bin = root.join(
        spec.bin_dirs
            .first()
            .cloned()
            .unwrap_or_else(|| PathBuf::from(".")),
    );
    let alias = bin.join(&spec.executable);
    if alias.exists() {
        return Ok(());
    }

    let major_minor = spec
        .version
        .split('.')
        .take(2)
        .collect::<Vec<_>>()
        .join(".");
    let candidates = if cfg!(windows) {
        vec![
            bin.join("python.exe"),
            bin.join(format!("python{major_minor}.exe")),
            bin.join("python3.exe"),
        ]
    } else {
        vec![
            bin.join(format!("python{major_minor}")),
            bin.join("python3"),
            bin.join("python"),
        ]
    };

    let Some(target) = candidates.into_iter().find(|path| path.is_file()) else {
        return Ok(());
    };

    create_alias(&target, &alias).with_context(|| {
        format!(
            "Failed to create python alias {} -> {}",
            alias.display(),
            target.display()
        )
    })
}

#[cfg(unix)]
fn create_alias(target: &Path, alias: &Path) -> Result<()> {
    use std::os::unix::fs::symlink;

    let file_name = target
        .file_name()
        .context("Python executable path has no file name")?;
    symlink(file_name, alias)?;
    Ok(())
}

/// Create the Python alias on Windows using a hard link, falling back to a full
/// copy if hard-linking fails (cross-device or permission issues).
#[cfg(windows)]
fn create_alias(target: &Path, alias: &Path) -> Result<()> {
    if fs::hard_link(target, alias).is_ok() {
        return Ok(());
    }
    fs::copy(target, alias).map(|_| ()).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::ArchiveKind;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().expect("create temp dir")
    }

    fn spec() -> RuntimeSpec {
        RuntimeSpec {
            tool: "node".to_string(),
            version: "20.11.0".to_string(),
            url: "https://example.invalid/node.tar.gz".to_string(),
            checksum_url: "https://example.invalid/SHASUMS256.txt".to_string(),
            archive_kind: ArchiveKind::TarGz,
            executable: if cfg!(windows) {
                "node.exe".to_string()
            } else {
                "node".to_string()
            },
            bin_dirs: if cfg!(windows) {
                vec![PathBuf::from(".")]
            } else {
                vec![PathBuf::from("bin")]
            },
        }
    }

    /// Populate a staging dir so it looks like a complete extraction.
    fn populate(staging: &Path, spec: &RuntimeSpec) {
        let bin = staging.join(&spec.bin_dirs[0]);
        fs::create_dir_all(&bin).unwrap();
        fs::write(bin.join(&spec.executable), b"#!/bin/sh\n").unwrap();
    }

    #[test]
    fn commit_promotes_staging_to_canonical_path() {
        let home = tmp();
        let spec = spec();

        let staging = staging_dir(home.path(), &spec).expect("staging dir");
        populate(&staging, &spec);

        let cached = commit_runtime(home.path(), &staging, &spec, None).expect("commit");

        assert_eq!(cached.root, runtime_root_in(home.path(), "node", "20.11.0"));
        assert!(cached.root.is_dir(), "runtime should exist at final path");
        assert!(!staging.exists(), "staging dir should be consumed");
        assert!(is_complete(&cached.root), "marker should be written");
    }

    /// The core atomicity property: a failed extraction leaves nothing at the
    /// canonical path, so the next run does not see a partial install.
    #[test]
    fn abandoned_staging_never_appears_as_a_cache_entry() {
        let home = tmp();
        let spec = spec();

        let staging = staging_dir(home.path(), &spec).expect("staging dir");
        // Simulate a partial extraction: some files, no executable.
        fs::create_dir_all(staging.join("lib")).unwrap();
        fs::write(staging.join("lib/partial.so"), b"x").unwrap();

        assert!(
            commit_runtime(home.path(), &staging, &spec, None).is_err(),
            "commit must fail when the executable is missing"
        );
        assert!(
            !runtime_root_in(home.path(), "node", "20.11.0").exists(),
            "nothing may be published at the canonical path"
        );
        assert!(
            cached_runtime_in(home.path(), &spec).unwrap().is_none(),
            "a failed install must not register as cached"
        );

        discard_staging(&staging);
        assert!(!staging.exists());
    }

    /// A failed reinstall must not destroy the runtime already in the cache.
    #[test]
    fn failed_install_preserves_the_existing_runtime() {
        let home = tmp();
        let spec = spec();

        let first = staging_dir(home.path(), &spec).expect("staging dir");
        populate(&first, &spec);
        commit_runtime(home.path(), &first, &spec, None).expect("first install");

        // A second install that fails verification.
        let second = staging_dir(home.path(), &spec).expect("second staging dir");
        fs::write(second.join("junk"), b"x").unwrap();
        assert!(commit_runtime(home.path(), &second, &spec, None).is_err());

        assert!(
            cached_runtime_in(home.path(), &spec).unwrap().is_some(),
            "the working runtime must survive a failed reinstall"
        );
    }

    #[test]
    fn commit_replaces_an_existing_runtime() {
        let home = tmp();
        let spec = spec();

        let first = staging_dir(home.path(), &spec).expect("staging");
        populate(&first, &spec);
        commit_runtime(home.path(), &first, &spec, None).expect("first install");

        let second = staging_dir(home.path(), &spec).expect("staging");
        populate(&second, &spec);
        fs::write(second.join("marker.txt"), b"second").unwrap();
        let cached = commit_runtime(home.path(), &second, &spec, None).expect("second install");

        assert!(
            cached.root.join("marker.txt").is_file(),
            "the newer install should win"
        );
    }

    #[test]
    fn concurrent_staging_dirs_are_distinct() {
        let home = tmp();
        let spec = spec();

        let a = staging_dir(home.path(), &spec).expect("first");
        let b = staging_dir(home.path(), &spec).expect("second");
        assert_ne!(a, b, "staging dirs must not collide");
        assert!(is_staging_name(a.file_name().unwrap().to_str().unwrap()));
    }

    /// Two threads installing the same runtime must both succeed and leave one
    /// valid runtime behind — the property that removes the need for a lockfile.
    #[test]
    fn concurrent_commits_yield_one_valid_runtime() {
        let home = tmp();
        let home_path = home.path().to_path_buf();

        let handles: Vec<_> = (0..4)
            .map(|_| {
                let home_path = home_path.clone();
                std::thread::spawn(move || {
                    let spec = spec();
                    let staging = staging_dir(&home_path, &spec).expect("staging");
                    populate(&staging, &spec);
                    commit_runtime(&home_path, &staging, &spec, None).map(|rt| rt.root)
                })
            })
            .collect();

        let mut succeeded = 0;
        for handle in handles {
            if handle.join().expect("thread should not panic").is_ok() {
                succeeded += 1;
            }
        }

        assert!(succeeded > 0, "at least one concurrent install should win");
        assert!(
            cached_runtime_in(&home_path, &spec()).unwrap().is_some(),
            "a valid runtime must remain after concurrent installs"
        );
    }

    /// Runtimes installed by earlier versions have no marker. They must be
    /// adopted, not deleted and re-downloaded.
    #[test]
    fn legacy_runtime_without_marker_is_adopted() {
        let home = tmp();
        let spec = spec();

        let root = runtime_root_in(home.path(), "node", "20.11.0");
        fs::create_dir_all(root.join(&spec.bin_dirs[0])).unwrap();
        fs::write(
            root.join(&spec.bin_dirs[0]).join(&spec.executable),
            b"legacy",
        )
        .unwrap();
        assert!(!is_complete(&root), "precondition: no marker yet");

        let cached = cached_runtime_in(home.path(), &spec)
            .expect("lookup should succeed")
            .expect("legacy runtime should be usable");

        assert_eq!(cached.root, root);
        assert!(is_complete(&root), "marker should be added on adoption");
        assert_eq!(
            fs::read(root.join(&spec.bin_dirs[0]).join(&spec.executable)).unwrap(),
            b"legacy",
            "adoption must not re-download or alter the runtime"
        );
    }

    #[test]
    fn missing_runtime_is_not_cached() {
        let home = tmp();
        assert!(cached_runtime_in(home.path(), &spec()).unwrap().is_none());
    }

    #[test]
    fn receipt_round_trips() {
        let home = tmp();
        let spec = spec();
        let staging = staging_dir(home.path(), &spec).expect("staging");
        populate(&staging, &spec);
        write_receipt(&staging, &spec, Some("abc123".to_string())).expect("write receipt");

        let receipt = read_receipt(&staging).expect("receipt should parse");
        assert_eq!(receipt.tool, "node");
        assert_eq!(receipt.version, "20.11.0");
        assert_eq!(receipt.sha256.as_deref(), Some("abc123"));
        assert_eq!(receipt.runx_version, env!("CARGO_PKG_VERSION"));
    }

    #[test]
    fn corrupt_marker_is_not_treated_as_complete() {
        let home = tmp();
        let spec = spec();
        let staging = staging_dir(home.path(), &spec).expect("staging");
        fs::write(staging.join(COMPLETION_MARKER), b"{not json").unwrap();

        assert!(!is_complete(&staging));
        assert!(read_receipt(&staging).is_none());
    }

    #[test]
    fn runx_home_prefers_the_env_override() {
        // Verified through the public path rather than by mutating the
        // process environment, which would race with parallel tests.
        let home = tmp();
        let root = runtime_root_in(home.path(), "node", "20.11.0");
        assert!(root.starts_with(home.path()));
        assert!(root.ends_with("runtimes/node/20.11.0") || cfg!(windows));
    }

    // ── Size formatting ──────────────────────────────────────────────────────

    #[test]
    fn formats_sizes_with_binary_units() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1023), "1023 B");
        assert_eq!(format_size(1024), "1.0 KiB");
        assert_eq!(format_size(1536), "1.5 KiB");
        assert_eq!(format_size(1024 * 1024), "1.0 MiB");
        assert_eq!(format_size(50 * 1024 * 1024), "50.0 MiB");
        assert_eq!(format_size(1024 * 1024 * 1024), "1.0 GiB");
    }

    // ── Directory sizing ─────────────────────────────────────────────────────

    #[test]
    fn sums_nested_file_sizes() {
        let dir = tmp();
        fs::create_dir_all(dir.path().join("bin")).unwrap();
        fs::write(dir.path().join("bin/node"), vec![0u8; 1000]).unwrap();
        fs::write(dir.path().join("README"), vec![0u8; 24]).unwrap();

        assert_eq!(directory_size(dir.path()), 1024);
    }

    /// Python runtimes alias `python -> python3.11`. Following the link would
    /// count the target twice and overstate the cache size.
    #[test]
    #[cfg(unix)]
    fn does_not_double_count_symlinked_files() {
        let dir = tmp();
        fs::write(dir.path().join("python3.11"), vec![0u8; 1000]).unwrap();
        std::os::unix::fs::symlink("python3.11", dir.path().join("python")).unwrap();

        let size = directory_size(dir.path());
        assert!(
            size < 1100,
            "a symlink must not be counted as another full copy, got {size}"
        );
    }

    /// A link pointing outside the cache must not inflate the total.
    #[test]
    #[cfg(unix)]
    fn ignores_symlink_targets_outside_the_tree() {
        let dir = tmp();
        let outside = dir.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("huge"), vec![0u8; 100_000]).unwrap();

        let runtime = dir.path().join("runtime");
        fs::create_dir_all(&runtime).unwrap();
        std::os::unix::fs::symlink(&outside, runtime.join("escape")).unwrap();

        assert!(
            directory_size(&runtime) < 10_000,
            "must not follow a link out of the runtime directory"
        );
    }

    #[test]
    fn missing_directory_has_zero_size() {
        let dir = tmp();
        assert_eq!(directory_size(&dir.path().join("nope")), 0);
    }

    // ── Listing ──────────────────────────────────────────────────────────────

    #[test]
    fn empty_cache_lists_nothing() {
        let dir = tmp();
        assert!(list_cached(dir.path()).expect("should succeed").is_empty());
    }

    #[test]
    fn lists_installed_runtimes_sorted() {
        let home = tmp();
        let spec = spec();

        // Install node 20.11.0 properly.
        let staging = staging_dir(home.path(), &spec).expect("staging");
        populate(&staging, &spec);
        commit_runtime(home.path(), &staging, &spec, Some("abc".to_string())).expect("commit");

        // And a second runtime, out of alphabetical order.
        let mut python = spec.clone();
        python.tool = "python".to_string();
        python.version = "3.11.7".to_string();
        let staging = staging_dir(home.path(), &python).expect("staging");
        populate(&staging, &python);
        commit_runtime(home.path(), &staging, &python, None).expect("commit");

        let entries = list_cached(home.path()).expect("list");
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].tool, "node", "should be sorted by tool");
        assert_eq!(entries[1].tool, "python");
        assert!(entries[0].complete);
        assert!(entries[0].size_bytes > 0);
        assert!(entries[0].installed_at_secs.is_some());
    }

    /// Staging directories are mid-install, not usable runtimes.
    #[test]
    fn listing_excludes_staging_directories() {
        let home = tmp();
        let spec = spec();
        let staging = staging_dir(home.path(), &spec).expect("staging");
        populate(&staging, &spec);

        assert!(
            list_cached(home.path()).expect("list").is_empty(),
            "an in-progress install must not appear as a cached runtime"
        );

        let abandoned = list_staging(home.path()).expect("list staging");
        assert_eq!(abandoned.len(), 1, "but it is reportable as staging");
        assert_eq!(abandoned[0], staging);
    }

    /// A legacy or damaged install still lists, flagged incomplete, so the user
    /// can see and clean it rather than wondering where the disk went.
    #[test]
    fn lists_incomplete_runtimes_as_incomplete() {
        let home = tmp();
        let spec = spec();
        let root = runtime_root_in(home.path(), "node", "20.11.0");
        fs::create_dir_all(root.join(&spec.bin_dirs[0])).unwrap();
        fs::write(root.join(&spec.bin_dirs[0]).join(&spec.executable), b"x").unwrap();

        let entries = list_cached(home.path()).expect("list");
        assert_eq!(entries.len(), 1);
        assert!(!entries[0].complete, "no receipt means incomplete");
        assert!(entries[0].installed_at_secs.is_none());
    }

    // ── Last-used tracking ───────────────────────────────────────────────────

    #[test]
    fn last_used_round_trips() {
        let dir = tmp();
        touch_last_used(dir.path());

        let recorded = read_last_used(dir.path()).expect("should record a timestamp");
        assert!(
            now_secs().saturating_sub(recorded) < 5,
            "timestamp should be recent"
        );
    }

    #[test]
    fn missing_or_corrupt_last_used_is_none() {
        let dir = tmp();
        assert_eq!(read_last_used(dir.path()), None);

        fs::write(dir.path().join(LAST_USED_MARKER), b"not a number").unwrap();
        assert_eq!(read_last_used(dir.path()), None);
    }

    /// Recording last use must never be able to invalidate a runtime, which is
    /// why it lives in its own file rather than in the receipt.
    #[test]
    fn touching_last_used_does_not_affect_completeness() {
        let home = tmp();
        let spec = spec();
        let staging = staging_dir(home.path(), &spec).expect("staging");
        populate(&staging, &spec);
        let cached = commit_runtime(home.path(), &staging, &spec, None).expect("commit");

        touch_last_used(&cached.root);

        assert!(is_complete(&cached.root), "still a valid runtime");
        assert!(read_receipt(&cached.root).is_some(), "receipt intact");
    }

    #[test]
    fn age_prefers_last_use_over_install_date() {
        let now = 10 * 86_400;
        let entry = CacheEntry {
            tool: "node".to_string(),
            version: "20.11.0".to_string(),
            root: PathBuf::from("/tmp/x"),
            size_bytes: 0,
            complete: true,
            installed_at_secs: Some(0),
            last_used_secs: Some(8 * 86_400),
        };

        assert_eq!(entry.age_days(now), Some(2), "should measure from last use");

        let never_used = CacheEntry {
            last_used_secs: None,
            ..entry
        };
        assert_eq!(
            never_used.age_days(now),
            Some(10),
            "should fall back to the install date"
        );
    }

    #[test]
    fn age_is_unknown_without_any_timestamp() {
        let entry = CacheEntry {
            tool: "node".to_string(),
            version: "20.11.0".to_string(),
            root: PathBuf::from("/tmp/x"),
            size_bytes: 0,
            complete: false,
            installed_at_secs: None,
            last_used_secs: None,
        };
        assert_eq!(entry.age_days(now_secs()), None);
    }

    // ── Removal ──────────────────────────────────────────────────────────────

    #[test]
    fn removes_a_cached_runtime() {
        let home = tmp();
        let spec = spec();
        let staging = staging_dir(home.path(), &spec).expect("staging");
        populate(&staging, &spec);
        let cached = commit_runtime(home.path(), &staging, &spec, None).expect("commit");

        remove_entry(&cached.root).expect("remove");

        assert!(!cached.root.exists());
        assert!(list_cached(home.path()).expect("list").is_empty());
        assert!(cached_runtime_in(home.path(), &spec).unwrap().is_none());
    }
}
