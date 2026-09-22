use crate::cache::CachedRuntime;
use anyhow::{Context, Result};
use std::{
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
};

/// Detected dependency manager for a project.
///
/// Each variant carries the lockfile (or manifest) path that triggered
/// detection. Future phases add Pnpm, Yarn, Bun, Python, Go, Deno.
pub enum DepManager {
    Npm { lockfile: PathBuf },
}

/// Result of scanning a project directory for dependency information.
pub struct DepDetection {
    pub manager: DepManager,
    pub label: &'static str,
}

/// Scan `project_dir` for dependency manager indicators.
///
/// Returns `None` when no recognised lockfile/manifest is found — the
/// caller decides what that means (error, skip, etc.).
pub fn detect(project_dir: &Path) -> Option<DepDetection> {
    // ponytail: first-match wins, one file per PM.
    // Phase 5 adds pnpm-lock.yaml, yarn.lock, bun.lock/bun.lockb.
    if project_dir.join("package-lock.json").is_file() {
        return Some(DepDetection {
            manager: DepManager::Npm {
                lockfile: project_dir.join("package-lock.json"),
            },
            label: "npm",
        });
    }
    // ponytail: Phase 3 adds pyproject.toml + requirements.txt.
    // Phase 4 adds bun.lock, go.mod, deno.json.
    None
}

/// True when installed `node_modules` is newer than `package-lock.json`.
///
/// This is a heuristic — it can be fooled by manual edits to node_modules
/// — but it covers the dominant case (clean checkout → install → run) and
/// avoids running `npm ls` or hashing. The ceiling is a false-skip after
/// manual tampering; the upgrade path is a content hash of the lockfile
/// compared against a marker file inside node_modules.
pub fn deps_installed(project_dir: &Path, manager: &DepManager) -> bool {
    match manager {
        DepManager::Npm { lockfile } => {
            let nm = project_dir.join("node_modules");
            match (nm.metadata(), lockfile.metadata()) {
                (Ok(nm_meta), Ok(lf_meta)) => {
                    let nm_modified = nm_meta
                        .modified()
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    let lf_modified = lf_meta
                        .modified()
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    nm_modified >= lf_modified
                }
                // node_modules doesn't exist → not installed.
                (Err(_), _) => false,
                // lockfile doesn't exist → can't compare, assume not installed.
                (_, Err(_)) => false,
            }
        }
    }
}

/// Install project dependencies using the runx-managed runtime.
///
/// `runtimes` is the list of provisioned runtimes (from [`crate::cache`]);
/// the first entry whose tool is `"node"` supplies the `npm` binary on PATH.
pub fn install(
    project_dir: &Path,
    detection: &DepDetection,
    runtimes: &[CachedRuntime],
) -> Result<()> {
    match &detection.manager {
        DepManager::Npm { .. } => install_npm(project_dir, runtimes),
    }
}

fn install_npm(project_dir: &Path, runtimes: &[CachedRuntime]) -> Result<()> {
    let node_runtime = runtimes
        .iter()
        .find(|r| {
            r.bin_dirs
                .iter()
                .any(|d| d.join("node").is_file() || d.join("node.exe").is_file())
        })
        .context("Node runtime not provisioned — cannot run npm ci")?;

    // Build a PATH with the node bin dir first so npm from the managed
    // Node is used, never a stray system npm.
    let mut paths: Vec<PathBuf> = node_runtime.bin_dirs.clone();
    if let Some(sys) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&sys));
    }
    let path = std::env::join_paths(&paths).context("Failed to build PATH for npm")?;

    // ponytail: `npm ci` for deterministic installs from package-lock.json.
    // `npm install` would regenerate the lockfile, which is not what we want.
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg("npm ci");
        c
    } else {
        let mut c = Command::new("/bin/sh");
        c.arg("-c").arg("npm ci");
        c
    };

    cmd.current_dir(project_dir)
        .env("PATH", &path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    let status: ExitStatus = cmd
        .spawn()
        .context("Failed to start `npm ci`")?
        .wait()
        .context("Failed to wait for `npm ci`")?;

    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "`npm ci` failed in {} with exit code {}",
            project_dir.display(),
            status.code().unwrap_or(-1)
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn detects_npm_from_package_lock_json() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("package.json"), "{}").unwrap();
        fs::write(dir.path().join("package-lock.json"), "{}").unwrap();

        let det = detect(dir.path()).expect("should detect npm");
        assert_eq!(det.label, "npm");
        assert!(matches!(det.manager, DepManager::Npm { .. }));
    }

    #[test]
    fn no_lockfile_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("package.json"), "{}").unwrap();
        assert!(detect(dir.path()).is_none());
    }

    #[test]
    fn npm_not_installed_when_node_modules_missing() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("package-lock.json"), "{}").unwrap();
        let det = detect(dir.path()).unwrap();
        assert!(!deps_installed(dir.path(), &det.manager));
    }

    #[test]
    fn npm_installed_when_node_modules_newer() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("package-lock.json"), "{}").unwrap();
        fs::create_dir(dir.path().join("node_modules")).unwrap();
        let det = detect(dir.path()).unwrap();
        // node_modules was created after package-lock.json
        assert!(deps_installed(dir.path(), &det.manager));
    }
}
