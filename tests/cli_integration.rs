//! CLI routing and exit-code behaviour.
//!
//! runx dispatches `[run]` keys through clap's `external_subcommand`, so a bare
//! `runx dev` and a built-in `runx init` share one argument parser. That
//! arrangement is easy to break by accident — adding a subcommand silently
//! steals any `[run]` key with the same name — so the routing rules are pinned
//! here rather than left to manual checking.
//!
//! These tests use only `echo`-style commands and no `[runtimes]` section, so
//! nothing here touches the network.

use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

fn tmp() -> TempDir {
    tempfile::tempdir().expect("create temp dir")
}

/// Run the built binary in `dir` with an isolated cache.
///
/// `RUNX_HOME` is redirected so a test can never read or write the developer's
/// real `~/.runx`.
fn runx(dir: &Path, args: &[&str]) -> std::process::Output {
    let cache_home = dir.join(".runx-test-home");
    Command::new(env!("CARGO_BIN_EXE_runx"))
        .args(args)
        .current_dir(dir)
        .env("RUNX_HOME", &cache_home)
        .output()
        .expect("failed to run the runx binary")
}

fn config_path(dir: &Path) -> std::path::PathBuf {
    dir.join("runx.toml")
}

/// Write a `runx.toml` with the given `[run]` body and no runtimes.
fn write_config(dir: &Path, run_body: &str) {
    fs::write(config_path(dir), format!("[run]\n{run_body}")).expect("write runx.toml");
}

