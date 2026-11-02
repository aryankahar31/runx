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
    /// `[run]` is optional: a project may only need `runx install`/`runx lock`,
    /// which need runtimes but no commands. Dispatch rejects an empty map with
    /// a message that names the missing section.
    #[serde(default)]
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

/// Validate that a runtime version entry is a requirement runx understands.
///
/// Accepts an exact pin (`20.11.0`) or a range (`>=20`, `^20.11`, `~=3.11`).
/// A range is resolved to a concrete release later, once the published version
/// list is available, so this layer only rejects text that is not a version
/// requirement at all — `lts/iron`, a path, a shell fragment.
///
/// The strict `MAJOR.MINOR.PATCH` check still runs, but at the point where it
/// matters: [`crate::runtime::resolve_runtime`] validates the *concrete* version
/// immediately before it is interpolated into the cache path and download URL.
/// Requirement text never reaches either, and `Req::parse` itself only accepts
/// digits, dots and comparison operators, so a traversal payload is refused
/// twice over.
fn validate_version_format(tool: &str, version: &str) -> Result<()> {
    if crate::version::Req::parse(version).is_some() {
        return Ok(());
    }

    Err(UserError::new(format!(
        "Invalid version requirement `{version}` for runtime `{tool}`.\n\
         Expected an exact version (e.g. 20.11.0) or a range (e.g. >=20, ^20.11).\n\
         Hint: aliases such as `lts/iron` or `latest` are not supported."
    ))
    .into())
}

/// Walk up the directory tree from `start`, returning the first directory that
/// looks like a project root.
///
/// A directory qualifies if it contains a `runx.toml`, or any standard
/// ecosystem version file (`.nvmrc`, `.node-version`, `.python-version`,
/// `package.json`, `pyproject.toml`, `go.mod`, `.go-version`, `bun.lock`,
/// `bun.lockb`, `.bun-version`, `bunfig.toml`, `.dvmrc`, `.deno-version`).
/// Returns `None` if the filesystem root is reached without a match. This
/// mirrors how cargo/npm/git locate their config.
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
            ".go-version",
            ".bun-version",
            ".dvmrc",
            ".deno-version",
            "package.json",
            "pyproject.toml",
            "Pipfile",
            "go.mod",
            "bun.lock",
            "bun.lockb",
            "bunfig.toml",
        ]
        .iter()
        .any(|f| current.join(f).exists());
        if has_version_file {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
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
    load_or_detect_impl(dir, true)
}

/// Like [`load_or_detect`], but for commands that only need runtimes.
///
/// `runx install` and `runx lock` have no use for `[run]`, so a Go or Python
/// project whose scripts cannot be inferred is a valid input rather than an
/// error.
pub fn load_or_detect_runtimes_only(dir: &Path) -> Result<ResolvedConfig> {
    load_or_detect_impl(dir, false)
}

fn load_or_detect_impl(dir: &Path, require_commands: bool) -> Result<ResolvedConfig> {
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

    // Transparency lines naming the requirement and the file it came from. The
    // concrete version is reported later, by whoever resolves the requirement,
    // since choosing it may need the published release list.
    let mut detection_lines: Vec<String> = vec![];
    for (tool, slot) in detected.slots() {
        let Some(runtime) = slot.and_then(detect::Detected::found) else {
            continue;
        };
        // An open-ended requirement (`*`, from a lockfile with no version)
        // reads better without the star: `bun (from bun.lock)`.
        let requirement = if runtime.requirement == "*" {
            String::new()
        } else {
            format!(" {}", runtime.requirement)
        };
        detection_lines.push(format!("  {tool}{requirement} (from {})", runtime.source));
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
             Hint: run `runx init` to create a starter config, or add a .nvmrc / package.json / go.mod.",
            dir = dir.display()
        ))
        .into());
    }

    // Determine the run commands. Detection and command inference are
    // separate problems: a project can have valid runtime requirements and no
    // package.json scripts at all. Every script becomes a command (keyed by
    // script name), so `runx test` works without a runx.toml.
    if require_commands && detected.inferred_commands.is_empty() {
        let detected_block = if detection_lines.is_empty() {
            String::new()
        } else {
            format!(
                "Detected runtimes from project files:\n{}\n",
                detection_lines.join("\n")
            )
        };
        let reason = if dir.join("package.json").is_file() {
            "this project's package.json defines no scripts".to_string()
        } else {
            "runx infers commands from the `scripts` in a package.json, \
              and this project has none"
                .to_string()
        };
        return Err(UserError::new(format!(
            "No runx.toml found in {dir}.\n\
             {detected_block}\
             Could not infer any run command: {reason}.\n\
             Hint: run `runx init` to create a runx.toml and define commands \
             under [run].",
            dir = dir.display()
        ))
        .into());
    }

    // Route through `from_parts` so detected versions face the same validation
    // as `runx.toml` ones. A struct literal here would reopen the traversal.
    let config = RunxConfig::from_parts(runtimes, detected.inferred_commands)?;
    Ok(ResolvedConfig {
        inner: config,
        detection_lines,
    })
}

