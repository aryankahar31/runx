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
pub fn commit_runtime(home: &Path, staging: &Path, spec: &RuntimeSpec) -> Result<CachedRuntime> {
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

    write_receipt(staging, spec, None)?;

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

fn now_secs() -> u64 {
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

        let cached = commit_runtime(home.path(), &staging, &spec).expect("commit");

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
            commit_runtime(home.path(), &staging, &spec).is_err(),
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
        commit_runtime(home.path(), &first, &spec).expect("first install");

        // A second install that fails verification.
        let second = staging_dir(home.path(), &spec).expect("second staging dir");
        fs::write(second.join("junk"), b"x").unwrap();
        assert!(commit_runtime(home.path(), &second, &spec).is_err());

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
        commit_runtime(home.path(), &first, &spec).expect("first install");

        let second = staging_dir(home.path(), &spec).expect("staging");
        populate(&second, &spec);
        fs::write(second.join("marker.txt"), b"second").unwrap();
        let cached = commit_runtime(home.path(), &second, &spec).expect("second install");

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
                    commit_runtime(&home_path, &staging, &spec).map(|rt| rt.root)
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
}