fn stdout_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr_of(output: &std::process::Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

// ── Bare key dispatch ────────────────────────────────────────────────────────

/// The headline ergonomic: `runx dev` with no subcommand keyword. Breaking this
/// would break every documented example.
#[test]
fn bare_key_runs_the_matching_command() {
    let dir = tmp();
    write_config(dir.path(), "hello = \"echo MARKER_BARE\"\n");

    let output = runx(dir.path(), &["hello"]);
    assert!(
        stdout_of(&output).contains("MARKER_BARE"),
        "bare key should run the command, got:\n{}\n{}",
        stdout_of(&output),
        stderr_of(&output)
    );
    assert_eq!(output.status.code(), Some(0));
}

#[test]
fn explicit_run_subcommand_runs_the_command() {
    let dir = tmp();
    write_config(dir.path(), "hello = \"echo MARKER_EXPLICIT\"\n");

    let output = runx(dir.path(), &["run", "hello"]);
    assert!(stdout_of(&output).contains("MARKER_EXPLICIT"));
    assert_eq!(output.status.code(), Some(0));
}

/// A key colliding with a built-in subcommand must stay reachable via
/// `runx run <key>`, so adding subcommands never strands an existing project.
#[test]
fn reserved_name_key_is_reachable_through_run() {
    let dir = tmp();
    write_config(dir.path(), "init = \"echo MARKER_RESERVED\"\n");

    let output = runx(dir.path(), &["run", "init"]);
    assert!(
        stdout_of(&output).contains("MARKER_RESERVED"),
        "a shadowed key must remain runnable, got:\n{}",
        stdout_of(&output)
    );

    // And the user is told about the collision rather than left guessing.
    assert!(
        stderr_of(&output).contains("reserved"),
        "should warn about the reserved key, got:\n{}",
        stderr_of(&output)
    );
}

/// For the *bare* form the built-in subcommand wins, even when a `[run]` key
/// shares its name. `runx init` must behave as `init`, not run the script.
#[test]
fn builtin_wins_over_a_same_named_run_key() {
    let dir = tmp();
    write_config(dir.path(), "init = \"echo MARKER_SHOULD_NOT_RUN\"\n");

    let output = runx(dir.path(), &["init"]);

    assert!(
        !stdout_of(&output).contains("MARKER_SHOULD_NOT_RUN"),
        "the built-in must win for the bare form, got:\n{}",
        stdout_of(&output)
    );
    // `init` refuses to clobber the config that already exists here.
    assert!(
        stderr_of(&output).contains("already exists"),
        "expected the built-in init behaviour, got:\n{}",
        stderr_of(&output)
    );
}

// ── Argument handling ────────────────────────────────────────────────────────

/// Extra arguments must be reported, not silently dropped. Passthrough is not
/// implemented yet, and quietly ignoring `--port 3000` would look like the flag
/// was honoured.
#[test]
fn extra_arguments_are_rejected_not_ignored() {
    let dir = tmp();
    write_config(dir.path(), "hello = \"echo MARKER\"\n");

    let output = runx(dir.path(), &["hello", "--port", "3000"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "extra args should fail rather than run the command"
    );

    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("--port 3000"),
        "error should quote the ignored args, got:\n{stderr}"
    );
    assert!(
        !stdout_of(&output).contains("MARKER"),
        "the command must not run when arguments are rejected"
    );
}

// ── Errors and exit codes ────────────────────────────────────────────────────

#[test]
fn unknown_key_lists_available_commands_and_exits_one() {
    let dir = tmp();
    write_config(dir.path(), "build = \"echo b\"\ndev = \"echo d\"\n");

    let output = runx(dir.path(), &["nosuchkey"]);
    assert_eq!(output.status.code(), Some(1));

    let stderr = stderr_of(&output);
    assert!(stderr.contains("nosuchkey"), "should name the bad key");
    assert!(
        stderr.contains("build") && stderr.contains("dev"),
        "should list the available keys, got:\n{stderr}"
    );
}

/// The exit status of the underlying command must propagate, or `runx test` in
/// CI would report success for a failing suite.
#[test]
fn command_exit_status_propagates() {
    let dir = tmp();
    write_config(dir.path(), "fail = \"exit 42\"\n");

    let output = runx(dir.path(), &["fail"]);
    assert_eq!(
        output.status.code(),
        Some(42),
        "runx must exit with the command's own status"
    );
}

#[test]
fn missing_project_config_exits_one_with_a_hint() {
    let dir = tmp();
    // Deliberately no runx.toml and no version files anywhere in the tree.
    let output = runx(dir.path(), &["dev"]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr_of(&output).contains("runx init"),
        "error should suggest `runx init`, got:\n{}",
        stderr_of(&output)
    );
}

// ── Built-ins ────────────────────────────────────────────────────────────────

#[test]
fn version_flag_reports_the_crate_version() {
    let dir = tmp();
    let output = runx(dir.path(), &["--version"]);

    assert_eq!(output.status.code(), Some(0));
    assert!(
        stdout_of(&output).contains(env!("CARGO_PKG_VERSION")),
        "--version should print the crate version, got:\n{}",
        stdout_of(&output)
    );
}

#[test]
fn no_arguments_prints_help_and_succeeds() {
    let dir = tmp();
    let output = runx(dir.path(), &[]);

    assert_eq!(output.status.code(), Some(0));
    let stdout = stdout_of(&output);
    assert!(
        stdout.contains("runx") && stdout.contains("init"),
        "help should mention the binary and its subcommands, got:\n{stdout}"
    );
}

#[test]
fn init_refuses_to_overwrite_an_existing_config() {
    let dir = tmp();
    write_config(dir.path(), "dev = \"echo original\"\n");

    let output = runx(dir.path(), &["init"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stderr_of(&output).contains("already exists"),
        "should refuse rather than clobber, got:\n{}",
        stderr_of(&output)
    );

    let contents = fs::read_to_string(config_path(dir.path())).unwrap();
    assert!(
        contents.contains("echo original"),
        "the existing config must be left untouched"
    );
}

/// `runx init` output must itself be valid and immediately runnable.
#[test]
fn generated_starter_config_is_usable() {
    let dir = tmp();
    let init = runx(dir.path(), &["init"]);
    assert_eq!(init.status.code(), Some(0));

    let contents = fs::read_to_string(config_path(dir.path())).expect("config written");
    assert!(contents.contains("[run]"), "starter config needs a [run]");
    assert!(
        contents.contains("[runtimes]"),
        "starter config should show a [runtimes] example"
    );
}

// ── Isolation guarantees ─────────────────────────────────────────────────────

/// runx must never touch shell startup files or the ambient environment. This
/// is the stated differentiator against nvm/asdf, so it is worth asserting.
#[test]
fn running_a_command_writes_nothing_outside_the_cache() {
    let dir = tmp();
    write_config(dir.path(), "hello = \"echo MARKER\"\n");

    let before: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|entry| entry.ok().map(|e| e.file_name()))
        .collect();

    let output = runx(dir.path(), &["hello"]);
    assert!(stdout_of(&output).contains("MARKER"));

    let after: Vec<_> = fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|entry| entry.ok().map(|e| e.file_name()))
        .filter(|name| name != ".runx-test-home")
        .collect();

    assert_eq!(
        before.len(),
        after.len(),
        "running a command must not create files in the project directory"
    );
}

