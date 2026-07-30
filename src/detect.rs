//! Auto-detection of runtime versions from standard project files.
//!
//! Detection priority (first match wins):
//!
//! **Node.js**
//! 1. `.nvmrc`
//! 2. `.node-version`
//! 3. `package.json` → `engines.node`
//!
//! **Python**
//! 1. `.python-version`
//! 2. `pyproject.toml` → `[project].requires-python`
//!
//! **Range resolution**: version hints are parsed with [`crate::version::Req`].
//! A hint that cannot be turned into a concrete version — because it has no
//! lower bound (`<20`), because it excludes its own bound (`!=3.11`), or
//! because it is not a version at all (`lts/iron`) — is reported as
//! [`Detected::Unresolvable`] rather than silently guessed. Guessing is how
//! `<20` used to resolve to `20.0.0`, a version the constraint forbids.

use crate::version::{Req, Version};
use std::{collections::BTreeMap, fs, path::Path};

// ── Public types ─────────────────────────────────────────────────────────────

/// A single detected runtime version and the file it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedRuntime {
    /// The concrete version string (e.g. `"20.11.0"`).
    pub version: String,
    /// Human-readable source description (e.g. `".nvmrc"`).
    pub source: String,
    /// The original requirement text as written in the project file.
    pub requirement: String,
    /// Set to `true` when the version was resolved from a range rather than
    /// read verbatim.  Callers should print a note in that case.
    pub range_collapsed: bool,
}

/// The outcome of inspecting one runtime's version hints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detected {
    /// A concrete version was determined.
    Version(DetectedRuntime),
    /// A hint was found but could not be resolved to a concrete version.
    ///
    /// This is deliberately *not* an `Option::None`: the difference between
    /// "this project says nothing about Node" and "this project asks for a Node
    /// version I cannot pin down" matters to the user, and collapsing the two
    /// produces a misleading "nothing detected" error.
    Unresolvable {
        source: String,
        requirement: String,
        reason: String,
    },
}

impl Detected {
    /// The resolved runtime, if resolution succeeded.
    pub fn resolved(&self) -> Option<&DetectedRuntime> {
        match self {
            Self::Version(runtime) => Some(runtime),
            Self::Unresolvable { .. } => None,
        }
    }
}

/// The result of scanning a project directory for runtime version hints.
#[derive(Debug, Default)]
pub struct DetectionResult {
    pub node: Option<Detected>,
    pub python: Option<Detected>,
    /// Shell command inferred from `package.json` `scripts.dev`, if present.
    /// Currently only `"npm run dev"` is inferred — no other heuristics are
    /// attempted, per the v0.2 scope.
    pub inferred_dev_command: Option<String>,
}

