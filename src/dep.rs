use crate::cache::CachedRuntime;
use anyhow::{Context, Result};
use std::{
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
};

/// Detected dependency manager for a project.
///
/// Each variant carries the file path that triggered detection.
pub enum DepManager {
    Npm { lockfile: PathBuf },
    Yarn { lockfile: PathBuf },
    Pnpm { lockfile: PathBuf },
    PythonPyproject { pyproject: PathBuf },
    PythonRequirements { requirements: PathBuf },
    Bun { lockfile: PathBuf },
    Go { gomod: PathBuf },
    Deno { lockfile: PathBuf },
}

/// Result of scanning a project directory for dependency information.
pub struct DepDetection {
    pub manager: DepManager,
    pub label: &'static str,
}

/// Scan `project_dir` for dependency manager indicators.
///
/// Returns `None` when no recognised lockfile/manifest is found.
/// This is the single-manager API (first-match wins) kept for backward compatibility.
pub fn detect(project_dir: &Path) -> Option<DepDetection> {
    detect_all(project_dir).into_iter().next()
}

/// Scan `project_dir` for ALL dependency manager indicators.
///
/// Returns a list of all detected managers in priority order.
/// Priority: npm > pnpm > yarn > pip (pyproject) > pip (requirements) > bun > go > deno.
pub fn detect_all(project_dir: &Path) -> Vec<DepDetection> {
    let mut detections = Vec::new();

    if project_dir.join("package-lock.json").is_file() {
        detections.push(DepDetection {
            manager: DepManager::Npm {
                lockfile: project_dir.join("package-lock.json"),
            },
            label: "npm",
        });
    }
    if project_dir.join("pnpm-lock.yaml").is_file() {
        detections.push(DepDetection {
            manager: DepManager::Pnpm {
                lockfile: project_dir.join("pnpm-lock.yaml"),
            },
            label: "pnpm",
        });
    }
    if project_dir.join("yarn.lock").is_file() {
        detections.push(DepDetection {
            manager: DepManager::Yarn {
                lockfile: project_dir.join("yarn.lock"),
            },
            label: "yarn",
        });
    }
    if project_dir.join("pyproject.toml").is_file() {
        detections.push(DepDetection {
            manager: DepManager::PythonPyproject {
                pyproject: project_dir.join("pyproject.toml"),
            },
            label: "pip (pyproject.toml)",
        });
    }
    if project_dir.join("requirements.txt").is_file() {
        detections.push(DepDetection {
            manager: DepManager::PythonRequirements {
                requirements: project_dir.join("requirements.txt"),
            },
            label: "pip (requirements.txt)",
        });
    }
    for name in &["bun.lock", "bun.lockb"] {
        if project_dir.join(name).is_file() {
            detections.push(DepDetection {
                manager: DepManager::Bun {
                    lockfile: project_dir.join(name),
                },
                label: "bun",
            });
            break;
        }
    }
    if project_dir.join("bunfig.toml").is_file() {
        detections.push(DepDetection {
            manager: DepManager::Bun {
                lockfile: project_dir.join("bunfig.toml"),
            },
            label: "bun",
        });
    }
    if project_dir.join("go.mod").is_file() {
        detections.push(DepDetection {
            manager: DepManager::Go {
                gomod: project_dir.join("go.mod"),
            },
            label: "go",
        });
    }
    if project_dir.join("deno.lock").is_file() {
        detections.push(DepDetection {
            manager: DepManager::Deno {
                lockfile: project_dir.join("deno.lock"),
            },
            label: "deno",
        });
    }

    detections
}

/// Return `true` if `pyproject.toml` contains a `[build-system]` table.
///
/// Only buildable packages with a PEP 517 backend can use `pip install -e .`.
/// Config-only pyproject.toml files (dependency declarations, tool config)
/// lack this table and must install deps via individual `pip install` args.
pub fn has_build_system(pyproject_path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(pyproject_path) else {
        return false;
    };
    let Ok(table) = text.parse::<toml::Table>() else {
        return false;
    };
    table.contains_key("build-system")
}

