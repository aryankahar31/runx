use crate::runtime::RuntimeSpec;
use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone)]
pub struct CachedRuntime {
    pub root: PathBuf,
    pub bin_dirs: Vec<PathBuf>,
}

pub fn runx_home() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not determine your home directory")?;
    Ok(home.join(".runx"))
}

pub fn runtime_root(spec: &RuntimeSpec) -> Result<PathBuf> {
    Ok(runx_home()?
        .join("runtimes")
        .join(&spec.tool)
        .join(&spec.version))
}

pub fn cached_runtime(spec: &RuntimeSpec) -> Result<Option<CachedRuntime>> {
    let root = runtime_root(spec)?;
    let exe = expected_executable(&root, spec);
    if exe.is_file() {
        Ok(Some(CachedRuntime {
            root: root.clone(),
            bin_dirs: absolute_bin_dirs(&root, spec),
        }))
    } else {
        Ok(None)
    }
}

pub fn prepare_runtime_dir(spec: &RuntimeSpec) -> Result<PathBuf> {
    let root = runtime_root(spec)?;
    if root.exists() {
        fs::remove_dir_all(&root)
            .with_context(|| format!("Failed to replace incomplete cache at {}", root.display()))?;
    }
    fs::create_dir_all(&root)
        .with_context(|| format!("Failed to create runtime cache {}", root.display()))?;
    Ok(root)
}

pub fn finalize_cached_runtime(root: &Path, spec: &RuntimeSpec) -> Result<CachedRuntime> {
    normalize_runtime(root, spec)?;
    let exe = expected_executable(root, spec);
    if !exe.is_file() {
        anyhow::bail!(
            "Downloaded {} {} but did not find expected executable at {}",
            spec.tool,
            spec.version,
            exe.display()
        );
    }

    Ok(CachedRuntime {
        root: root.to_path_buf(),
        bin_dirs: absolute_bin_dirs(root, spec),
    })
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

#[cfg(windows)]
/// Create the Python alias on Windows using a hard link, falling back to a full
/// copy only if hard-linking fails (e.g. cross-device or permission issues).
/// Hard links avoid wasting disk space and never go stale relative to the
/// original executable.
fn create_alias(target: &Path, alias: &Path) -> Result<()> {
    if std::fs::hard_link(target, alias).is_ok() {
        return Ok(());
    }
    // Fallback for cross-device or permission issues.
    fs::copy(target, alias).map(|_| ()).map_err(Into::into)
}