pub fn starter_config() -> &'static str {
    r#"[runtimes]
node = "20.11.0"
bun = "1.2.3"

[run]
dev = "npm run dev"
build = "npm run build"
"#
}

/// The starter config `runx init` writes for `dir`: the runtimes the project
/// already declares and the commands its package.json already implies.
///
/// The fixed [`starter_config`] names node and npm in a Python-only project,
/// which then fails `runx install` — the generated file has to describe the
/// directory it lands in. Falls back to the fixed starter when nothing is
/// detected, so an empty directory still gets a runnable example.
pub fn starter_config_for(dir: &Path) -> String {
    let detected = detect::detect_runtimes(dir);
    let mut runtimes = detect::detected_runtimes_map(&detected);
    let commands = detected.inferred_commands;

    if runtimes.is_empty() && commands.is_empty() {
        return starter_config().to_string();
    }
    // Scripts with no version hint still need a runtime to run them.
    if runtimes.is_empty() {
        runtimes.insert("node".to_string(), "20.11.0".to_string());
    }

    let mut out = String::from("[runtimes]\n");
    for (tool, requirement) in &runtimes {
        out.push_str(&format!("{tool} = \"{}\"\n", toml_string(requirement)));
    }
    if !commands.is_empty() {
        out.push_str("\n[run]\n");
        for (key, command) in &commands {
            out.push_str(&format!("{key} = \"{}\"\n", toml_string(command)));
        }
    }
    out
}