impl DetectionResult {
    /// Every hint that was found but could not be resolved, as printable lines.
    pub fn unresolvable(&self) -> Vec<String> {
        [("node", &self.node), ("python", &self.python)]
            .into_iter()
            .filter_map(|(tool, slot)| match slot.as_ref()? {
                Detected::Unresolvable {
                    source,
                    requirement,
                    reason,
                } => Some(format!(
                    "  {tool} `{requirement}` (from {source}) — {reason}"
                )),
                Detected::Version(_) => None,
            })
            .collect()
    }
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Scan `dir` for standard ecosystem version files and return whatever could
/// be detected.  Returns a `DetectionResult` with `None` fields for anything
/// not found; callers decide how to handle missing data.
///
/// This function is purely read-only — it never writes to disk.
pub fn detect_runtimes(dir: &Path) -> DetectionResult {
    DetectionResult {
        node: detect_node(dir),
        python: detect_python(dir),
        inferred_dev_command: infer_dev_command(dir),
    }
}

// ── Node.js detection ─────────────────────────────────────────────────────────

fn detect_node(dir: &Path) -> Option<Detected> {
    // Priority 1 and 2: plain-text version files.
    for filename in [".nvmrc", ".node-version"] {
        if let Some(raw) = read_plain_version_file(dir, filename) {
            return Some(resolve_hint(&raw, filename));
        }
    }

    // Priority 3: package.json engines.node
    detect_node_from_package_json(dir)
}

fn detect_node_from_package_json(dir: &Path) -> Option<Detected> {
    let raw = read_file_to_string_lossy(&dir.join("package.json"))?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let node_range = json
        .get("engines")
        .and_then(|engines| engines.get("node"))
        .and_then(|value| value.as_str())?;

    Some(resolve_hint(node_range, "package.json (engines.node)"))
}

// ── Python detection ──────────────────────────────────────────────────────────

fn detect_python(dir: &Path) -> Option<Detected> {
    // Priority 1: .python-version
    if let Some(raw) = read_plain_version_file(dir, ".python-version") {
        return Some(resolve_hint(&raw, ".python-version"));
    }

    // Priority 2: pyproject.toml [project].requires-python
    detect_python_from_pyproject(dir)
}

fn detect_python_from_pyproject(dir: &Path) -> Option<Detected> {
    let raw = read_file_to_string_lossy(&dir.join("pyproject.toml"))?;
    let doc: toml::Value = toml::from_str(&raw).ok()?;
    let requires = doc
        .get("project")
        .and_then(|project| project.get("requires-python"))
        .and_then(|value| value.as_str())?;

    Some(resolve_hint(requires, "pyproject.toml (requires-python)"))
}

// ── Run-command inference ─────────────────────────────────────────────────────

/// Return `Some("npm run dev")` if `package.json` has a `"dev"` script.
/// No other commands are inferred — this is the only well-defined heuristic
/// in v0.2 scope.
fn infer_dev_command(dir: &Path) -> Option<String> {
    let raw = read_file_to_string_lossy(&dir.join("package.json"))?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let has_dev = json
        .get("scripts")
        .and_then(|scripts| scripts.get("dev"))
        .is_some();
    has_dev.then(|| "npm run dev".to_string())
}

// ── Shared helpers ────────────────────────────────────────────────────────────

/// Read a file to string, supporting UTF-16 LE/BE with BOM, UTF-8 with BOM,
/// and standard UTF-8.
///
/// UTF-16 matters on Windows: `Set-Content` and `>` in PowerShell 5.1 write
/// UTF-16 LE by default, so a `.nvmrc` created with `"20" > .nvmrc` is UTF-16.
fn read_file_to_string_lossy(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    if bytes.is_empty() {
        return Some(String::new());
    }

    match bytes.as_slice() {
        // UTF-16 LE with BOM.
        [0xFF, 0xFE, rest @ ..] => decode_utf16(rest, u16::from_le_bytes),
        // UTF-16 BE with BOM.
        [0xFE, 0xFF, rest @ ..] => decode_utf16(rest, u16::from_be_bytes),
        // UTF-8 with BOM.
        [0xEF, 0xBB, 0xBF, rest @ ..] => Some(String::from_utf8_lossy(rest).into_owned()),
        // Standard UTF-8.
        _ => Some(String::from_utf8_lossy(&bytes).into_owned()),
    }
}

/// Decode UTF-16 code units, tolerating a trailing odd byte and unpaired
/// surrogates rather than discarding the whole file.
///
/// The previous implementation used `String::from_utf16(...).ok()?`, so a single
/// unpaired surrogate anywhere in the file made the entire version hint vanish
/// and silently fell through to the next detection source.
fn decode_utf16(bytes: &[u8], to_u16: fn([u8; 2]) -> u16) -> Option<String> {
    let units: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|chunk| to_u16([chunk[0], chunk[1]]))
        .collect();
    Some(String::from_utf16_lossy(&units))
}

/// Read a plain-text version file and return its first non-empty line.
///
/// Only the first line is considered: `.nvmrc` files sometimes carry a trailing
/// comment or blank line, and feeding the whole buffer to the parser would
/// reject an otherwise valid file.
fn read_plain_version_file(dir: &Path, filename: &str) -> Option<String> {
    let raw = read_file_to_string_lossy(&dir.join(filename))?;
    let line = raw
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#'))?;
    Some(line.to_string())
}

