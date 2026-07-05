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
//! **Range resolution**: semver ranges are collapsed to the *minimum version
//! that satisfies the constraint* (e.g. `>=3.11` → `3.11.0`, `^20` → `20.0.0`).
//! A transparency message is always emitted when a range is collapsed so the
//! user knows which concrete version was chosen and why.

use std::{collections::BTreeMap, fs, path::Path};

// ── Public types ─────────────────────────────────────────────────────────────

/// A single detected runtime version and the file it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedRuntime {
    /// The concrete version string (e.g. `"20.11.0"`).
    pub version: String,
    /// Human-readable source description (e.g. `".nvmrc"`).
    pub source: String,
    /// Set to `true` when the version was resolved from a range rather than
    /// read verbatim.  Callers should print a note in that case.
    pub range_collapsed: bool,
}

/// The result of scanning a project directory for runtime version hints.
#[derive(Debug, Default)]
pub struct DetectionResult {
    pub node: Option<DetectedRuntime>,
    pub python: Option<DetectedRuntime>,
    /// Shell command inferred from `package.json` `scripts.dev`, if present.
    /// Currently only `"npm run dev"` is inferred — no other heuristics are
    /// attempted, per the v0.2 scope.
    pub inferred_dev_command: Option<String>,
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

fn detect_node(dir: &Path) -> Option<DetectedRuntime> {
    // Priority 1: .nvmrc
    if let Some(v) = read_plain_version_file(dir, ".nvmrc") {
        return Some(DetectedRuntime {
            version: v,
            source: ".nvmrc".to_string(),
            range_collapsed: false,
        });
    }

    // Priority 2: .node-version
    if let Some(v) = read_plain_version_file(dir, ".node-version") {
        return Some(DetectedRuntime {
            version: v,
            source: ".node-version".to_string(),
            range_collapsed: false,
        });
    }

    // Priority 3: package.json engines.node
    detect_node_from_package_json(dir)
}

fn detect_node_from_package_json(dir: &Path) -> Option<DetectedRuntime> {
    let raw = fs::read_to_string(dir.join("package.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let node_range = json
        .get("engines")
        .and_then(|e| e.get("node"))
        .and_then(|v| v.as_str())?;

    let (version, was_range) = resolve_semver_range(node_range);
    Some(DetectedRuntime {
        version,
        source: "package.json (engines.node)".to_string(),
        range_collapsed: was_range,
    })
}

// ── Python detection ──────────────────────────────────────────────────────────

fn detect_python(dir: &Path) -> Option<DetectedRuntime> {
    // Priority 1: .python-version
    if let Some(v) = read_plain_version_file(dir, ".python-version") {
        return Some(DetectedRuntime {
            version: v,
            source: ".python-version".to_string(),
            range_collapsed: false,
        });
    }

    // Priority 2: pyproject.toml [project].requires-python
    detect_python_from_pyproject(dir)
}

fn detect_python_from_pyproject(dir: &Path) -> Option<DetectedRuntime> {
    let raw = fs::read_to_string(dir.join("pyproject.toml")).ok()?;
    let doc: toml::Value = toml::from_str(&raw).ok()?;
    let requires = doc
        .get("project")
        .and_then(|p| p.get("requires-python"))
        .and_then(|v| v.as_str())?;

    let (version, was_range) = resolve_semver_range(requires);
    Some(DetectedRuntime {
        version,
        source: "pyproject.toml (requires-python)".to_string(),
        range_collapsed: was_range,
    })
}

// ── Run-command inference ─────────────────────────────────────────────────────

/// Return `Some("npm run dev")` if `package.json` has a `"dev"` script.
/// No other commands are inferred — this is the only well-defined heuristic
/// in v0.2 scope.
fn infer_dev_command(dir: &Path) -> Option<String> {
    let raw = fs::read_to_string(dir.join("package.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let has_dev = json
        .get("scripts")
        .and_then(|s| s.get("dev"))
        .is_some();
    if has_dev {
        Some("npm run dev".to_string())
    } else {
        None
    }
}

// ── Shared helpers ────────────────────────────────────────────────────────────

/// Read a file to string lossy, supporting UTF-16 LE with BOM, UTF-16 BE with BOM,
/// UTF-8 with BOM, and standard UTF-8.
fn read_file_to_string_lossy(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    if bytes.is_empty() {
        return Some(String::new());
    }

    if bytes.starts_with(&[0xFF, 0xFE]) {
        // UTF-16 LE
        let u16_chars: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        String::from_utf16(&u16_chars).ok()
    } else if bytes.starts_with(&[0xFE, 0xFF]) {
        // UTF-16 BE
        let u16_chars: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|chunk| u16::from_be_bytes([chunk[0], chunk[1]]))
            .collect();
        String::from_utf16(&u16_chars).ok()
    } else if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        // UTF-8 with BOM
        String::from_utf8(bytes[3..].to_vec()).ok()
    } else {
        // Standard UTF-8 (or fallback/invalid)
        String::from_utf8(bytes).ok()
    }
}

/// Read a plain-text version file, strip whitespace and a leading `v`.
/// Returns `None` if the file does not exist or is empty after stripping.
fn read_plain_version_file(dir: &Path, filename: &str) -> Option<String> {
    let raw = read_file_to_string_lossy(&dir.join(filename))?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(strip_leading_v(trimmed))
}

/// Strip a leading `v` or `V` from a version string.
fn strip_leading_v(s: &str) -> String {
    s.strip_prefix('v')
        .or_else(|| s.strip_prefix('V'))
        .unwrap_or(s)
        .to_string()
}

/// Resolve a semver range expression to a concrete version string.
///
/// **Resolution strategy (documented choice):** The *minimum version that
/// satisfies the constraint* is used.  Examples:
///
/// | Input | Output | Was range |
/// |-------|--------|-----------|
/// | `20.11.0` | `20.11.0` | false |
/// | `v20.11.0` | `20.11.0` | false |
/// | `^20` | `20.0.0` | true |
/// | `~20.11` | `20.11.0` | true |
/// | `>=3.11` | `3.11.0` | true |
/// | `>=3.11.7` | `3.11.7` | true |
/// | `==3.11.7` | `3.11.7` | false (exact) |
/// | `>20` | `20.0.0` | true |
/// | `=20.11.0` | `20.11.0` | false (exact) |
///
/// This is a deliberate simplification.  The caller is responsible for
/// printing a note so users know which version was chosen.
pub fn resolve_semver_range(range: &str) -> (String, bool) {
    let stripped = range.trim();

    // Handle common multi-char prefixes first (order matters)
    let prefixes: &[(&str, bool)] = &[
        (">=", true),
        ("<=", true),
        ("==", false),
        ("!=", true), // unusual, fall back to base version
        ("^", true),
        ("~", true),
        (">", true),
        ("<", true),
        ("=", false),
        ("v", false),
        ("V", false),
    ];

    for &(prefix, is_range) in prefixes {
        if let Some(rest) = stripped.strip_prefix(prefix) {
            let version = normalise_to_three_parts(rest.trim());
            return (version, is_range);
        }
    }

    // No prefix — already an exact version
    let version = normalise_to_three_parts(stripped);
    (version, false)
}

/// Ensure a version string has exactly three dot-separated numeric parts,
/// padding with `.0` as needed (e.g. `"20"` → `"20.0.0"`, `"3.11"` → `"3.11.0"`).
fn normalise_to_three_parts(v: &str) -> String {
    // Strip a leading `v` that might remain after prefix stripping
    let v = v
        .strip_prefix('v')
        .or_else(|| v.strip_prefix('V'))
        .unwrap_or(v);

    let parts: Vec<&str> = v.split('.').collect();
    match parts.len() {
        0 => "0.0.0".to_string(),
        1 => format!("{}.0.0", parts[0]),
        2 => format!("{}.{}.0", parts[0], parts[1]),
        _ => format!("{}.{}.{}", parts[0], parts[1], parts[2]),
    }
}

/// Build the `HashMap<String, String>` form expected by `RunxConfig::runtimes`
/// from a `DetectionResult`.
pub fn detected_runtimes_map(result: &DetectionResult) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    if let Some(node) = &result.node {
        map.insert("node".to_string(), node.version.clone());
    }
    if let Some(python) = &result.python {
        map.insert("python".to_string(), python.version.clone());
    }
    map
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ── Helper ───────────────────────────────────────────────────────────────

    fn tmp() -> TempDir {
        tempfile::tempdir().expect("create temp dir")
    }

    // ── Node.js detection ─────────────────────────────────────────────────────

    #[test]
    fn detects_node_from_nvmrc() {
        let dir = tmp();
        fs::write(dir.path().join(".nvmrc"), "v20.11.0\n").unwrap();

        let result = detect_runtimes(dir.path());
        let node = result.node.expect("should detect node");
        assert_eq!(node.version, "20.11.0");
        assert_eq!(node.source, ".nvmrc");
        assert!(!node.range_collapsed);
    }

    #[test]
    fn detects_node_from_node_version_file() {
        let dir = tmp();
        fs::write(dir.path().join(".node-version"), "18.20.3").unwrap();

        let result = detect_runtimes(dir.path());
        let node = result.node.expect("should detect node");
        assert_eq!(node.version, "18.20.3");
        assert_eq!(node.source, ".node-version");
        assert!(!node.range_collapsed);
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
        let node = result.node.expect("should detect node");
        assert_eq!(node.version, "20.11.0");
        assert_eq!(node.source, "package.json (engines.node)");
        assert!(node.range_collapsed, ">=20.11.0 is a range");
    }

    #[test]
    fn returns_none_when_no_node_files_present() {
        let dir = tmp();
        let result = detect_runtimes(dir.path());
        assert!(result.node.is_none());
    }

    #[test]
    fn nvmrc_wins_over_package_json_engines() {
        let dir = tmp();
        // .nvmrc has priority 1; package.json engines has priority 3
        fs::write(dir.path().join(".nvmrc"), "v20.11.0").unwrap();
        fs::write(
            dir.path().join("package.json"),
            r#"{"engines": {"node": "18.0.0"}}"#,
        )
        .unwrap();

        let result = detect_runtimes(dir.path());
        let node = result.node.expect("should detect node");
        assert_eq!(node.version, "20.11.0", ".nvmrc must win over package.json");
        assert_eq!(node.source, ".nvmrc");
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

        let result = detect_runtimes(dir.path());
        let node = result.node.expect("should detect node");
        assert_eq!(
            node.version, "18.20.3",
            ".node-version must win over package.json engines"
        );
    }

    #[test]
    fn nvmrc_wins_over_node_version_file() {
        let dir = tmp();
        fs::write(dir.path().join(".nvmrc"), "v20.11.0").unwrap();
        fs::write(dir.path().join(".node-version"), "18.20.3").unwrap();

        let result = detect_runtimes(dir.path());
        let node = result.node.expect("should detect node");
        assert_eq!(node.version, "20.11.0", ".nvmrc must win over .node-version");
        assert_eq!(node.source, ".nvmrc");
    }

    // ── Python detection ──────────────────────────────────────────────────────

    #[test]
    fn detects_python_from_python_version_file() {
        let dir = tmp();
        fs::write(dir.path().join(".python-version"), "3.11.7\n").unwrap();

        let result = detect_runtimes(dir.path());
        let python = result.python.expect("should detect python");
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
        let python = result.python.expect("should detect python");
        assert_eq!(python.version, "3.11.0");
        assert_eq!(python.source, "pyproject.toml (requires-python)");
        assert!(python.range_collapsed, ">=3.11 is a range");
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

        let result = detect_runtimes(dir.path());
        let python = result.python.expect("should detect python");
        assert_eq!(
            python.version, "3.12.0",
            ".python-version must win over pyproject.toml"
        );
        assert_eq!(python.source, ".python-version");
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

        let result = detect_runtimes(dir.path());
        assert_eq!(
            result.inferred_dev_command.as_deref(),
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

        let result = detect_runtimes(dir.path());
        assert!(
            result.inferred_dev_command.is_none(),
            "should not infer a command when no dev script exists"
        );
    }

    #[test]
    fn no_inferred_command_when_no_package_json() {
        let dir = tmp();
        let result = detect_runtimes(dir.path());
        assert!(result.inferred_dev_command.is_none());
    }

    // ── Range resolution ──────────────────────────────────────────────────────

    #[test]
    fn resolve_range_exact_version() {
        let (v, range) = resolve_semver_range("20.11.0");
        assert_eq!(v, "20.11.0");
        assert!(!range);
    }

    #[test]
    fn resolve_range_v_prefix() {
        let (v, range) = resolve_semver_range("v20.11.0");
        assert_eq!(v, "20.11.0");
        assert!(!range);
    }

    #[test]
    fn resolve_range_caret() {
        let (v, range) = resolve_semver_range("^20");
        assert_eq!(v, "20.0.0");
        assert!(range);
    }

    #[test]
    fn resolve_range_tilde_two_parts() {
        let (v, range) = resolve_semver_range("~20.11");
        assert_eq!(v, "20.11.0");
        assert!(range);
    }

    #[test]
    fn resolve_range_gte_two_parts() {
        let (v, range) = resolve_semver_range(">=3.11");
        assert_eq!(v, "3.11.0");
        assert!(range);
    }

    #[test]
    fn resolve_range_gte_three_parts() {
        let (v, range) = resolve_semver_range(">=20.11.0");
        assert_eq!(v, "20.11.0");
        assert!(range);
    }

    #[test]
    fn resolve_range_double_equals() {
        let (v, range) = resolve_semver_range("==3.11.7");
        assert_eq!(v, "3.11.7");
        assert!(!range, "== is exact, not a range");
    }

    #[test]
    fn reads_utf16_le_with_bom() {
        let dir = tmp();
        let path = dir.path().join(".nvmrc");
        // UTF-16 LE BOM (FF FE) followed by "20.11.0\n"
        let data: &[u8] = &[
            0xFF, 0xFE, // BOM
            b'2', 0x00, b'0', 0x00, b'.', 0x00, b'1', 0x00, b'1', 0x00, b'.', 0x00, b'0', 0x00, b'\n', 0x00,
        ];
        fs::write(&path, data).unwrap();
        let result = detect_runtimes(dir.path());
        let node = result.node.expect("should read UTF-16 LE file");
        assert_eq!(node.version, "20.11.0");
    }

    #[test]
    fn reads_utf16_be_with_bom() {
        let dir = tmp();
        let path = dir.path().join(".nvmrc");
        // UTF-16 BE BOM (FE FF) followed by "18.20.0"
        let data: &[u8] = &[
            0xFE, 0xFF, // BOM
            0x00, b'1', 0x00, b'8', 0x00, b'.', 0x00, b'2', 0x00, b'0', 0x00, b'.', 0x00, b'0',
        ];
        fs::write(&path, data).unwrap();
        let result = detect_runtimes(dir.path());
        let node = result.node.expect("should read UTF-16 BE file");
        assert_eq!(node.version, "18.20.0");
    }
}