/// Extract dependency strings from `[project.dependencies]` in a pyproject.toml.
///
/// Supports two TOML layouts:
/// - PEP 621 array: `dependencies = ["flask>=2.0"]`
/// - Table format: `[project.dependencies]\nflask = ">=2.0"`
///
/// Returns PEP 508 dependency strings, or an empty vec if unparseable.
pub fn extract_pyproject_deps(pyproject_path: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(pyproject_path) else {
        return Vec::new();
    };
    let Ok(table) = text.parse::<toml::Table>() else {
        return Vec::new();
    };
    let Some(project) = table.get("project") else {
        return Vec::new();
    };
    let Some(deps) = project.get("dependencies") else {
        return Vec::new();
    };

    // PEP 621 inline array: `dependencies = ["flask>=2.0", "click>=8.0"]`
    if let Some(arr) = deps.as_array() {
        return arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
    }

    // Table format: `[project.dependencies]\nflask = ">=2.0"` → "flask>=2.0"
    if let Some(tbl) = deps.as_table() {
        return tbl
            .iter()
            .map(|(name, val)| {
                let ver = val.as_str().unwrap_or("*");
                format!("{name}{ver}")
            })
            .collect();
    }

    Vec::new()
}

/// True when dependencies appear to already be installed.
///
/// NPM: `node_modules` mtime >= `package-lock.json` mtime.
/// Python: `.venv` or `venv` directory exists (standard venv convention).
///
/// ponytail: heuristic, not proof. Ceiling: false-skip after manual tampering.
/// Upgrade path: content hash of lockfile vs marker in venv/site-packages.
pub fn deps_installed(project_dir: &Path, manager: &DepManager) -> bool {
    match manager {
        DepManager::Npm { lockfile }
        | DepManager::Yarn { lockfile }
        | DepManager::Pnpm { lockfile } => {
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
                (Err(_), _) => false,
                (_, Err(_)) => false,
            }
        }
        DepManager::PythonPyproject { .. } | DepManager::PythonRequirements { .. } => {
            // Check for standard venv directories.
            project_dir.join(".venv").is_dir() || project_dir.join("venv").is_dir()
        }
        DepManager::Bun { lockfile } => {
            // Bun uses node_modules, same as npm.
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
                (Err(_), _) => false,
                (_, Err(_)) => false,
            }
        }
        DepManager::Go { gomod } => {
            // go.sum is the local proof that modules are downloaded.
            let go_sum = project_dir.join("go.sum");
            match (go_sum.metadata(), gomod.metadata()) {
                (Ok(gs_meta), Ok(gm_meta)) => {
                    let gs_modified = gs_meta
                        .modified()
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    let gm_modified = gm_meta
                        .modified()
                        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                    gs_modified >= gm_modified
                }
                _ => false,
            }
        }
        DepManager::Deno { lockfile } => {
            // deno.lock is both the manifest and the proof of installation.
            // An empty lockfile ({} or empty) means no deps have been locked yet.
            // We consider deps installed if the lockfile exists and has meaningful content.
            match std::fs::read_to_string(lockfile) {
                Ok(content) => {
                    let trimmed = content.trim();
                    // Empty file or empty JSON object means no deps locked yet.
                    !(trimmed.is_empty() || trimmed == "{}" || trimmed == "[]")
                }
                Err(_) => false,
            }
        }
    }
}