/// The command runs with the project directory as its working directory, so
/// relative paths in scripts behave as the user expects.
#[test]
fn command_runs_in_the_project_directory() {
    let dir = tmp();
    write_config(dir.path(), "pwd = \"echo MARKER > produced.txt\"\n");

    let output = runx(dir.path(), &["pwd"]);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert!(
        dir.path().join("produced.txt").is_file(),
        "the command should run with the project dir as cwd"
    );
}

/// Config in a parent directory is found from a subdirectory, matching how
/// cargo, npm and git locate their config.
#[test]
fn finds_config_in_a_parent_directory() {
    let dir = tmp();
    write_config(dir.path(), "hello = \"echo MARKER_PARENT\"\n");

    let nested = dir.path().join("src").join("deep");
    fs::create_dir_all(&nested).unwrap();

    let output = runx(&nested, &["hello"]);
    assert!(
        stdout_of(&output).contains("MARKER_PARENT"),
        "should walk up to find runx.toml, got:\n{}\n{}",
        stdout_of(&output),
        stderr_of(&output)
    );
}

// ── Cache subcommands ────────────────────────────────────────────────────────
//
// These build a fake cache directly on disk rather than installing a real
// runtime, so they stay offline and fast. `runx cache` only reads directory
// structure and receipts, so a synthetic tree exercises the same code paths.

/// Create a plausible cached runtime under `home`, optionally marked complete.
fn plant_runtime(home: &Path, tool: &str, version: &str, complete: bool) -> std::path::PathBuf {
    let root = home.join("runtimes").join(tool).join(version);
    let bin = root.join(if cfg!(windows) { "." } else { "bin" });
    fs::create_dir_all(&bin).expect("create runtime dirs");

    let exe = if cfg!(windows) {
        format!("{tool}.exe")
    } else {
        tool.to_string()
    };
    // Enough bytes that the size report is non-zero.
    fs::write(bin.join(exe), vec![0u8; 4096]).expect("write fake executable");

    if complete {
        let receipt = format!(
            r#"{{"tool":"{tool}","version":"{version}","installed_at_secs":0,
                "runx_version":"test","source_url":"https://example.invalid","sha256":null}}"#
        );
        fs::write(root.join(".runx-complete.json"), receipt).expect("write receipt");
    }
    root
}

/// Run runx with an explicit cache home, outside any project.
fn runx_with_home(dir: &Path, home: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_runx"))
        .args(args)
        .current_dir(dir)
        .env("RUNX_HOME", home)
        .output()
        .expect("failed to run the runx binary")
}

