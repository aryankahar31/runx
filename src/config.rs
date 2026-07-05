use crate::detect;
use crate::error::UserError;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

pub const CONFIG_FILE: &str = "runx.toml";

#[derive(Debug, Clone, Deserialize)]
pub struct RunxConfig {
    #[serde(default)]
    pub runtimes: BTreeMap<String, String>,
    pub run: BTreeMap<String, String>,
}

/// The resolved configuration, annotated with information about how it was
/// loaded.  Callers use `detection_lines` to print the transparency banner
/// when auto-detection was used.
#[derive(Debug, Clone)]
pub struct ResolvedConfig {
    pub inner: RunxConfig,
    /// Non-empty when auto-detection was used (no `runx.toml` present).
    /// Each entry is one line to print to the user, e.g.:
    ///   "  node 20.11.0 (from .nvmrc)"
    pub detection_lines: Vec<String>,
}

impl RunxConfig {
    /// Load `runx.toml` from `dir` and return it unchanged.
    /// Returns an error if the file does not exist or fails to parse.
    pub fn load_from_dir(dir: &Path) -> Result<Self> {
        let path = dir.join(CONFIG_FILE);
        if !path.exists() {
            return Err(UserError::new(format!(
                "No runx.toml found in {}.\nHint: run `runx init` to create a starter config.",
                dir.display()
            ))
            .into());
        }

        let raw = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        Self::from_str(&raw).with_context(|| format!("Failed to parse {}", path.display()))
    }

    pub fn from_str(raw: &str) -> Result<Self> {
        let config: Self = toml::from_str(raw)?;
        config.validate()?;
        Ok(config)
    }

    pub fn command(&self, key: &str) -> Result<&str> {
        self.run.get(key).map(String::as_str).ok_or_else(|| {
            let available = self.run.keys().cloned().collect::<Vec<_>>().join(", ");
            UserError::new(format!(
                "No run command named `{key}` found in runx.toml.\nAvailable commands: {available}"
            ))
            .into()
        })
    }

    fn validate(&self) -> Result<()> {
        if self.run.is_empty() {
            return Err(
                UserError::new("runx.toml must contain at least one command under [run].").into(),
            );
        }

        for (tool, version) in &self.runtimes {
            if tool.trim().is_empty() || version.trim().is_empty() {
                return Err(UserError::new("Runtime names and versions cannot be empty.").into());
            }
            validate_version_format(tool, version)?;
        }

        for (name, command) in &self.run {
            if name.trim().is_empty() || command.trim().is_empty() {
                return Err(UserError::new("Run command names and values cannot be empty.").into());
            }
        }

        Ok(())
    }
}

/// Validate that a runtime version string is in `MAJOR.MINOR.PATCH` form, where
/// each part is a non-negative integer (e.g. `20.11.0`).
///
/// This rejects loose specifiers like `latest` or `lts/iron` that would
/// otherwise pass the emptiness check and later produce a confusing 404.
fn validate_version_format(tool: &str, version: &str) -> Result<()> {
    let parts: Vec<&str> = version.splitn(4, '.').collect();
    let valid = parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()));
    if !valid {
        return Err(UserError::new(format!(
            "Invalid version `{version}` for runtime `{tool}`.\n\
             Expected MAJOR.MINOR.PATCH with numeric parts (e.g. 20.11.0).\n\
             Hint: use `runx init` to see example versions."
        ))
        .into());
    }
    Ok(())
}

/// Walk up the directory tree from `start`, returning the first directory that
/// looks like a project root.
///
/// A directory qualifies if it contains a `runx.toml`, or any standard
/// ecosystem version file (`.nvmrc`, `.node-version`, `.python-version`,
/// `package.json`, `pyproject.toml`). Returns `None` if the filesystem root is
/// reached without a match. This mirrors how cargo/npm/git locate their config.
pub fn find_project_dir(start: &Path) -> Option<PathBuf> {
    let mut current = start;
    loop {
        if current.join(CONFIG_FILE).exists() {
            return Some(current.to_path_buf());
        }
        // Also treat any directory containing detectable version files as a project root.
        let has_version_file = [
            ".nvmrc",
            ".node-version",
            ".python-version",
            "package.json",
            "pyproject.toml",
        ]
        .iter()
        .any(|f| current.join(f).exists());
        if has_version_file {
            return Some(current.to_path_buf());
        }
        match current.parent() {
            Some(parent) => current = parent,
            None => return None,
        }
    }
}