/// Install project dependencies using the runx-managed runtime.
pub fn install(
    project_dir: &Path,
    detection: &DepDetection,
    runtimes: &[CachedRuntime],
) -> Result<()> {
    match &detection.manager {
        DepManager::Npm { .. } => install_npm(project_dir, runtimes),
        DepManager::Yarn { .. } => install_yarn(project_dir, runtimes),
        DepManager::PythonPyproject { .. } => install_python(project_dir, runtimes, true),
        DepManager::PythonRequirements { .. } => install_python(project_dir, runtimes, false),
        DepManager::Bun { .. } => install_bun(project_dir, runtimes),
        DepManager::Go { .. } => install_go(project_dir, runtimes),
        DepManager::Pnpm { .. } => install_pnpm(project_dir, runtimes),
        DepManager::Deno { .. } => install_deno(project_dir, runtimes),
    }
}

fn find_runtime<'a>(runtimes: &'a [CachedRuntime], exe: &str) -> Option<&'a CachedRuntime> {
    runtimes.iter().find(|r| {
        r.bin_dirs
            .iter()
            .any(|d| d.join(exe).is_file() || d.join(format!("{exe}.exe")).is_file())
    })
}

fn build_path(runtime: &CachedRuntime) -> Result<std::ffi::OsString> {
    let mut paths: Vec<PathBuf> = runtime.bin_dirs.clone();
    if let Some(sys) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&sys));
    }
    std::env::join_paths(paths).context("Failed to build PATH")
}

fn run_shell_command(
    command: &str,
    project_dir: &Path,
    path: &std::ffi::OsString,
) -> Result<ExitStatus> {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(command);
        c
    } else {
        let mut c = Command::new("/bin/sh");
        c.arg("-c").arg(command);
        c
    };
    cmd.current_dir(project_dir)
        .env("PATH", path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    let status = cmd
        .spawn()
        .context(format!("Failed to start `{command}`"))?
        .wait()
        .context(format!("Failed to wait for `{command}`"))?;
    Ok(status)
}

fn install_npm(project_dir: &Path, runtimes: &[CachedRuntime]) -> Result<()> {
    let node = find_runtime(runtimes, "node")
        .context("Node runtime not provisioned — cannot run npm ci")?;
    let path = build_path(node)?;
    let status = run_shell_command("npm ci", project_dir, &path)?;
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

fn install_python(
    project_dir: &Path,
    runtimes: &[CachedRuntime],
    is_pyproject: bool,
) -> Result<()> {
    let py = find_runtime(runtimes, "python")
        .context("Python runtime not provisioned — cannot run pip")?;
    let path = build_path(py)?;

    // Step 1: Create a project-local .venv using the managed Python.
    let venv_status = run_shell_command("python -m venv .venv", project_dir, &path)?;
    if !venv_status.success() {
        return Err(anyhow::anyhow!(
            "Failed to create virtual environment in {} (exit code {})",
            project_dir.display(),
            venv_status.code().unwrap_or(-1)
        ));
    }

    // Step 2: Choose the pip install command targeting the venv's pip.
    // Use Path::join to construct correct paths (not string formatting).
    let venv_pip = project_dir.join(".venv").join("bin").join("pip");
    let pyproject_path = project_dir.join("pyproject.toml");
    let cmd_str = if is_pyproject && has_build_system(&pyproject_path) {
        // PEP 517 build backend present — editable install into the venv.
        format!("{} install -e .", venv_pip.display())
    } else if is_pyproject {
        // Config-only pyproject.toml — extract deps, install individually.
        let deps = extract_pyproject_deps(&pyproject_path);
        if deps.is_empty() {
            return Err(anyhow::anyhow!(
                "pyproject.toml in {} has no [build-system] and no [project.dependencies].\n\
                 Hint: add a [build-system] table, a requirements.txt, or [project.dependencies].",
                project_dir.display()
            ));
        }
        let quoted: Vec<String> = deps
            .iter()
            .map(|d| format!("'{}'", d.replace('\'', "'\\''")))
            .collect();
        format!("{} install {}", venv_pip.display(), quoted.join(" "))
    } else {
        format!("{} install -r requirements.txt", venv_pip.display())
    };

    let status = run_shell_command(&cmd_str, project_dir, &path)?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "`{cmd_str}` failed in {} with exit code {}",
            project_dir.display(),
            status.code().unwrap_or(-1)
        ))
    }
}

