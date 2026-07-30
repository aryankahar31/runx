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
        Self::parse_toml(&raw).with_context(|| format!("Failed to parse {}", path.display()))
    }

    /// Parse and validate a `runx.toml` document.
    ///
    /// Named `parse_toml` rather than `from_str` so it is not mistaken for
    /// `std::str::FromStr::from_str` (clippy::should_implement_trait).
    pub fn parse_toml(raw: &str) -> Result<Self> {
        let config: Self = toml::from_str(raw)?;
        config.validate()?;
        Ok(config)
    }

    /// Build a config from already-separated parts, applying the same
    /// validation as [`Self::parse_toml`].
    ///
    /// Auto-detection previously built `RunxConfig` with a struct literal,
    /// which skipped [`Self::validate`] entirely — that is how a `.nvmrc`
    /// containing `../../../etc` reached the cache path and download URL while
    /// the identical value in `runx.toml` was correctly rejected. Every
    /// construction path must go through validation.
    pub fn from_parts(
        runtimes: BTreeMap<String, String>,
        run: BTreeMap<String, String>,
    ) -> Result<Self> {
        let config = Self { runtimes, run };
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

/// Validate that a runtime version string is a concrete `MAJOR.MINOR.PATCH`.
///
/// Delegates to [`crate::version::validate_concrete`] so that `runx.toml` and
/// auto-detected versions are held to exactly the same rules. Keeping a second
/// hand-rolled check here is how the two paths drifted apart in the first
/// place: this one rejected `lts/iron`, while auto-detection accepted it.
fn validate_version_format(tool: &str, version: &str) -> Result<()> {
    crate::version::validate_concrete(tool, version)
        .map_err(|message| UserError::new(message).into())
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

    // Transparency lines for everything that resolved, plus a note whenever a
    // range was collapsed so the user sees which concrete version was picked.
    let mut detection_lines: Vec<String> = vec![];
    for (tool, slot) in [("node", &detected.node), ("python", &detected.python)] {
        let Some(runtime) = slot.as_ref().and_then(detect::Detected::resolved) else {
            continue;
        };
        if runtime.range_collapsed {
            eprintln!(
                "  Note: {tool} range `{}` in {} resolved to {} (lowest satisfying version)",
                runtime.requirement, runtime.source, runtime.version
            );
        }
        detection_lines.push(format!(
            "  {tool} {} (from {})",
            runtime.version, runtime.source
        ));
    }

    let runtimes = detect::detected_runtimes_map(&detected);

    if runtimes.is_empty() {
        // Distinguish "no version files at all" from "version files present but
        // unusable". Reporting the latter as "nothing detected" sends users
        // looking for a missing file that is actually right there.
        let unresolvable = detected.unresolvable();
        if !unresolvable.is_empty() {
            return Err(UserError::new(format!(
                "No runx.toml found in {dir}, and the version requirements that were \
                 found could not be resolved to a concrete version:\n{hints}\n\
                 Hint: pin an exact version (e.g. `20.11.0`), or run `runx init` to \
                 create a runx.toml.",
                dir = dir.display(),
                hints = unresolvable.join("\n")
            ))
            .into());
        }

        return Err(UserError::new(format!(
            "No runx.toml found in {dir} and no standard version files were detected.\n\
             Hint: run `runx init` to create a starter config, or add a .nvmrc / package.json.",
            dir = dir.display()
        ))
        .into());
    }

    // Determine the run command.
    let Some(dev_command) = detected.inferred_dev_command else {
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

    let run = BTreeMap::from([("dev".to_string(), dev_command)]);

    // Route through `from_parts` so detected versions face the same validation
    // as `runx.toml` ones. A struct literal here would reopen the traversal.
    let config = RunxConfig::from_parts(runtimes, run)?;
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

        let config = RunxConfig::parse_toml(raw).expect("sample config should parse");
        assert_eq!(config.runtimes["node"], "20.11.0");
        assert_eq!(config.runtimes["python"], "3.11.7");
        assert_eq!(config.command("dev").expect("dev command"), "npm run dev");
    }

    /// Backward compatibility: every runtime/version form that v0.2 accepted in
    /// a `runx.toml` must still parse.
    #[test]
    fn existing_toml_files_remain_valid() {
        let raw = r#"
[runtimes]
node = "20.11.0"
python = "3.11.7"

[run]
dev = "npm run dev"
build = "npm run build"
test = "npm test"
"#;
        let config = RunxConfig::parse_toml(raw).expect("v0.2 config must still parse");
        assert_eq!(config.runtimes.len(), 2);
        assert_eq!(config.run.len(), 3);
    }

    /// A `[runtimes]`-free config is valid — commands may need no runtime.
    #[test]
    fn runtimes_section_is_optional() {
        let config = RunxConfig::parse_toml("[run]\ndev = \"echo hi\"\n").expect("should parse");
        assert!(config.runtimes.is_empty());
    }

    /// The traversal payload proved reachable via auto-detection must be
    /// rejected when it arrives through `runx.toml` too.
    #[test]
    fn rejects_traversal_and_alias_versions_in_toml() {
        for bad in [
            "../../../../tmp/pwned",
            "lts/iron",
            "latest",
            "20",
            "v20.11.0",
            "20.11.0 && echo hi",
        ] {
            let raw = format!("[runtimes]\nnode = \"{bad}\"\n\n[run]\ndev = \"true\"\n");
            assert!(
                RunxConfig::parse_toml(&raw).is_err(),
                "version {bad:?} must be rejected"
            );
        }
    }

    /// Regression for the validation bypass: `from_parts` is the only way to
    /// build a config outside TOML parsing, and it must validate.
    #[test]
    fn from_parts_validates_versions() {
        use std::collections::BTreeMap;

        let runtimes = BTreeMap::from([("node".to_string(), "../../../evil".to_string())]);
        let run = BTreeMap::from([("dev".to_string(), "true".to_string())]);

        assert!(
            RunxConfig::from_parts(runtimes, run).is_err(),
            "from_parts must reject a traversal payload, not just parse_toml"
        );
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
