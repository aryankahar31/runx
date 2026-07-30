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