/// Turn one raw version hint into a [`Detected`].
///
/// Resolution uses the *minimum* version satisfying the requirement. When no
/// minimum is defensible the hint is reported as unresolvable instead of being
/// guessed at.
fn resolve_hint(raw: &str, source: &str) -> Detected {
    let requirement = raw.trim().to_string();

    let unresolvable = |reason: &str| Detected::Unresolvable {
        source: source.to_string(),
        requirement: requirement.clone(),
        reason: reason.to_string(),
    };

    let Some(req) = Req::parse(&requirement) else {
        return unresolvable(
            "not a recognised version or range (aliases like `lts/*` are not supported)",
        );
    };

    match req.minimum() {
        Some(version) => Detected::Version(DetectedRuntime {
            version: version.to_three_parts(),
            source: source.to_string(),
            requirement: requirement.clone(),
            range_collapsed: !req.exact,
        }),
        // e.g. `<20` (no floor) or `!=3.11` (excludes its own bound).
        None => unresolvable("no concrete lowest version satisfies this range"),
    }
}

/// Resolve a version requirement to the minimum satisfying version.
///
/// Returns `None` when the requirement cannot be resolved. Prefer
/// [`Req::best_match`] when a real release list is available, since resolving
/// to the newest matching release is what nvm/volta/mise do.
pub fn resolve_semver_range(range: &str) -> Option<(String, bool)> {
    let req = Req::parse(range)?;
    let minimum = req.minimum()?;
    Some((minimum.to_three_parts(), !req.exact))
}

/// Highest version in `available` satisfying `requirement`.
pub fn resolve_to_latest(requirement: &str, available: &[Version]) -> Option<String> {
    let req = Req::parse(requirement)?;
    req.best_match(available).map(|best| best.to_three_parts())
}