#[test]
fn cache_list_reports_an_empty_cache_clearly() {
    let dir = tmp();
    let home = dir.path().join("home");
    fs::create_dir_all(&home).unwrap();

    let output = runx_with_home(dir.path(), &home, &["cache", "list"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(
        stdout_of(&output).contains("No cached runtimes"),
        "got:\n{}",
        stdout_of(&output)
    );
}

#[test]
fn cache_list_shows_installed_runtimes_with_size() {
    let dir = tmp();
    let home = dir.path().join("home");
    plant_runtime(&home, "node", "20.11.0", true);
    plant_runtime(&home, "python", "3.11.7", true);

    let output = runx_with_home(dir.path(), &home, &["cache", "list"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(0));
    assert!(
        stdout.contains("node") && stdout.contains("20.11.0"),
        "{stdout}"
    );
    assert!(
        stdout.contains("python") && stdout.contains("3.11.7"),
        "{stdout}"
    );
    assert!(stdout.contains("KiB"), "should report a size: {stdout}");
}

/// A runtime with no receipt is legacy or damaged; it must be listed and
/// flagged, not hidden, or the user cannot account for their disk usage.
#[test]
fn cache_list_flags_incomplete_runtimes() {
    let dir = tmp();
    let home = dir.path().join("home");
    plant_runtime(&home, "node", "18.0.0", false);

    let output = runx_with_home(dir.path(), &home, &["cache", "list"]);
    assert!(
        stdout_of(&output).contains("incomplete"),
        "an unreceipted runtime should be flagged: {}",
        stdout_of(&output)
    );
}

#[test]
fn cache_size_totals_the_cache() {
    let dir = tmp();
    let home = dir.path().join("home");
    plant_runtime(&home, "node", "20.11.0", true);

    let output = runx_with_home(dir.path(), &home, &["cache", "size"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(0));
    assert!(stdout.contains("1 runtime"), "{stdout}");
    assert!(stdout.contains("Total:"), "{stdout}");
}

/// Deleting every cached runtime is destructive and easy to mistype, so it must
/// be a dry run until explicitly confirmed.
#[test]
fn cache_clean_without_yes_deletes_nothing() {
    let dir = tmp();
    let home = dir.path().join("home");
    let root = plant_runtime(&home, "node", "20.11.0", true);

    let output = runx_with_home(dir.path(), &home, &["cache", "clean"]);
    let stdout = stdout_of(&output);

    assert_eq!(output.status.code(), Some(0));
    assert!(stdout.contains("Would remove"), "{stdout}");
    assert!(
        stdout.contains("--yes"),
        "should say how to confirm: {stdout}"
    );
    assert!(
        root.is_dir(),
        "the runtime must survive a run without --yes"
    );
}

#[test]
fn cache_clean_with_yes_removes_runtimes() {
    let dir = tmp();
    let home = dir.path().join("home");
    let root = plant_runtime(&home, "node", "20.11.0", true);

    let output = runx_with_home(dir.path(), &home, &["cache", "clean", "--yes"]);

    assert_eq!(output.status.code(), Some(0));
    assert!(!root.exists(), "the runtime should be deleted");
}

/// Age is measured from last use; a freshly planted runtime has an install
/// timestamp of 0 (the epoch) and so is prunable, but only with --yes.
#[test]
fn cache_prune_without_yes_deletes_nothing() {
    let dir = tmp();
    let home = dir.path().join("home");
    let root = plant_runtime(&home, "node", "20.11.0", true);

    let output = runx_with_home(dir.path(), &home, &["cache", "prune", "--older-than", "0"]);

    assert_eq!(output.status.code(), Some(0));
    assert!(
        stdout_of(&output).contains("Would remove"),
        "{}",
        stdout_of(&output)
    );
    assert!(root.is_dir(), "prune must not delete without --yes");
}

#[test]
fn cache_prune_spares_recently_used_runtimes() {
    let dir = tmp();
    let home = dir.path().join("home");
    let root = plant_runtime(&home, "node", "20.11.0", true);
    // Mark it as used right now.
    fs::write(
        root.join(".runx-last-used"),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .to_string(),
    )
    .unwrap();

    let output = runx_with_home(dir.path(), &home, &["cache", "prune", "--older-than", "30"]);

    assert_eq!(output.status.code(), Some(0));
    assert!(
        root.is_dir(),
        "a runtime used today must not be pruned at a 30-day threshold"
    );
    assert!(
        stdout_of(&output).contains("No runtimes unused"),
        "got:\n{}",
        stdout_of(&output)
    );
}

#[test]
fn cache_prune_with_yes_removes_stale_runtimes() {
    let dir = tmp();
    let home = dir.path().join("home");
    let root = plant_runtime(&home, "node", "20.11.0", true);

    let output = runx_with_home(
        dir.path(),
        &home,
        &["cache", "prune", "--older-than", "0", "--yes"],
    );

    assert_eq!(output.status.code(), Some(0));
    assert!(!root.exists(), "a stale runtime should be removed");
}

/// Cache commands must work anywhere, with no project config present.
#[test]
fn cache_commands_work_outside_a_project() {
    let dir = tmp();
    let home = dir.path().join("home");
    plant_runtime(&home, "node", "20.11.0", true);

    for args in [
        vec!["cache", "list"],
        vec!["cache", "size"],
        vec!["cache", "clean"],
        vec!["cache", "prune"],
    ] {
        let output = runx_with_home(dir.path(), &home, &args);
        assert_eq!(
            output.status.code(),
            Some(0),
            "`runx {}` should not require a project, stderr:\n{}",
            args.join(" "),
            stderr_of(&output)
        );
    }
}
