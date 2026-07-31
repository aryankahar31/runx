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
//! **Bun**
//! 1. `package.json` → `engines.bun`
//! 2. `package.json` → `packageManager` (`"bun@1.1.0"`, corepack-style)
//!
//! **Go**
//! 1. `go.mod` → `go` directive (`go 1.22.0`)
//!
//! **Range resolution**: version hints are parsed with [`crate::version::Req`].
//! A hint that cannot be turned into a concrete version — because it has no
//! lower bound (`<20`), because it excludes its own bound (`!=3.11`), or
//! because it is not a version at all (`lts/iron`) — is reported as
//! [`Detected::Unresolvable`] rather than silently guessed. Guessing is how
//! `<20` used to resolve to `20.0.0`, a version the constraint forbids.

use crate::version::Req;
use std::{collections::BTreeMap, fs, path::Path};

// ── Public types ─────────────────────────────────────────────────────────────

/// A version requirement found in a project file.
///
/// Detection deliberately stops at the *requirement* and does not pick a
/// concrete version. Choosing one may need the upstream release list (to honour
/// "newest satisfying release"), and detection must stay a fast, offline,
/// read-only scan. Resolution happens later, in [`crate::registry`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedRuntime {
    /// The requirement text as written in the project file, e.g. `">=20"`.
    pub requirement: String,
    /// Human-readable source description (e.g. `".nvmrc"`).
    pub source: String,
    /// True when the requirement is a range rather than a single pinned version.
    pub is_range: bool,
}

/// The outcome of inspecting one runtime's version hints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detected {
    /// A usable version requirement was found.
    Found(DetectedRuntime),
    /// A hint was present but is not a version requirement at all.
    ///
    /// This is deliberately *not* an `Option::None`: the difference between
    /// "this project says nothing about Node" and "this project asks for a Node
    /// version I cannot make sense of" matters to the user, and collapsing the
    /// two produces a misleading "nothing detected" error.
    Unresolvable {
        source: String,
        requirement: String,
        reason: String,
    },
}

impl Detected {
    /// The requirement, if the hint parsed.
    pub fn found(&self) -> Option<&DetectedRuntime> {
        match self {
            Self::Found(runtime) => Some(runtime),
            Self::Unresolvable { .. } => None,
        }
    }
}

/// The result of scanning a project directory for runtime version hints.
#[derive(Debug, Default)]
pub struct DetectionResult {
    pub node: Option<Detected>,
    pub python: Option<Detected>,
    pub bun: Option<Detected>,
    pub go: Option<Detected>,
    /// Shell command inferred from `package.json` `scripts.dev`, if present.
    /// Currently only `"npm run dev"` is inferred — no other heuristics are
    /// attempted, per the v0.2 scope.
    pub inferred_dev_command: Option<String>,
}