fn install_bun(project_dir: &Path, runtimes: &[CachedRuntime]) -> Result<()> {
    let bun = find_runtime(runtimes, "bun")
        .context("Bun runtime not provisioned — cannot run bun install")?;
    let path = build_path(bun)?;
    let status = run_shell_command("bun install", project_dir, &path)?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "`bun install` failed in {} with exit code {}",
            project_dir.display(),
            status.code().unwrap_or(-1)
        ))
    }
}

fn install_go(project_dir: &Path, runtimes: &[CachedRuntime]) -> Result<()> {
    let go = find_runtime(runtimes, "go")
        .context("Go runtime not provisioned — cannot run go mod download")?;
    let path = build_path(go)?;
    let status = run_shell_command("go mod download", project_dir, &path)?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "`go mod download` failed in {} with exit code {}",
            project_dir.display(),
            status.code().unwrap_or(-1)
        ))
    }
}

/// Install project dependencies via Deno.
///
/// Uses the runx-managed Deno runtime to run `deno install` which
/// reads the lockfile and installs dependencies.
fn install_deno(project_dir: &Path, runtimes: &[CachedRuntime]) -> Result<()> {
    let deno = find_runtime(runtimes, "deno")
        .context("Deno runtime not provisioned — cannot run deno install")?;
    let path = build_path(deno)?;
    let status = run_shell_command("deno install", project_dir, &path)?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "`deno install` failed in {} with exit code {}",
            project_dir.display(),
            status.code().unwrap_or(-1)
        ))
    }
}

/// Install project dependencies via pnpm.
///
/// pnpm is a separate package manager — it is NOT bundled with Node.
/// The user's system must have pnpm installed (e.g. `npm install -g pnpm`
/// or via corepack). This function finds the managed Node runtime so the
/// correct Node version is on PATH, then delegates to the system pnpm.
fn install_pnpm(project_dir: &Path, runtimes: &[CachedRuntime]) -> Result<()> {
    let node = find_runtime(runtimes, "node")
        .context("Node runtime not provisioned — cannot run pnpm install")?;
    let path = build_path(node)?;
    let status = run_shell_command("pnpm install", project_dir, &path)?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "`pnpm install` failed in {} with exit code {}\n\
             Hint: ensure pnpm is installed (npm install -g pnpm) or available via corepack.",
            project_dir.display(),
            status.code().unwrap_or(-1)
        ))
    }
}