/// Quote a value for a TOML basic string.
fn toml_string(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::{
        find_project_dir, load_or_detect, load_or_detect_runtimes_only, starter_config,
        starter_config_for, RunxConfig, CONFIG_FILE,
    };
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

    /// Anything that is not a version requirement must be refused, including the
    /// traversal payload that was reachable through auto-detection.
    #[test]
    fn rejects_non_requirement_versions_in_toml() {
        for bad in [
            "../../../../tmp/pwned",
            "..",
            "/etc/passwd",
            "lts/iron",
            // A channel alias, not a constraint: it has no lower bound, so
            // treating it as a wildcard resolved to 0.0.0.
            "latest",
            "nightly",
            "20.11.0 && echo hi",
            "$(whoami)",
        ] {
            let raw = format!("[runtimes]\nnode = \"{bad}\"\n\n[run]\ndev = \"true\"\n");
            assert!(
                RunxConfig::parse_toml(&raw).is_err(),
                "version {bad:?} must be rejected"
            );
        }
    }

    /// Ranges are now valid in `runx.toml`, because the concrete version is
    /// chosen later against the published release list. The strict
    /// `MAJOR.MINOR.PATCH` check still applies at `resolve_runtime`, immediately
    /// before the version reaches a cache path or download URL.
    #[test]
    fn accepts_version_requirements_in_toml() {
        for good in [
            "20.11.0",
            "v20.11.0",
            // Partial versions are X-ranges, as in node-semver and nvm.
            "20",
            "20.11",
            ">=20",
            "^20.11",
            "~20.11",
            "~=3.11",
            "<20",
            ">=14 <17",
            "18 || >=20",
            "3.11.*",
        ] {
            let raw = format!("[runtimes]\nnode = \"{good}\"\n\n[run]\ndev = \"true\"\n");
            assert!(
                RunxConfig::parse_toml(&raw).is_ok(),
                "requirement {good:?} should be accepted"
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

    /// A directory with only a `go.mod` is a project root, so detection can
    /// find it walking up from a subdirectory.
    #[test]
    fn go_mod_marks_a_project_root() {
        let dir = tmp();
        fs::write(dir.path().join("go.mod"), "module m\n\ngo 1.22.5\n").unwrap();
        fs::create_dir(dir.path().join("cmd")).unwrap();

        let root = find_project_dir(&dir.path().join("cmd")).expect("go.mod marks the root");
        assert_eq!(root, dir.path());
    }

    /// Auto-detection synthesises a config from a `go.mod` + `package.json`
    /// pair even without a runx.toml.
    #[test]
    fn auto_detects_go_from_gomod() {
        let dir = tmp();
        fs::write(dir.path().join("go.mod"), "module m\n\ngo 1.22.5\n").unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"dev": "go run ."}}"#,
        )
        .unwrap();

        let resolved = load_or_detect(dir.path()).expect("should auto-detect");
        assert_eq!(resolved.inner.runtimes["go"], "1.22.5");
        assert_eq!(resolved.inner.run["dev"], "npm run dev");
    }

    // ── Multi-runtime auto-detection ────────────────────────────────────────

    /// A mixed project is detected as a collection of requirements, not a
    /// single winner: Python from `.python-version`, Bun from `bun.lock`.
    #[test]
    fn auto_detects_python_and_bun_together() {
        let dir = tmp();
        fs::write(dir.path().join(".python-version"), "3.13\n").unwrap();
        fs::write(dir.path().join("bun.lock"), "{}\n").unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"dev": "bun run spec"}}"#,
        )
        .unwrap();

        let resolved = load_or_detect(dir.path()).expect("should auto-detect");
        assert_eq!(resolved.inner.runtimes["python"], "3.13");
        assert_eq!(resolved.inner.runtimes["bun"], "*");
        assert_eq!(
            resolved.inner.run["dev"], "bun run dev",
            "a bun.lock project infers `bun run dev`"
        );
    }

    /// The detection banner renders an open-ended lockfile requirement without
    /// the star: `bun (from bun.lock)`, not `bun * (from bun.lock)`.
    #[test]
    fn detection_lines_omit_the_open_ended_star() {
        let dir = tmp();
        fs::write(dir.path().join("bun.lock"), "{}\n").unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"dev": "next dev"}}"#,
        )
        .unwrap();

        let resolved = load_or_detect(dir.path()).expect("should auto-detect");
        assert_eq!(resolved.detection_lines, vec!["  bun (from bun.lock)"]);
    }

    /// A Go project with no package.json has a valid runtime requirement but
    /// no inferable command. The error must name what was detected, say why
    /// no command follows, and point at `runx init` — it must not claim a
    /// `dev` script is missing from a package.json that does not exist.
    #[test]
    fn go_project_without_command_inference_errors_clearly() {
        let dir = tmp();
        fs::write(dir.path().join("go.mod"), "module m\n\ngo 1.22.5\n").unwrap();

        let err = load_or_detect(dir.path()).expect_err("no command can be inferred");
        let message = format!("{err:#}");
        assert!(
            message.contains("go 1.22.5"),
            "should name the detection: {message}"
        );
        assert!(
            message.contains("scripts"),
            "should explain inference: {message}"
        );
        assert!(
            message.contains("runx init"),
            "should point at init: {message}"
        );
    }

    /// A package.json without a `dev` script is no longer an error: every
    /// script becomes a command, so `runx build` works on detected-only
    /// projects (the audit finding that only `dev` was reachable).
    #[test]
    fn non_dev_scripts_become_commands_without_a_toml() {
        let dir = tmp();
        fs::write(
            dir.path().join("package.json"),
            r#"{"engines": {"node": ">=20"}, "scripts": {"build": "node build.js", "test": "node test.js"}}"#,
        )
        .unwrap();

        let resolved = load_or_detect(dir.path()).expect("scripts should become commands");
        assert_eq!(resolved.inner.runtimes["node"], ">=20");
        assert_eq!(resolved.inner.run["build"], "npm run build");
        assert_eq!(resolved.inner.run["test"], "npm run test");
    }

    /// A package.json whose scripts object is empty still errors, naming the
    /// actual reason.
    #[test]
    fn package_json_with_no_scripts_errors_clearly() {
        let dir = tmp();
        fs::write(
            dir.path().join("package.json"),
            r#"{"engines": {"node": ">=20"}}"#,
        )
        .unwrap();

        let err = load_or_detect(dir.path()).expect_err("no scripts to infer from");
        let message = format!("{err:#}");
        assert!(
            message.contains("defines no scripts"),
            "should explain inference: {message}"
        );
        assert!(
            message.contains("runx init"),
            "should point at init: {message}"
        );
    }

    /// `runx install` and `runx lock` need runtimes, not commands: the project
    /// that fails `runx dev` above loads cleanly when commands are optional.
    #[test]
    fn runtimes_only_load_tolerates_missing_commands() {
        let dir = tmp();
        fs::write(dir.path().join("go.mod"), "module m\n\ngo 1.22.5\n").unwrap();

        load_or_detect(dir.path()).expect_err("run paths still demand a command");

        let resolved =
            load_or_detect_runtimes_only(dir.path()).expect("install must not need commands");
        assert_eq!(resolved.inner.runtimes["go"], "1.22.5");
        assert!(resolved.inner.run.is_empty());
    }

    /// `[run]` is optional: a config that only pins runtimes is valid for
    /// install/lock, and run dispatch reports the missing section by name.
    #[test]
    fn toml_without_a_run_section_parses() {
        let config =
            RunxConfig::parse_toml("[runtimes]\nnode = \"20.11.0\"\n").expect("run is optional");
        assert_eq!(config.runtimes["node"], "20.11.0");
        assert!(config.run.is_empty());
    }

    /// `runx init` describes the project it lands in: Python runtimes from
    /// `.python-version`, not the node/npm default that made `runx install`
    /// fail on a Python-only directory.
    #[test]
    fn starter_config_describes_a_python_only_project() {
        let dir = tmp();
        fs::write(dir.path().join(".python-version"), "3.11.7\n").unwrap();

        let starter = starter_config_for(dir.path());
        assert!(starter.contains("python = \"3.11.7\""), "{starter}");
        assert!(
            !starter.contains("npm run"),
            "a Python project must not get npm commands: {starter}"
        );
    }

    /// Detected requirements and inferred scripts land in the starter, and the
    /// generated document still parses as a runx config.
    #[test]
    fn starter_config_carries_detected_runtimes_and_commands() {
        let dir = tmp();
        fs::write(dir.path().join(".nvmrc"), "20.11.0\n").unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"dev": "node server.js"}}"#,
        )
        .unwrap();

        let starter = starter_config_for(dir.path());
        assert!(starter.contains("node = \"20.11.0\""), "{starter}");
        assert!(starter.contains("dev = \"npm run dev\""), "{starter}");

        let parsed = RunxConfig::parse_toml(&starter).expect("starter must parse");
        assert_eq!(parsed.runtimes["node"], "20.11.0");
        assert_eq!(parsed.run["dev"], "npm run dev");
    }

    /// Nothing detected → the fixed starter, so an empty directory still gets
    /// a runnable example.
    #[test]
    fn starter_config_falls_back_when_nothing_is_detected() {
        let dir = tmp();
        assert_eq!(starter_config_for(dir.path()), starter_config());
    }

    /// `bun.lock` marks a project root even when nothing else does.
    #[test]
    fn bun_lock_marks_a_project_root() {
        let dir = tmp();
        fs::write(dir.path().join("bun.lock"), "{}\n").unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();

        let root = find_project_dir(&dir.path().join("src")).expect("bun.lock marks the root");
        assert_eq!(root, dir.path());
    }

    /// `.go-version` marks a project root too.
    #[test]
    fn go_version_file_marks_a_project_root() {
        let dir = tmp();
        fs::write(dir.path().join(".go-version"), "1.23.4\n").unwrap();

        let root = find_project_dir(dir.path()).expect(".go-version marks the root");
        assert_eq!(root, dir.path());
    }

    /// `runx.toml` still wins over a bun.lock's open-ended requirement.
    #[test]
    fn toml_wins_over_lockfile_only_bun() {
        let dir = tmp();
        fs::write(dir.path().join("bun.lock"), "{}\n").unwrap();
        fs::write(
            dir.path().join(CONFIG_FILE),
            "[runtimes]\nbun = \"1.3.14\"\n\n[run]\ndev = \"bun run dev\"\n",
        )
        .unwrap();

        let resolved = load_or_detect(dir.path()).expect("should load");
        assert_eq!(resolved.inner.runtimes["bun"], "1.3.14");
        assert!(resolved.detection_lines.is_empty());
    }
}