impl DetectionResult {
    /// Every hint that was found but could not be resolved, as printable lines.
    pub fn unresolvable(&self) -> Vec<String> {
        [
            ("node", &self.node),
            ("python", &self.python),
            ("bun", &self.bun),
            ("go", &self.go),
        ]
        .into_iter()
        .filter_map(|(tool, slot)| match slot.as_ref()? {
            Detected::Unresolvable {
                source,
                requirement,
                reason,
            } => Some(format!(
                "  {tool} `{requirement}` (from {source}) — {reason}"
            )),
            Detected::Found(_) => None,
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
        bun: detect_bun(dir),
        go: detect_go(dir),
        inferred_dev_command: infer_dev_command(dir),
    }
}

// ── Node.js detection ─────────────────────────────────────────────────────────

fn detect_node(dir: &Path) -> Option<Detected> {
    // Priority 1 and 2: plain-text version files.
    for filename in [".nvmrc", ".node-version"] {
        if let Some(raw) = read_plain_version_file(dir, filename) {
            return Some(record_hint(&raw, filename));
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

    Some(record_hint(node_range, "package.json (engines.node)"))
}

// ── Python detection ──────────────────────────────────────────────────────────

fn detect_python(dir: &Path) -> Option<Detected> {
    // Priority 1: .python-version
    if let Some(raw) = read_plain_version_file(dir, ".python-version") {
        return Some(record_hint(&raw, ".python-version"));
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

    Some(record_hint(requires, "pyproject.toml (requires-python)"))
}

// ── Bun detection ─────────────────────────────────────────────────────────────

fn detect_bun(dir: &Path) -> Option<Detected> {
    let raw = read_file_to_string_lossy(&dir.join("package.json"))?;
    let json: serde_json::Value = serde_json::from_str(&raw).ok()?;

    // Priority 1: engines.bun, mirroring engines.node.
    if let Some(range) = json
        .get("engines")
        .and_then(|engines| engines.get("bun"))
        .and_then(|value| value.as_str())
    {
        return Some(record_hint(range, "package.json (engines.bun)"));
    }

    // Priority 2: the corepack-style packageManager field ("bun@1.1.0").
    let manager = json.get("packageManager").and_then(|value| value.as_str())?;
    let requirement = manager
        .strip_prefix("bun@")?
        // Corepack appends a digest as `bun@1.1.0+sha512.…`; the digest is not
        // part of the version.
        .split('+')
        .next()?;

    Some(record_hint(requirement, "package.json (packageManager)"))
}

// ── Go detection ──────────────────────────────────────────────────────────────

/// Detect the Go version from the `go` directive in `go.mod`.
///
/// Only the first `go` directive is read (there is exactly one in a valid
/// module); the `toolchain go1.22.5` directive is deliberately ignored.
fn detect_go(dir: &Path) -> Option<Detected> {
    let raw = read_file_to_string_lossy(&dir.join("go.mod"))?;

    for line in raw.lines() {
        let mut tokens = line.trim().split_whitespace();
        if tokens.next() != Some("go") {
            continue;
        }
        // A trailing comment after the version is harmless: the requirement
        // parser only sees the version token.
        let Some(version) = tokens.next() else {
            continue;
        };
        return Some(record_hint(version, "go.mod"));
    }

    None
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

/// Record one raw version hint, without choosing a concrete version.
///
/// Only hints that are not version requirements at all (`lts/iron`, a path, a
/// shell fragment) are rejected here. A range such as `<20` is kept: it is
/// perfectly resolvable against the published release list, and deciding that
/// requires the network, which detection must not touch.
fn record_hint(raw: &str, source: &str) -> Detected {
    let requirement = normalise_requirement(raw);

    match Req::parse(&requirement) {
        Some(req) => Detected::Found(DetectedRuntime {
            requirement,
            source: source.to_string(),
            is_range: !req.exact,
        }),
        None => Detected::Unresolvable {
            source: source.to_string(),
            requirement,
            reason: "not a recognised version or range (aliases like `lts/*` are not supported)"
                .to_string(),
        },
    }
}

/// Strip the cosmetic `v` prefix that `.nvmrc` conventionally carries.
///
/// Only applied when a digit follows, so `v20.11.0` is normalised while `>=20`
/// and `^20.11` are left alone. Keeping requirement text canonical matters
/// because it is echoed in banners, synthesised configs and `runx.lock`, and is
/// compared against the lockfile to detect a changed requirement.
fn normalise_requirement(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Some(rest) = trimmed.strip_prefix(['v', 'V']) {
        if rest.starts_with(|c: char| c.is_ascii_digit()) {
            return rest.to_string();
        }
    }
    trimmed.to_string()
}

/// Build the map form expected by `RunxConfig::runtimes`, keyed tool to
/// *requirement*, including only hints that parsed.
pub fn detected_runtimes_map(result: &DetectionResult) -> BTreeMap<String, String> {
    [
        ("node", &result.node),
        ("python", &result.python),
        ("bun", &result.bun),
        ("go", &result.go),
    ]
    .into_iter()
    .filter_map(|(tool, slot)| {
        let found = slot.as_ref()?.found()?;
        Some((tool.to_string(), found.requirement.clone()))
    })
    .collect()
}

// ── Unit tests ────────────────────────────────────────────────────────────────

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn tmp() -> TempDir {
        tempfile::tempdir().expect("create temp dir")
    }

    /// The detected requirement for `node`, panicking with context if absent.
    fn node_of(result: &DetectionResult) -> &DetectedRuntime {
        result
            .node
            .as_ref()
            .expect("node should be detected")
            .found()
            .expect("node hint should parse")
    }

    fn python_of(result: &DetectionResult) -> &DetectedRuntime {
        result
            .python
            .as_ref()
            .expect("python should be detected")
            .found()
            .expect("python hint should parse")
    }

    // ── Node.js detection ─────────────────────────────────────────────────────

    #[test]
    fn detects_node_from_nvmrc() {
        let dir = tmp();
        fs::write(dir.path().join(".nvmrc"), "v20.11.0\n").unwrap();

        let result = detect_runtimes(dir.path());
        let node = node_of(&result);
        assert_eq!(node.requirement, "20.11.0", "the `v` prefix is cosmetic");
        assert_eq!(node.source, ".nvmrc");
        assert!(!node.is_range, "an exact pin is not a range");
    }

    #[test]
    fn detects_node_from_node_version_file() {
        let dir = tmp();
        fs::write(dir.path().join(".node-version"), "18.20.3").unwrap();

        let result = detect_runtimes(dir.path());
        assert_eq!(node_of(&result).requirement, "18.20.3");
        assert_eq!(node_of(&result).source, ".node-version");
    }

    /// Ranges are preserved verbatim rather than collapsed at detection time,
    /// so the resolver can pick the newest satisfying release later.
    #[test]
    fn detects_node_range_from_package_json_engines() {
        let dir = tmp();
        fs::write(
            dir.path().join("package.json"),
            r#"{"engines": {"node": ">=20.11.0"}}"#,
        )
        .unwrap();

        let result = detect_runtimes(dir.path());
        let node = node_of(&result);
        assert_eq!(
            node.requirement, ">=20.11.0",
            "the requirement must survive detection intact"
        );
        assert_eq!(node.source, "package.json (engines.node)");
        assert!(node.is_range);
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
        assert_eq!(node_of(&result).requirement, "20.11.0");
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

        assert_eq!(node_of(&detect_runtimes(dir.path())).requirement, "18.20.3");
    }

    #[test]
    fn nvmrc_wins_over_node_version_file() {
        let dir = tmp();
        fs::write(dir.path().join(".nvmrc"), "v20.11.0").unwrap();
        fs::write(dir.path().join(".node-version"), "18.20.3").unwrap();

        let result = detect_runtimes(dir.path());
        assert_eq!(node_of(&result).requirement, "20.11.0");
        assert_eq!(node_of(&result).source, ".nvmrc");
    }

    // ── Python detection ──────────────────────────────────────────────────────

    #[test]
    fn detects_python_from_python_version_file() {
        let dir = tmp();
        fs::write(dir.path().join(".python-version"), "3.11.7\n").unwrap();

        let result = detect_runtimes(dir.path());
        assert_eq!(python_of(&result).requirement, "3.11.7");
        assert_eq!(python_of(&result).source, ".python-version");
        assert!(!python_of(&result).is_range);
    }

    #[test]
    fn detects_python_requirement_from_pyproject() {
        let dir = tmp();
        fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nrequires-python = \">=3.11\"\n",
        )
        .unwrap();

        let result = detect_runtimes(dir.path());
        assert_eq!(python_of(&result).requirement, ">=3.11");
        assert!(python_of(&result).is_range);
    }

    /// PEP 440 compatible-release syntax appears widely in pyproject.toml.
    #[test]
    fn detects_pep440_compatible_release() {
        let dir = tmp();
        fs::write(
            dir.path().join("pyproject.toml"),
            "[project]\nrequires-python = \"~=3.11\"\n",
        )
        .unwrap();

        let result = detect_runtimes(dir.path());
        assert_eq!(python_of(&result).requirement, "~=3.11");
        assert!(python_of(&result).is_range);
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

        assert_eq!(
            python_of(&detect_runtimes(dir.path())).requirement,
            "3.12.0"
        );
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

    // ── Malicious and malformed hints ─────────────────────────────────────────

    /// A `.nvmrc` is attacker-controlled in any cloned repo. Its contents must
    /// never become a usable requirement, since a version reaches both a cache
    /// path that gets recursively deleted and a download URL.
    #[test]
    fn path_traversal_in_nvmrc_is_unresolvable() {
        let dir = tmp();
        fs::write(dir.path().join(".nvmrc"), "../../../../tmp/pwned").unwrap();

        let detected = detect_runtimes(dir.path()).node.expect("hint is present");
        assert!(
            detected.found().is_none(),
            "a traversal payload must not parse as a requirement"
        );
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
    fn shell_metacharacters_are_unresolvable() {
        for payload in ["20.11.0 && rm -rf /", "$(whoami)", "20.11.0; echo hi"] {
            let dir = tmp();
            fs::write(dir.path().join(".nvmrc"), payload).unwrap();

            let detected = detect_runtimes(dir.path()).node.expect("hint present");
            assert!(
                detected.found().is_none(),
                "{payload:?} must not parse as a requirement"
            );
        }
    }

    #[test]
    fn alias_in_nvmrc_is_reported_not_guessed() {
        let dir = tmp();
        fs::write(dir.path().join(".nvmrc"), "lts/iron").unwrap();

        match detect_runtimes(dir.path()).node.expect("hint is present") {
            Detected::Unresolvable {
                requirement,
                reason,
                source,
            } => {
                assert_eq!(requirement, "lts/iron");
                assert_eq!(source, ".nvmrc");
                assert!(reason.contains("lts"), "reason should mention aliases");
            }
            Detected::Found(runtime) => {
                panic!("lts/iron must not parse, got {}", runtime.requirement)
            }
        }
    }

    /// Behaviour change: `<20` is no longer rejected at detection time. It is a
    /// valid constraint, resolvable against the published release list, so it is
    /// recorded and resolved later rather than guessed at or discarded.
    #[test]
    fn upper_bound_only_range_is_recorded_for_later_resolution() {
        let dir = tmp();
        fs::write(
            dir.path().join("package.json"),
            r#"{"engines": {"node": "<20"}}"#,
        )
        .unwrap();

        let result = detect_runtimes(dir.path());
        assert_eq!(node_of(&result).requirement, "<20");
        assert!(node_of(&result).is_range);
    }

    /// Alternation used to produce the literal directory name
    /// `"18 || >=20.0.0"`; it is now kept intact as a requirement.
    #[test]
    fn alternation_is_preserved_verbatim() {
        let dir = tmp();
        fs::write(
            dir.path().join("package.json"),
            r#"{"engines": {"node": "18 || >=20"}}"#,
        )
        .unwrap();

        let result = detect_runtimes(dir.path());
        assert_eq!(node_of(&result).requirement, "18 || >=20");
        assert!(node_of(&result).is_range);
    }

    #[test]
    fn comments_and_blank_lines_in_version_files_are_skipped() {
        let dir = tmp();
        fs::write(dir.path().join(".nvmrc"), "\n# pinned for CI\n20.11.0\n").unwrap();

        assert_eq!(node_of(&detect_runtimes(dir.path())).requirement, "20.11.0");
    }

    #[test]
    fn empty_version_file_is_ignored() {
        let dir = tmp();
        fs::write(dir.path().join(".nvmrc"), "   \n\n").unwrap();
        assert!(detect_runtimes(dir.path()).node.is_none());
    }

    // ── Requirement normalisation ─────────────────────────────────────────────

    #[test]
    fn strips_only_a_cosmetic_v_prefix() {
        assert_eq!(normalise_requirement("v20.11.0"), "20.11.0");
        assert_eq!(normalise_requirement("V20.11.0"), "20.11.0");
        assert_eq!(normalise_requirement("  v20.11.0  "), "20.11.0");
        // Operators and non-version text must be left alone.
        assert_eq!(normalise_requirement(">=20"), ">=20");
        assert_eq!(normalise_requirement("^20.11"), "^20.11");
        assert_eq!(normalise_requirement("vnext"), "vnext");
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

        assert_eq!(node_of(&detect_runtimes(dir.path())).requirement, "20.11.0");
    }

    #[test]
    fn reads_utf16_be_with_bom() {
        let dir = tmp();
        let data: &[u8] = &[
            0xFE, 0xFF, // BOM
            0x00, b'1', 0x00, b'8', 0x00, b'.', 0x00, b'2', 0x00, b'0', 0x00, b'.', 0x00, b'0',
        ];
        fs::write(dir.path().join(".nvmrc"), data).unwrap();

        assert_eq!(node_of(&detect_runtimes(dir.path())).requirement, "18.20.0");
    }

    #[test]
    fn reads_utf8_with_bom() {
        let dir = tmp();
        let mut data = vec![0xEF, 0xBB, 0xBF];
        data.extend_from_slice(b"20.11.0\n");
        fs::write(dir.path().join(".nvmrc"), data).unwrap();

        assert_eq!(node_of(&detect_runtimes(dir.path())).requirement, "20.11.0");
    }

    /// A truncated UTF-16 file must not panic.
    #[test]
    fn odd_length_utf16_does_not_panic() {
        let dir = tmp();
        let data: &[u8] = &[0xFF, 0xFE, b'2', 0x00, b'0', 0x00, b'.'];
        fs::write(dir.path().join(".nvmrc"), data).unwrap();

        let _ = detect_runtimes(dir.path());
    }

    /// One unpaired surrogate must not discard the whole file.
    #[test]
    fn unpaired_surrogate_still_yields_a_hint() {
        let dir = tmp();
        let mut data = vec![0xFF, 0xFE];
        for byte in b"20.11.0" {
            data.push(*byte);
            data.push(0x00);
        }
        data.extend_from_slice(&[0x00, 0xD8]); // lone high surrogate
        fs::write(dir.path().join(".nvmrc"), data).unwrap();

        assert!(
            detect_runtimes(dir.path()).node.is_some(),
            "a lone surrogate must not discard the file"
        );
    }

    /// PowerShell 5.1 writes UTF-16 by default, so a `package.json` produced by
    /// `>` redirection must still parse.
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
        assert_eq!(node_of(&result).requirement, "20.11.0");
        assert_eq!(result.inferred_dev_command.as_deref(), Some("npm run dev"));
    }

    // ── Bun detection ────────────────────────────────────────────────────────

    #[test]
    fn detects_bun_from_package_json_engines() {
        let dir = tmp();
        fs::write(
            dir.path().join("package.json"),
            r#"{"engines": {"bun": ">=1.1.0"}}"#,
        )
        .unwrap();

        let result = detect_runtimes(dir.path());
        let bun = result
            .bun
            .as_ref()
            .expect("bun should be detected")
            .found()
            .expect("bun hint should parse");
        assert_eq!(bun.requirement, ">=1.1.0");
        assert_eq!(bun.source, "package.json (engines.bun)");
        assert!(bun.is_range);
    }

    /// Corepack's `packageManager` field is `bun@1.1.0` or, pinned,
    /// `bun@1.1.0+sha512.<digest>` — the digest is not part of the version.
    #[test]
    fn detects_bun_from_package_manager_field() {
        let dir = tmp();
        fs::write(
            dir.path().join("package.json"),
            r#"{"packageManager": "bun@1.1.0+sha512.a1b2c3"}"#,
        )
        .unwrap();

        let result = detect_runtimes(dir.path());
        let bun = result
            .bun
            .as_ref()
            .expect("bun should be detected")
            .found()
            .expect("bun hint should parse");
        assert_eq!(bun.requirement, "1.1.0");
        assert_eq!(bun.source, "package.json (packageManager)");
        assert!(!bun.is_range);
    }

    #[test]
    fn engines_bun_wins_over_package_manager() {
        let dir = tmp();
        fs::write(
            dir.path().join("package.json"),
            r#"{"engines": {"bun": "1.2.0"}, "packageManager": "bun@1.3.14"}"#,
        )
        .unwrap();

        let result = detect_runtimes(dir.path());
        let bun = result
            .bun
            .as_ref()
            .expect("bun should be detected")
            .found()
            .expect("bun hint should parse");
        assert_eq!(bun.requirement, "1.2.0");
        assert_eq!(bun.source, "package.json (engines.bun)");
    }

    /// A `packageManager` for another tool must not be read as a Bun hint.
    #[test]
    fn non_bun_package_managers_are_ignored() {
        let dir = tmp();
        fs::write(
            dir.path().join("package.json"),
            r#"{"packageManager": "pnpm@9.0.0"}"#,
        )
        .unwrap();

        assert!(detect_runtimes(dir.path()).bun.is_none());
    }

    #[test]
    fn returns_none_when_no_bun_hint_present() {
        let dir = tmp();
        fs::write(
            dir.path().join("package.json"),
            r#"{"engines": {"node": "20.11.0"}}"#,
        )
        .unwrap();

        assert!(detect_runtimes(dir.path()).bun.is_none());
    }

    // ── Go detection ─────────────────────────────────────────────────────────

    #[test]
    fn detects_go_from_gomod_directive() {
        let dir = tmp();
        fs::write(dir.path().join("go.mod"), "module example.com/hello\n\ngo 1.22.5\n")
            .unwrap();

        let result = detect_runtimes(dir.path());
        let go = result
            .go
            .as_ref()
            .expect("go should be detected")
            .found()
            .expect("go hint should parse");
        assert_eq!(go.requirement, "1.22.5");
        assert_eq!(go.source, "go.mod");
        assert!(!go.is_range);
    }

    /// A two-part directive (`go 1.22`) is an X-range, so it records the hint
    /// and resolves to a concrete release later.
    #[test]
    fn two_part_gomod_directive_is_a_range() {
        let dir = tmp();
        fs::write(dir.path().join("go.mod"), "module m\n\ngo 1.22\n").unwrap();

        let result = detect_runtimes(dir.path());
        let go = result
            .go
            .as_ref()
            .expect("go should be detected")
            .found()
            .expect("go hint should parse");
        assert_eq!(go.requirement, "1.22");
        assert!(go.is_range);
    }

    /// The `toolchain` directive must not be mistaken for the `go` directive.
    #[test]
    fn toolchain_directive_is_ignored() {
        let dir = tmp();
        fs::write(
            dir.path().join("go.mod"),
            "module m\n\ngo 1.22.0\ntoolchain go1.23.4\n",
        )
        .unwrap();

        let result = detect_runtimes(dir.path());
        let go = result
            .go
            .as_ref()
            .expect("go should be detected")
            .found()
            .expect("go hint should parse");
        assert_eq!(go.requirement, "1.22.0");
        assert_eq!(go.source, "go.mod");
    }

    #[test]
    fn returns_none_when_no_gomod() {
        let dir = tmp();
        assert!(detect_runtimes(dir.path()).go.is_none());
    }

    // ── Map building ──────────────────────────────────────────────────────────

    #[test]
    fn runtimes_map_carries_requirements_for_both_tools() {
        let dir = tmp();
        fs::write(dir.path().join(".nvmrc"), ">=20").unwrap();
        fs::write(dir.path().join(".python-version"), "3.11.7").unwrap();

        let map = detected_runtimes_map(&detect_runtimes(dir.path()));
        assert_eq!(map.get("node").map(String::as_str), Some(">=20"));
        assert_eq!(map.get("python").map(String::as_str), Some("3.11.7"));
    }

    /// Bun and Go requirements reach the runtimes map like any other tool.
    #[test]
    fn runtimes_map_carries_bun_and_go() {
        let dir = tmp();
        fs::write(
            dir.path().join("package.json"),
            r#"{"packageManager": "bun@1.3.14"}"#,
        )
        .unwrap();
        fs::write(dir.path().join("go.mod"), "module m\n\ngo 1.22.5\n").unwrap();

        let map = detected_runtimes_map(&detect_runtimes(dir.path()));
        assert_eq!(map.get("bun").map(String::as_str), Some("1.3.14"));
        assert_eq!(map.get("go").map(String::as_str), Some("1.22.5"));
    }

    /// An unusable Bun hint is reported with its tool and source, not silently
    /// dropped.
    #[test]
    fn unresolvable_bun_hints_name_the_tool_and_source() {
        let dir = tmp();
        fs::write(
            dir.path().join("package.json"),
            r#"{"packageManager": "bun@canary"}"#,
        )
        .unwrap();

        let lines = detect_runtimes(dir.path()).unresolvable();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("bun"), "should name the tool");
        assert!(lines[0].contains("packageManager"), "should name the source");
    }

    #[test]
    fn unresolvable_lines_name_the_tool_and_source() {
        let dir = tmp();
        fs::write(dir.path().join(".nvmrc"), "lts/iron").unwrap();

        let lines = detect_runtimes(dir.path()).unresolvable();
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("node"), "should name the tool");
        assert!(lines[0].contains(".nvmrc"), "should name the source file");
        assert!(
            lines[0].contains("lts/iron"),
            "should quote the requirement"
        );
    }
}