/// Install project dependencies via Yarn.
///
/// Like pnpm, Yarn is a separate package manager — it is NOT bundled with Node.
/// The user's system must have yarn installed. This function finds the managed
/// Node runtime so the correct Node version is on PATH, then delegates to the system yarn.
fn install_yarn(project_dir: &Path, runtimes: &[CachedRuntime]) -> Result<()> {
    let node = find_runtime(runtimes, "node")
        .context("Node runtime not provisioned — cannot run yarn install")?;
    let path = build_path(node)?;
    let status = run_shell_command("yarn install", project_dir, &path)?;
    if status.success() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(
            "`yarn install` failed in {} with exit code {}\n\
             Hint: ensure yarn is installed (npm install -g yarn) or via corepack.",
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
    fn detects_python_from_pyproject_toml() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("pyproject.toml"), "[project]\n").unwrap();
        let det = detect(dir.path()).expect("should detect python");
        assert_eq!(det.label, "pip (pyproject.toml)");
        assert!(matches!(det.manager, DepManager::PythonPyproject { .. }));
    }

    #[test]
    fn detects_python_from_requirements_txt() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("requirements.txt"), "flask\n").unwrap();
        let det = detect(dir.path()).expect("should detect python");
        assert_eq!(det.label, "pip (requirements.txt)");
        assert!(matches!(det.manager, DepManager::PythonRequirements { .. }));
    }

    #[test]
    fn pyproject_toml_wins_over_requirements_txt() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("pyproject.toml"), "[project]\n").unwrap();
        fs::write(dir.path().join("requirements.txt"), "flask\n").unwrap();
        let det = detect(dir.path()).unwrap();
        assert!(matches!(det.manager, DepManager::PythonPyproject { .. }));
    }

    #[test]
    fn no_lockfile_returns_none() {
        let dir = tempfile::tempdir().unwrap();
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
        assert!(deps_installed(dir.path(), &det.manager));
    }

    #[test]
    fn python_not_installed_without_venv() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("pyproject.toml"), "[project]\n").unwrap();
        let det = detect(dir.path()).unwrap();
        assert!(!deps_installed(dir.path(), &det.manager));
    }

    #[test]
    fn python_installed_with_dot_venv() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("pyproject.toml"), "[project]\n").unwrap();
        fs::create_dir(dir.path().join(".venv")).unwrap();
        let det = detect(dir.path()).unwrap();
        assert!(deps_installed(dir.path(), &det.manager));
    }

    #[test]
    fn python_installed_with_venv() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("requirements.txt"), "flask\n").unwrap();
        fs::create_dir(dir.path().join("venv")).unwrap();
        let det = detect(dir.path()).unwrap();
        assert!(deps_installed(dir.path(), &det.manager));
    }

    #[test]
    fn detects_bun_from_bun_lock() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("bun.lock"), "{}\n").unwrap();
        let det = detect(dir.path()).expect("should detect bun");
        assert_eq!(det.label, "bun");
        assert!(matches!(det.manager, DepManager::Bun { .. }));
    }

    #[test]
    fn detects_bun_from_bun_lockb() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("bun.lockb"), "BunLock\x00\x01").unwrap();
        let det = detect(dir.path()).expect("should detect bun");
        assert_eq!(det.label, "bun");
        assert!(matches!(det.manager, DepManager::Bun { .. }));
    }

    #[test]
    fn detects_bun_from_bunfig_toml() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("bunfig.toml"), "[install]\n").unwrap();
        let det = detect(dir.path()).expect("should detect bun");
        assert_eq!(det.label, "bun");
        assert!(matches!(det.manager, DepManager::Bun { .. }));
    }

    #[test]
    fn detects_go_from_go_mod() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("go.mod"), "module m\n\ngo 1.22.5\n").unwrap();
        let det = detect(dir.path()).expect("should detect go");
        assert_eq!(det.label, "go");
        assert!(matches!(det.manager, DepManager::Go { .. }));
    }

    #[test]
    fn bun_not_installed_without_node_modules() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("bun.lock"), "{}\n").unwrap();
        let det = detect(dir.path()).unwrap();
        assert!(!deps_installed(dir.path(), &det.manager));
    }

    #[test]
    fn bun_installed_with_node_modules() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("bun.lock"), "{}\n").unwrap();
        fs::create_dir(dir.path().join("node_modules")).unwrap();
        let det = detect(dir.path()).unwrap();
        assert!(deps_installed(dir.path(), &det.manager));
    }

    #[test]
    fn go_not_installed_without_go_sum() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("go.mod"), "module m\n\ngo 1.22.5\n").unwrap();
        let det = detect(dir.path()).unwrap();
        assert!(!deps_installed(dir.path(), &det.manager));
    }

    #[test]
    fn go_installed_with_go_sum() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("go.mod"), "module m\n\ngo 1.22.5\n").unwrap();
        fs::write(dir.path().join("go.sum"), "").unwrap();
        let det = detect(dir.path()).unwrap();
        assert!(deps_installed(dir.path(), &det.manager));
    }

    #[test]
    fn npm_lock_wins_over_bun_lock() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("bun.lock"), "{}\n").unwrap();
        fs::write(dir.path().join("package-lock.json"), "{}").unwrap();
        let det = detect(dir.path()).unwrap();
        assert!(
            matches!(det.manager, DepManager::Npm { .. }),
            "package-lock.json should take precedence for backward compat"
        );
    }

    #[test]
    fn detects_pnpm_from_pnpm_lock_yaml() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("pnpm-lock.yaml"),
            "lockfileVersion: '9.0'\n",
        )
        .unwrap();
        let det = detect(dir.path()).expect("should detect pnpm");
        assert_eq!(det.label, "pnpm");
        assert!(matches!(det.manager, DepManager::Pnpm { .. }));
    }

    #[test]
    fn pnpm_not_installed_without_node_modules() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("pnpm-lock.yaml"),
            "lockfileVersion: '9.0'\n",
        )
        .unwrap();
        let det = detect(dir.path()).unwrap();
        assert!(!deps_installed(dir.path(), &det.manager));
    }

    #[test]
    fn pnpm_installed_with_node_modules() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("pnpm-lock.yaml"),
            "lockfileVersion: '9.0'\n",
        )
        .unwrap();
        fs::create_dir(dir.path().join("node_modules")).unwrap();
        let det = detect(dir.path()).unwrap();
        assert!(deps_installed(dir.path(), &det.manager));
    }

    #[test]
    fn npm_lock_wins_over_pnpm_lock() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("pnpm-lock.yaml"),
            "lockfileVersion: '9.0'\n",
        )
        .unwrap();
        fs::write(dir.path().join("package-lock.json"), "{}").unwrap();
        let det = detect(dir.path()).unwrap();
        assert!(
            matches!(det.manager, DepManager::Npm { .. }),
            "package-lock.json should take precedence over pnpm-lock.yaml"
        );
    }

    #[test]
    fn detects_yarn_from_yarn_lock() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("yarn.lock"), "# yarn lockfile\n").unwrap();
        let det = detect(dir.path()).expect("should detect yarn");
        assert_eq!(det.label, "yarn");
        assert!(matches!(det.manager, DepManager::Yarn { .. }));
    }

    #[test]
    fn yarn_not_installed_without_node_modules() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("yarn.lock"), "# yarn lockfile\n").unwrap();
        let det = detect(dir.path()).unwrap();
        assert!(!deps_installed(dir.path(), &det.manager));
    }

    #[test]
    fn yarn_installed_with_node_modules() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("yarn.lock"), "# yarn lockfile\n").unwrap();
        fs::create_dir(dir.path().join("node_modules")).unwrap();
        let det = detect(dir.path()).unwrap();
        assert!(deps_installed(dir.path(), &det.manager));
    }

    #[test]
    fn npm_lock_wins_over_yarn_lock() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("yarn.lock"), "# yarn lockfile\n").unwrap();
        fs::write(dir.path().join("package-lock.json"), "{}").unwrap();
        let det = detect(dir.path()).unwrap();
        assert!(
            matches!(det.manager, DepManager::Npm { .. }),
            "package-lock.json should take precedence over yarn.lock"
        );
    }

    #[test]
    fn detects_deno_from_deno_lock() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("deno.lock"), "{}\n").unwrap();
        let det = detect(dir.path()).expect("should detect deno");
        assert_eq!(det.label, "deno");
        assert!(matches!(det.manager, DepManager::Deno { .. }));
    }

    #[test]
    fn deno_not_installed_without_deno_lock() {
        let dir = tempfile::tempdir().unwrap();
        // No deno.lock -> not installed
        assert!(detect(dir.path()).is_none());
    }

    #[test]
    fn deno_installed_with_deno_lock() {
        let dir = tempfile::tempdir().unwrap();
        // A lockfile with actual dependency entries (not empty {}) means installed.
        fs::write(
            dir.path().join("deno.lock"),
            r#"{"https://deno.land/std@0.200.0/http/server.ts": "sha256-abc123"}"#,
        )
        .unwrap();
        let det = detect(dir.path()).unwrap();
        assert!(deps_installed(dir.path(), &det.manager));
    }
}