/// Load configuration for a project, falling back to auto-detection if no
/// `runx.toml` is present.
///
/// Resolution order:
/// 1. If `runx.toml` exists → load it as-is (explicit config always wins).
/// 2. Otherwise → scan for standard ecosystem version files via
///    [`detect::detect_runtimes`], synthesise an in-memory config, and return
///    a [`ResolvedConfig`] with `detection_lines` populated so the caller can
///    print the transparency banner.
/// 3. If neither a toml nor any detectable version info is found → return a
///    clear [`UserError`].
///
/// Note: auto-detection never writes a file; only `runx init` does that.
pub fn load_or_detect(dir: &Path) -> Result<ResolvedConfig> {
    let toml_path = dir.join(CONFIG_FILE);

    // ── Branch 1: runx.toml present ──────────────────────────────────────────
    if toml_path.exists() {
        let config = RunxConfig::load_from_dir(dir)?;
        return Ok(ResolvedConfig {
            inner: config,
            detection_lines: vec![],
        });
    }

    // ── Branch 2: auto-detection ──────────────────────────────────────────────
    let detected = detect::detect_runtimes(dir);

    // Collect transparency lines and print notes for range-collapsed versions
    let mut detection_lines: Vec<String> = vec![];
    if let Some(ref node) = detected.node {
        if node.range_collapsed {
            eprintln!(
                "  Note: node range in {} resolved to {} (minimum satisfying version)",
                node.source, node.version
            );
        }
        detection_lines.push(format!("  node {} (from {})", node.version, node.source));
    }
    if let Some(ref python) = detected.python {
        if python.range_collapsed {
            eprintln!(
                "  Note: python range in {} resolved to {} (minimum satisfying version)",
                python.source, python.version
            );
        }
        detection_lines.push(format!(
            "  python {} (from {})",
            python.version, python.source
        ));
    }

    // Build the runtimes map
    let runtimes = detect::detected_runtimes_map(&detected);

    if runtimes.is_empty() {
        // Nothing was detected at all
        return Err(UserError::new(format!(
            "No runx.toml found in {dir} and no standard version files were detected.\n\
             Hint: run `runx init` to create a starter config, or add a .nvmrc / package.json.",
            dir = dir.display()
        ))
        .into());
    }

    // Determine the run command
    let run = if let Some(cmd) = detected.inferred_dev_command {
        let mut m = BTreeMap::new();
        m.insert("dev".to_string(), cmd);
        m
    } else {
        // We found runtimes but cannot infer a dev command
        return Err(UserError::new(format!(
            "No runx.toml found in {dir}.\n\
             Detected runtimes from project files but could not infer a run command \
             (no `dev` script in package.json).\n\
             Hint: run `runx init` to create a runx.toml with the detected runtimes, \
             or add a `dev` script to package.json.",
            dir = dir.display()
        ))
        .into());
    };

    let config = RunxConfig { runtimes, run };
    Ok(ResolvedConfig {
        inner: config,
        detection_lines,
    })
}

pub fn starter_config() -> &'static str {
    r#"[runtimes]
node = "20.11.0"

[run]
dev = "node --version"
build = "node --version"
"#
}

#[cfg(test)]
mod tests {
    use super::{load_or_detect, RunxConfig, CONFIG_FILE};
    use std::fs;
    use tempfile::TempDir;

    fn tmp() -> TempDir {
        tempfile::tempdir().expect("create temp dir")
    }

    #[test]
    fn parses_sample_config() {
        let raw = r#"
[runtimes]
node = "20.11.0"
python = "3.11.7"

[run]
dev = "npm run dev"
build = "npm run build"
"#;

        let config = RunxConfig::from_str(raw).expect("sample config should parse");
        assert_eq!(config.runtimes["node"], "20.11.0");
        assert_eq!(config.runtimes["python"], "3.11.7");
        assert_eq!(config.command("dev").expect("dev command"), "npm run dev");
    }

    /// An existing runx.toml must always win over auto-detection sources.
    #[test]
    fn toml_wins_over_auto_detection() {
        let dir = tmp();

        // Write a runx.toml with node 18.0.0
        fs::write(
            dir.path().join(CONFIG_FILE),
            "[runtimes]\nnode = \"18.0.0\"\n\n[run]\ndev = \"node --version\"\n",
        )
        .unwrap();

        // Also write a .nvmrc with a DIFFERENT version
        fs::write(dir.path().join(".nvmrc"), "v20.11.0").unwrap();

        let resolved = load_or_detect(dir.path()).expect("should load without error");

        // The toml version must win, .nvmrc must be ignored
        assert_eq!(
            resolved.inner.runtimes["node"], "18.0.0",
            "runx.toml must take priority over .nvmrc"
        );
        assert!(
            resolved.detection_lines.is_empty(),
            "detection_lines should be empty when runx.toml is used"
        );
    }

    /// Auto-detection should synthesize a config when only a package.json
    /// (with engines + dev script) exists.
    #[test]
    fn auto_detects_from_package_json() {
        let dir = tmp();
        fs::write(
            dir.path().join("package.json"),
            r#"{
  "engines": { "node": "20.11.0" },
  "scripts": { "dev": "node index.js" }
}"#,
        )
        .unwrap();

        let resolved = load_or_detect(dir.path()).expect("should auto-detect");

        assert_eq!(resolved.inner.runtimes["node"], "20.11.0");
        assert_eq!(resolved.inner.run["dev"], "npm run dev");
        assert!(
            !resolved.detection_lines.is_empty(),
            "detection_lines should be populated"
        );
    }

    /// If nothing is present at all, a clear error should be returned.
    #[test]
    fn returns_clear_error_when_nothing_detected() {
        let dir = tmp();
        let err = load_or_detect(dir.path()).expect_err("should error");
        let msg = err.to_string();
        assert!(
            msg.contains("No runx.toml found"),
            "error should mention missing runx.toml: {msg}"
        );
        assert!(
            msg.contains("runx init"),
            "error should hint at runx init: {msg}"
        );
    }
}
