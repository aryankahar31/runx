//! Integration tests for zero-config auto-detection (runx v0.2).
//!
//! These tests exercise config resolution without downloading any runtimes.
//! Full end-to-end download tests are manual (see walkthrough.md).

use std::fs;
use tempfile::TempDir;

fn tmp() -> TempDir {
    tempfile::tempdir().expect("create temp dir")
}

// Re-export the config module so we can call load_or_detect.
// Integration tests live in `tests/` and access only `pub` items.
use runx::config;

/// A project with only a `package.json` (engines + dev script, no runx.toml)
/// should produce a synthesized config that matches the detected values.
#[test]
fn zero_config_from_package_json_only() {
    let dir = tmp();
    fs::write(
        dir.path().join("package.json"),
        r#"{
  "name": "my-app",
  "engines": { "node": "20.11.0" },
  "scripts": { "dev": "node index.js" }
}"#,
    )
    .unwrap();

    let resolved =
        config::load_or_detect(dir.path()).expect("should auto-detect from package.json");

    assert_eq!(
        resolved.inner.runtimes.get("node").map(String::as_str),
        Some("20.11.0"),
        "node version should be detected from package.json engines"
    );
    assert_eq!(
        resolved.inner.run.get("dev").map(String::as_str),
        Some("npm run dev"),
        "dev command should be inferred as 'npm run dev'"
    );
    assert!(
        !resolved.detection_lines.is_empty(),
        "transparency lines should be populated when auto-detecting"
    );
}

/// A project with a `.nvmrc` takes that version rather than `package.json`
/// engines (priority order check).
#[test]
fn nvmrc_beats_package_json_engines_in_zero_config() {
    let dir = tmp();
    fs::write(dir.path().join(".nvmrc"), "v18.20.3\n").unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"engines": {"node": "20.11.0"}, "scripts": {"dev": "node index.js"}}"#,
    )
    .unwrap();

    let resolved = config::load_or_detect(dir.path()).expect("should auto-detect from .nvmrc");

    assert_eq!(
        resolved.inner.runtimes["node"], "18.20.3",
        ".nvmrc must win over package.json engines"
    );
}

/// When a `runx.toml` exists it always wins — even if a `.nvmrc` with a
/// *different* version is also present.
#[test]
fn explicit_toml_always_wins_over_detection() {
    let dir = tmp();

    fs::write(
        dir.path().join("runx.toml"),
        "[runtimes]\nnode = \"16.20.0\"\n\n[run]\ndev = \"node --version\"\n",
    )
    .unwrap();
    fs::write(dir.path().join(".nvmrc"), "v20.11.0").unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"engines": {"node": "18.0.0"}, "scripts": {"dev": "node index.js"}}"#,
    )
    .unwrap();

    let resolved = config::load_or_detect(dir.path()).expect("should load runx.toml without error");

    assert_eq!(
        resolved.inner.runtimes["node"], "16.20.0",
        "runx.toml version must win over both .nvmrc and package.json"
    );
    assert!(
        resolved.detection_lines.is_empty(),
        "no detection lines should be emitted when runx.toml is used"
    );
}

/// A project with a semver range in `package.json` engines should resolve to
/// the minimum satisfying version and mark the range as collapsed.
#[test]
fn range_in_package_json_engines_is_resolved() {
    let dir = tmp();
    fs::write(
        dir.path().join("package.json"),
        r#"{"engines": {"node": "^20"}, "scripts": {"dev": "node index.js"}}"#,
    )
    .unwrap();

    let resolved =
        config::load_or_detect(dir.path()).expect("should auto-detect and resolve range");

    assert_eq!(
        resolved.inner.runtimes["node"], "20.0.0",
        "^20 should resolve to 20.0.0 (minimum satisfying version)"
    );
}

/// A Python project with only `.python-version` and no Node files should
/// detect the Python version and report an error about the missing dev command
/// (no package.json present).
#[test]
fn python_only_project_no_dev_command_errors_clearly() {
    let dir = tmp();
    fs::write(dir.path().join(".python-version"), "3.11.7\n").unwrap();

    let err = config::load_or_detect(dir.path())
        .expect_err("should fail because no dev command can be inferred");

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

/// End-to-end binary test: running `runx dev` against a zero-config directory
/// should print the auto-detection banner and trigger runtime installation/use.
#[test]
fn integration_runs_binary_and_prints_banner() {
    let binary = env!("CARGO_BIN_EXE_runx");
    let dir = tmp();

    // Create a package.json with dev script and engines.node
    fs::write(
        dir.path().join("package.json"),
        r#"{
  "engines": { "node": "20.11.0" },
  "scripts": { "dev": "node --version" }
}"#,
    )
    .unwrap();

    let output = std::process::Command::new(binary)
        .arg("dev")
        .current_dir(dir.path())
        .output()
        .expect("failed to run runx binary");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // It should print the transparency banner
    assert!(
        stdout.contains("No runx.toml found — detected from project files:"),
        "stdout should contain banner, got:\nSTDOUT:\n{stdout}\nSTDERR:\n{stderr}"
    );
    assert!(
        stdout.contains("node 20.11.0 (from package.json (engines.node))"),
        "stdout should contain detected runtime, got:\n{stdout}"
    );

    // It should either say "Installing node" or "Using cached node"
    assert!(
        stdout.contains("Installing node 20.11.0") || stdout.contains("Using cached node 20.11.0"),
        "stdout should contain installing/cached log, got:\n{stdout}"
    );
}