/// Build the map form expected by `RunxConfig::runtimes`, including only
/// runtimes that resolved to a concrete version.
pub fn detected_runtimes_map(result: &DetectionResult) -> BTreeMap<String, String> {
    [("node", &result.node), ("python", &result.python)]
        .into_iter()
        .filter_map(|(tool, slot)| {
            let resolved = slot.as_ref()?.resolved()?;
            Some((tool.to_string(), resolved.version.clone()))
        })
        .collect()
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn tmp() -> TempDir {
        tempfile::tempdir().expect("create temp dir")
    }

    /// Resolved runtime for `node`, panicking with context if unresolved.
    fn node_of(result: &DetectionResult) -> &DetectedRuntime {
        result
            .node
            .as_ref()
            .expect("node should be detected")
            .resolved()
            .expect("node should resolve to a concrete version")
    }

    fn python_of(result: &DetectionResult) -> &DetectedRuntime {
        result
            .python
            .as_ref()
            .expect("python should be detected")
            .resolved()
            .expect("python should resolve to a concrete version")
    }

    // ── Node.js detection ─────────────────────────────────────────────────────

    #[test]
    fn detects_node_from_nvmrc() {
        let dir = tmp();
        fs::write(dir.path().join(".nvmrc"), "v20.11.0\n").unwrap();

        let result = detect_runtimes(dir.path());
        let node = node_of(&result);
        assert_eq!(node.version, "20.11.0");
        assert_eq!(node.source, ".nvmrc");
        assert!(!node.range_collapsed);
    }

    #[test]
    fn detects_node_from_node_version_file() {
        let dir = tmp();
        fs::write(dir.path().join(".node-version"), "18.20.3").unwrap();

        let result = detect_runtimes(dir.path());
        let node = node_of(&result);
        assert_eq!(node.version, "18.20.3");
        assert_eq!(node.source, ".node-version");
    }

    #[test]
    fn detects_node_from_package_json_engines() {
        let dir = tmp();
        fs::write(
            dir.path().join("package.json"),
            r#"{"engines": {"node": ">=20.11.0"}}"#,
        )
        .unwrap();

        let result = detect_runtimes(dir.path());
        let node = node_of(&result);
        assert_eq!(node.version, "20.11.0");
        assert_eq!(node.source, "package.json (engines.node)");
        assert!(node.range_collapsed, ">=20.11.0 is a range");
    }

    #[test]
    fn returns_none_when_no_node_files_present() {
        let dir = tmp();
        assert!(detect_runtimes(dir.path()).node.is_none());
    }

    #[test]
    fn nvmrc_wins_over_package_json_engines() {
        let dir = tmp();
        fs::write(dir.path().join(".nvmrc"), "v20.11.0").unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"engines": {"node": "18.0.0"}}"#,
        )
        .unwrap();

        let result = detect_runtimes(dir.path());
        assert_eq!(node_of(&result).version, "20.11.0");
        assert_eq!(node_of(&result).source, ".nvmrc");
    }

    #[test]
    fn node_version_file_wins_over_package_json_engines() {
        let dir = tmp();
        fs::write(dir.path().join(".node-version"), "18.20.3").unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"engines": {"node": "16.0.0"}}"#,
        )
        .unwrap();

        assert_eq!(node_of(&detect_runtimes(dir.path())).version, "18.20.3");
    }

    #[test]
    fn nvmrc_wins_over_node_version_file() {
        let dir = tmp();
        fs::write(dir.path().join(".nvmrc"), "v20.11.0").unwrap();
        fs::write(dir.path().join(".node-version"), "18.20.3").unwrap();

        let result = detect_runtimes(dir.path());
        assert_eq!(node_of(&result).version, "20.11.0");
        assert_eq!(node_of(&result).source, ".nvmrc");
    }

    // ── Python detection ──────────────────────────────────────────────────────

    #[test]
    fn detects_python_from_python_version_file() {
        let dir = tmp();
        fs::write(dir.path().join(".python-version"), "3.11.7\n").unwrap();

        let result = detect_runtimes(dir.path());
        let python = python_of(&result);
        assert_eq!(python.version, "3.11.7");
        assert_eq!(python.source, ".python-version");
        assert!(!python.range_collapsed);
    }

    #[test]
    fn detects_python_from_pyproject_toml_requires_python() {
        let dir = tmp();
        fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nrequires-python = \">=3.11\"\n",
        )
        .unwrap();

        let result = detect_runtimes(dir.path());
        let python = python_of(&result);
        assert_eq!(python.version, "3.11.0");
        assert!(python.range_collapsed);
    }

    #[test]
    fn python_version_file_wins_over_pyproject() {
        let dir = tmp();
        fs::write(dir.path().join(".python-version"), "3.12.0").unwrap();
        fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nrequires-python = \">=3.11\"\n",
        )
        .unwrap();

        assert_eq!(python_of(&detect_runtimes(dir.path())).version, "3.12.0");
    }

    // ── Run-command inference ─────────────────────────────────────────────────

    #[test]
    fn infers_npm_run_dev_when_dev_script_present() {
        let dir = tmp();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"dev": "node index.js"}}"#,
        )
        .unwrap();

        assert_eq!(
            detect_runtimes(dir.path()).inferred_dev_command.as_deref(),
            Some("npm run dev")
        );
    }

    #[test]
    fn no_inferred_command_when_no_dev_script() {
        let dir = tmp();
        fs::write(
            dir.path().join("package.json"),
            r#"{"scripts": {"build": "node build.js"}}"#,
        )
        .unwrap();

        assert!(detect_runtimes(dir.path()).inferred_dev_command.is_none());
    }

    #[test]
    fn no_inferred_command_when_no_package_json() {
        let dir = tmp();
        assert!(detect_runtimes(dir.path()).inferred_dev_command.is_none());
    }

    // ── Malicious / malformed hints ───────────────────────────────────────────

    /// A `.nvmrc` is attacker-controlled in any cloned repo. Its contents must
    /// never become a usable version, because the version lands in a cache path
    /// that gets recursively deleted and in a download URL.
    #[test]
    fn path_traversal_in_nvmrc_is_unresolvable() {
        let dir = tmp();
        fs::write(dir.path().join(".nvmrc"), "../../../../tmp/pwned").unwrap();

        let detected = detect_runtimes(dir.path()).node.expect("hint is present");
        assert!(
            detected.resolved().is_none(),
            "traversal payload must not resolve to a version"
        );
        assert!(matches!(detected, Detected::Unresolvable { .. }));
    }

    #[test]
    fn traversal_payload_is_excluded_from_runtimes_map() {
        let dir = tmp();
        fs::write(dir.path().join(".nvmrc"), "../../../evil").unwrap();

        let result = detect_runtimes(dir.path());
        assert!(
            detected_runtimes_map(&result).is_empty(),
            "unresolvable hints must not reach the runtimes map"
        );
        assert_eq!(result.unresolvable().len(), 1);
    }

    #[test]
    fn alias_in_nvmrc_is_reported_not_guessed() {
        let dir = tmp();
        fs::write(dir.path().join(".nvmrc"), "lts/iron").unwrap();

        let detected = detect_runtimes(dir.path()).node.expect("hint is present");
        match detected {
            Detected::Unresolvable {
                requirement,
                reason,
                source,
            } => {
                assert_eq!(requirement, "lts/iron");
                assert_eq!(source, ".nvmrc");
                assert!(reason.contains("lts"), "reason should mention aliases");
            }
            Detected::Version(runtime) => {
                panic!("lts/iron must not resolve, got {}", runtime.version)
            }
        }
    }

    /// Regression: `<20` used to resolve to `20.0.0`, which the range forbids.
    #[test]
    fn upper_bound_only_range_is_unresolvable_not_wrong() {
        let dir = tmp();
        fs::write(
            dir.path().join("package.json"),
            r#"{"engines": {"node": "<20"}}"#,
        )
        .unwrap();

        let detected = detect_runtimes(dir.path()).node.expect("hint is present");
        assert!(
            detected.resolved().is_none(),
            "<20 has no defensible minimum"
        );
    }

    /// Regression: `18 || >=20` used to produce the literal directory name
    /// `"18 || >=20.0.0"`.
    #[test]
    fn alternation_resolves_to_lowest_alternative() {
        let dir = tmp();
        fs::write(
            dir.path().join("package.json"),
            r#"{"engines": {"node": "18 || >=20"}}"#,
        )
        .unwrap();

        let result = detect_runtimes(dir.path());
        assert_eq!(node_of(&result).version, "18.0.0");
    }

    #[test]
    fn comments_and_blank_lines_in_version_files_are_skipped() {
        let dir = tmp();
        fs::write(dir.path().join(".nvmrc"), "\n# pinned for CI\n20.11.0\n").unwrap();

        assert_eq!(node_of(&detect_runtimes(dir.path())).version, "20.11.0");
    }

    #[test]
    fn empty_version_file_is_ignored() {
        let dir = tmp();
        fs::write(dir.path().join(".nvmrc"), "   \n\n").unwrap();
        assert!(detect_runtimes(dir.path()).node.is_none());
    }

    // ── Encoding ──────────────────────────────────────────────────────────────

    #[test]
    fn reads_utf16_le_with_bom() {
        let dir = tmp();
        let data: &[u8] = &[
            0xFF, 0xFE, // BOM
            b'2', 0x00, b'0', 0x00, b'.', 0x00, b'1', 0x00, b'1', 0x00, b'.', 0x00, b'0', 0x00,
            b'\n', 0x00,
        ];
        fs::write(dir.path().join(".nvmrc"), data).unwrap();

        assert_eq!(node_of(&detect_runtimes(dir.path())).version, "20.11.0");
    }

    #[test]
    fn reads_utf16_be_with_bom() {
        let dir = tmp();
        let data: &[u8] = &[
            0xFE, 0xFF, // BOM
            0x00, b'1', 0x00, b'8', 0x00, b'.', 0x00, b'2', 0x00, b'0', 0x00, b'.', 0x00, b'0',
        ];
        fs::write(dir.path().join(".nvmrc"), data).unwrap();

        assert_eq!(node_of(&detect_runtimes(dir.path())).version, "18.20.0");
    }

    #[test]
    fn reads_utf8_with_bom() {
        let dir = tmp();
        let mut data = vec![0xEF, 0xBB, 0xBF];
        data.extend_from_slice(b"20.11.0\n");
        fs::write(dir.path().join(".nvmrc"), data).unwrap();

        assert_eq!(node_of(&detect_runtimes(dir.path())).version, "20.11.0");
    }

    /// A UTF-16 file with an odd trailing byte previously produced a version;
    /// the important property is that it does not panic.
    #[test]
    fn odd_length_utf16_does_not_panic() {
        let dir = tmp();
        let data: &[u8] = &[0xFF, 0xFE, b'2', 0x00, b'0', 0x00, b'.'];
        fs::write(dir.path().join(".nvmrc"), data).unwrap();

        let _ = detect_runtimes(dir.path());
    }

    /// UTF-16 with an unpaired surrogate must not make the hint disappear.
    #[test]
    fn unpaired_surrogate_still_yields_a_hint() {
        let dir = tmp();
        // "20.11.0" followed by a lone high surrogate (0xD800).
        let mut data = vec![0xFF, 0xFE];
        for byte in b"20.11.0" {
            data.push(*byte);
            data.push(0x00);
        }
        data.extend_from_slice(&[0x00, 0xD8]);
        fs::write(dir.path().join(".nvmrc"), data).unwrap();

        let detected = detect_runtimes(dir.path());
        assert!(
            detected.node.is_some(),
            "a lone surrogate must not discard the whole file"
        );
    }

    /// A UTF-16 `package.json` must parse; previously `serde_json` received raw
    /// UTF-16 bytes because only plain version files went through the decoder.
    #[test]
    fn reads_utf16_package_json() {
        let dir = tmp();
        let text = r#"{"engines":{"node":"20.11.0"},"scripts":{"dev":"node ."}}"#;
        let mut data = vec![0xFF, 0xFE];
        for unit in text.encode_utf16() {
            data.extend_from_slice(&unit.to_le_bytes());
        }
        fs::write(dir.path().join("package.json"), data).unwrap();

        let result = detect_runtimes(dir.path());
        assert_eq!(node_of(&result).version, "20.11.0");
        assert_eq!(result.inferred_dev_command.as_deref(), Some("npm run dev"));
    }

    // ── Range resolution helpers ──────────────────────────────────────────────

    #[test]
    fn resolve_semver_range_reports_failure_instead_of_guessing() {
        assert_eq!(
            resolve_semver_range("20.11.0"),
            Some(("20.11.0".to_string(), false))
        );
        assert_eq!(
            resolve_semver_range(">=3.11"),
            Some(("3.11.0".to_string(), true))
        );
        assert_eq!(resolve_semver_range("<20"), None);
        assert_eq!(resolve_semver_range("lts/iron"), None);
    }

    #[test]
    fn resolve_to_latest_picks_newest_match() {
        let available: Vec<Version> = ["20.10.0", "20.11.0", "22.1.0"]
            .iter()
            .map(|raw| Version::parse(raw).unwrap())
            .collect();

        assert_eq!(
            resolve_to_latest(">=20", &available).as_deref(),
            Some("22.1.0")
        );
        assert_eq!(
            resolve_to_latest("^20", &available).as_deref(),
            Some("20.11.0")
        );
        assert_eq!(resolve_to_latest(">=24", &available), None);
    }
}
