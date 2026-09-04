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

/// Everything after `--` is passed through to the run command verbatim.
#[test]
fn passthrough_args_after_double_dash_reach_the_command() {
    let dir = tmp();
    write_config(dir.path(), "hello = \"echo MARKER\"\n");

    let output = runx(dir.path(), &["hello", "--", "--port", "3000"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    let stdout = stdout_of(&output);
    assert!(stdout.contains("MARKER"), "{stdout}");
    for fragment in ["--port", "3000"] {
        assert!(
            stdout.contains(fragment),
            "passthrough args must reach the command, missing {fragment:?}, got:\n{stdout}"
        );
    }
}

/// Passthrough appends to arguments the run command already carries, rather
/// than replacing them.
#[test]
fn passthrough_appends_to_arguments_already_in_the_command() {
    let dir = tmp();
    write_config(dir.path(), "dev = \"echo pre\"\n");

    let output = runx(dir.path(), &["dev", "--", "--port", "3000", "-o", "out"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    let stdout = stdout_of(&output);
    // Per-token containment: `cmd`'s echo prints the double-quoted form.
    for fragment in ["pre", "--port", "3000", "-o", "out"] {
        assert!(
            stdout.contains(fragment),
            "missing {fragment:?}, got:\n{stdout}"
        );
    }
}

/// Arguments with spaces, quotes and other shell specials survive the trip
/// intact. The assert uses containment because `cmd` on Windows echoes the
/// double-quoted form, but the characters themselves must be unmodified.
#[test]
fn passthrough_preserves_spaces_and_quotes() {
    let dir = tmp();
    write_config(dir.path(), "dev = \"echo MARKER\"\n");

    let output = runx(
        dir.path(),
        &["dev", "--", "a b", "it's", "$HOME", "*not-a-glob*"],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    let stdout = stdout_of(&output);
    for fragment in ["MARKER", "a b", "it's", "$HOME", "*not-a-glob*"] {
        assert!(
            stdout.contains(fragment),
            "missing {fragment:?}, got:\n{stdout}"
        );
    }
}

/// The explicit `runx run <key>` form passes arguments through identically.
#[test]
fn passthrough_works_with_explicit_run_subcommand() {
    let dir = tmp();
    write_config(dir.path(), "hello = \"echo MARKER\"\n");

    let output = runx(dir.path(), &["run", "hello", "--", "--port", "3000"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    let stdout = stdout_of(&output);
    assert!(stdout.contains("MARKER"), "{stdout}");
    for fragment in ["--port", "3000"] {
        assert!(
            stdout.contains(fragment),
            "missing {fragment:?}, got:\n{stdout}"
        );
    }
}

/// `npm run <script>` eats its own flag-style args (`--port` is a real npm
/// config key), so runx inserts npm's `--` separator before passthrough args.
/// A fake `npm` planted next to the fake node prints its argv: the separator
/// must sit between the script name and the forwarded args.
#[cfg(unix)]
#[test]
fn npm_run_gets_the_double_dash_separator_before_passthrough_args() {
    let dir = tmp();
    let home = dir.path().join("home");
    plant_reporting_node(&home);
    let spec = runx::runtime::resolve_runtime("node", "0.0.0").expect("offline spec");
    let npm = home
        .join("runtimes")
        .join("node")
        .join("0.0.0")
        .join(&spec.bin_dirs[0])
        .join("npm");
    fs::write(&npm, "#!/bin/sh\necho \"NPM_FAKE|$@\"\n").expect("write fake npm");
    fs::set_permissions(&npm, std::os::unix::fs::PermissionsExt::from_mode(0o755))
        .expect("make fake npm executable");
    fs::write(
        config_path(dir.path()),
        "[runtimes]\nnode = \"0.0.0\"\n\n[run]\ndev = \"npm run dev\"\n",
    )
    .expect("write runx.toml");

    let output = runx_with_home(
        dir.path(),
        &home,
        &["dev", "--", "--port", "4999", "-o", "out"],
    );
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    let stdout = stdout_of(&output);
    assert!(
        stdout.contains("NPM_FAKE|run dev -- --port 4999 -o out"),
        "npm must receive the `--` separator before passthrough args:\n{stdout}"
    );
}

/// Without `--`, extra arguments are still rejected rather than silently
/// dropped, with a hint pointing at the passthrough syntax.
#[test]
fn extra_arguments_without_double_dash_are_rejected_with_a_hint() {
    let dir = tmp();
    write_config(dir.path(), "hello = \"echo MARKER\"\n");

    let output = runx(dir.path(), &["hello", "--port", "3000"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "bare extra args should fail rather than run the command"
    );

    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("--port 3000"),
        "error should quote the rejected args, got:\n{stderr}"
    );
    assert!(
        stderr.contains("--"),
        "error should hint at the `--` passthrough syntax, got:\n{stderr}"
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

// ── Completions ──────────────────────────────────────────────────────────────

/// Completion scripts are generated offline for every shell clap knows; the
/// script must reference the binary name so shells can find the right command.
#[test]
fn completions_generate_a_script_for_known_shells() {
    let dir = tmp();
    for shell in ["bash", "zsh", "fish", "powershell"] {
        let output = runx(dir.path(), &["completions", shell]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "completions {shell} should succeed"
        );
        assert!(
            stdout_of(&output).contains("runx"),
            "completions {shell} should reference runx, got:\n{}",
            stdout_of(&output)
        );
    }

    let unknown = runx(dir.path(), &["completions", "csh"]);
    assert_ne!(
        unknown.status.code(),
        Some(0),
        "unknown shells are rejected"
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

// ── Multi-runtime projects ────────────────────────────────────────────────────
//
// These plant *executable* fake runtimes (shell scripts) at the exact cache
// layout `resolve_runtime` produces, so provisioning finds them cached and the
// child process actually executes them. Everything stays offline: node and
// bun specs are built purely from strings, and the fake versions (`0.0.0`)
// are never resolved against a release index.

/// Plant an executable fake runtime for `tool` at `home`, returning its root.
///
/// The body is a shell fragment run when the fake executable is invoked.
#[cfg(unix)]
fn plant_executable(home: &Path, tool: &str, version: &str, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let spec = runx::runtime::resolve_runtime(tool, version).expect("spec is offline");
    let root = home.join("runtimes").join(tool).join(version);
    // A "." bin dir (bun, deno) is the root itself; join() would keep the
    // trailing dot, which create_dir_all rejects on macOS.
    let bin = if spec.bin_dirs[0] == std::path::Path::new(".") {
        root.clone()
    } else {
        root.join(&spec.bin_dirs[0])
    };
    fs::create_dir_all(&bin).expect("create bin dir");

    let exe = bin.join(&spec.executable);
    fs::write(&exe, format!("#!/bin/sh\n{body}\n")).expect("write fake executable");
    fs::set_permissions(&exe, fs::Permissions::from_mode(0o755)).expect("make executable");

    let receipt = format!(
        r#"{{"tool":"{tool}","version":"{version}","installed_at_secs":0,
            "runx_version":"test","source_url":"https://example.invalid","sha256":null}}"#
    );
    fs::write(root.join(".runx-complete.json"), receipt).expect("write receipt");
    root
}

/// The `npm run dev` failure mode, generalised: the run command invokes one
/// runtime (node, standing in for npm) while a second runtime (bun) is only
/// reached through the child's PATH. Both must be found.
#[cfg(unix)]
#[test]
fn multi_runtime_command_reaches_every_runtime_on_the_child_path() {
    let dir = tmp();
    let home = dir.path().join("home");
    plant_executable(&home, "node", "0.0.0", "echo FAKE_NODE");
    plant_executable(&home, "bun", "0.0.0", "echo FAKE_BUN");
    fs::write(
        config_path(dir.path()),
        "[runtimes]\nnode = \"0.0.0\"\nbun = \"0.0.0\"\n\n\
         [run]\ndev = \"node --version && bun --version\"\n",
    )
    .expect("write runx.toml");

    let output = runx_with_home(dir.path(), &home, &["dev"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    let stdout = stdout_of(&output);
    assert!(
        stdout.contains("FAKE_NODE"),
        "node must be on the child PATH:\n{stdout}"
    );
    assert!(
        stdout.contains("FAKE_BUN"),
        "bun must be on the child PATH even though the command only mentions node:\n{stdout}"
    );
}

/// The combined PATH is prepended to, never replacing, the user's system
/// PATH: system tools like git and make must keep working inside the child.
#[cfg(unix)]
#[test]
fn child_path_prepends_runtime_bins_and_keeps_the_system_path() {
    let dir = tmp();
    let home = dir.path().join("home");
    let node_root = plant_executable(&home, "node", "0.0.0", "echo x");
    let bun_root = plant_executable(&home, "bun", "0.0.0", "echo x");
    fs::write(
        config_path(dir.path()),
        "[runtimes]\nnode = \"0.0.0\"\nbun = \"0.0.0\"\n\n\
         [run]\ndev = \"echo \\\"$PATH\\\"\"\n",
    )
    .expect("write runx.toml");

    let output = runx_with_home(dir.path(), &home, &["dev"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    let stdout = stdout_of(&output);

    let spec = runx::runtime::resolve_runtime("node", "0.0.0").expect("offline spec");
    let node_bin = node_root.join(&spec.bin_dirs[0]);
    assert!(
        stdout.contains(&node_bin.display().to_string()),
        "node bin dir must be on the child PATH:\n{stdout}"
    );
    let spec = runx::runtime::resolve_runtime("bun", "0.0.0").expect("offline spec");
    let bun_bin = bun_root.join(&spec.bin_dirs[0]);
    assert!(
        stdout.contains(&bun_bin.display().to_string()),
        "bun bin dir must be on the child PATH:\n{stdout}"
    );
    assert!(
        stdout.contains("/usr/bin") || stdout.contains("/bin"),
        "the system PATH must survive intact:\n{stdout}"
    );
}

// ── Auto-detection diagnostics ────────────────────────────────────────────────

/// A modern Bun project (bun.lock, no other metadata) is detected and
/// reported by doctor without touching the network.
#[test]
fn doctor_reports_auto_detected_project_runtimes() {
    let dir = tmp();
    let home = dir.path().join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(dir.path().join("bun.lock"), "{}\n").unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"scripts": {"dev": "next dev"}}"#,
    )
    .unwrap();

    let output = runx_with_home(dir.path(), &home, &["doctor"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    let stdout = stdout_of(&output);
    assert!(stdout.contains("Project runtimes detected"), "{stdout}");
    assert!(stdout.contains("bun"), "{stdout}");
    assert!(stdout.contains("bun.lock"), "{stdout}");
}

/// A mixed Python + Bun project is reported with both requirements and their
/// sources, plus a note that the open-ended Bun requirement resolves later.
#[test]
fn doctor_reports_multi_runtime_projects_with_sources() {
    let dir = tmp();
    let home = dir.path().join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(dir.path().join(".python-version"), "3.13\n").unwrap();
    fs::write(dir.path().join("bun.lock"), "{}\n").unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"scripts": {"dev": "bun run spec"}}"#,
    )
    .unwrap();

    let output = runx_with_home(dir.path(), &home, &["doctor"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    let stdout = stdout_of(&output);
    assert!(stdout.contains("Project runtimes detected"), "{stdout}");
    assert!(stdout.contains("python"), "{stdout}");
    assert!(stdout.contains(".python-version"), "{stdout}");
    assert!(stdout.contains("bun"), "{stdout}");
    assert!(stdout.contains("bun.lock"), "{stdout}");
    assert!(
        stdout.contains("resolves to a concrete version"),
        "{stdout}"
    );
}

/// Detection and command inference are separate: a Go project has a valid
/// runtime requirement but no inferable command, and the error must say
/// exactly that instead of complaining about a package.json.
#[test]
fn go_project_without_inferable_command_errors_clearly() {
    let dir = tmp();
    fs::write(dir.path().join("go.mod"), "module m\n\ngo 1.22.5\n").unwrap();

    let output = runx(dir.path(), &["dev"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("go 1.22.5"),
        "should name the detection:\n{stderr}"
    );
    assert!(
        stderr.contains("Could not infer any run command"),
        "should separate detection from inference:\n{stderr}"
    );
    assert!(
        stderr.contains("runx init"),
        "should point at init:\n{stderr}"
    );
}

// ── Project working directory ────────────────────────────────────────────────
//
// Commands must execute in the directory the user invoked runx from. A
// directory with no project files that happens to sit under a configured
// ancestor must not silently run the ancestor's project — the regression
// these tests pin: every `runx dev` in a nested static directory ran the
// parent project's Vite app instead of staying put.
//
// The fake node binary doubles as the managed-runtime check: only runx's
// cached runtime prints NODE_FAKE, so the output proves both the runtime
// selection and the working directory.

/// Canonicalize a tempdir path so `$PWD` comparisons survive macOS's
/// `/var` → `/private/var` resolution.
#[cfg(unix)]
fn real_path(dir: &Path) -> String {
    fs::canonicalize(dir)
        .expect("canonicalize")
        .display()
        .to_string()
}

/// `dev = "node -e <anything>"`: the fake node prints its runtime marker, its
/// working directory and its arguments.
#[cfg(unix)]
const FAKE_NODE_DEV: &str = "dev = \"node -e x\"\n";

/// A cached fake node whose script reports `NODE_FAKE|<cwd>|<args>`.
#[cfg(unix)]
fn plant_reporting_node(home: &Path) {
    plant_executable(home, "node", "0.0.0", "echo \"NODE_FAKE|$PWD|$@\"");
}

/// Test A — running from a project executes the child in that project.
#[cfg(unix)]
#[test]
fn child_runs_in_the_project_directory() {
    let dir = tmp();
    let home = dir.path().join("home");
    plant_reporting_node(&home);
    fs::write(
        config_path(dir.path()),
        format!("[runtimes]\nnode = \"0.0.0\"\n\n[run]\n{FAKE_NODE_DEV}"),
    )
    .expect("write runx.toml");

    let output = runx_with_home(dir.path(), &home, &["dev"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    let stdout = stdout_of(&output);
    assert!(
        stdout.contains("NODE_FAKE"),
        "the managed runtime must run, not a system node:\n{stdout}"
    );
    assert!(
        stdout.contains(&real_path(dir.path())),
        "child must run in the project directory:\n{stdout}"
    );
}

/// Test B — sibling projects each run their own command in their own
/// directory; the second execution never reuses the first project.
#[cfg(unix)]
#[test]
fn sibling_projects_run_in_their_own_directories() {
    let root = tmp();
    let home = root.path().join("home");
    plant_reporting_node(&home);
    let projects = root.path().join("projects");
    for name in ["project-a", "project-b"] {
        let project = projects.join(name);
        fs::create_dir_all(&project).expect("create project");
        fs::write(
            project.join("runx.toml"),
            format!("[runtimes]\nnode = \"0.0.0\"\n\n[run]\n{FAKE_NODE_DEV}"),
        )
        .expect("write runx.toml");
    }

    for name in ["project-a", "project-b"] {
        let project = projects.join(name);
        let output = runx_with_home(&project, &home, &["dev"]);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{name} stderr: {}",
            stderr_of(&output)
        );
        assert!(
            stdout_of(&output).contains(&real_path(&project)),
            "{name} must run in its own directory:\n{}",
            stdout_of(&output)
        );
    }
}

/// The core regression — a nested directory with no project files must not
/// silently adopt the configured ancestor: the child stays in the nested
/// directory and the ancestor sourcing is made explicit.
#[cfg(unix)]
#[test]
fn nested_directory_without_project_files_runs_in_place() {
    let dir = tmp();
    let home = dir.path().join("home");
    plant_reporting_node(&home);
    fs::write(
        config_path(dir.path()),
        format!("[runtimes]\nnode = \"0.0.0\"\n\n[run]\n{FAKE_NODE_DEV}"),
    )
    .expect("write runx.toml");
    let nested = dir.path().join("nested");
    fs::create_dir_all(&nested).expect("create nested dir");

    let output = runx_with_home(&nested, &home, &["dev"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    let stdout = stdout_of(&output);
    assert!(
        stdout.contains(&format!("|{}|", real_path(&nested))),
        "child must run in the invocation directory, not the ancestor:\n{stdout}"
    );
    assert!(
        !stdout.contains(&format!("|{}|", real_path(dir.path()))),
        "must not run the ancestor's project:\n{stdout}"
    );
    assert!(
        stderr_of(&output).contains("Note: using runtimes and commands from"),
        "ancestor sourcing must be explicit:\n{}",
        stderr_of(&output)
    );
}

/// Test C — `runx dev -- --port 4000` forwards the arguments after the
/// working-directory fix.
#[cfg(unix)]
#[test]
fn passthrough_arguments_reach_the_child_in_the_project_directory() {
    let dir = tmp();
    let home = dir.path().join("home");
    plant_reporting_node(&home);
    fs::write(
        config_path(dir.path()),
        format!("[runtimes]\nnode = \"0.0.0\"\n\n[run]\n{FAKE_NODE_DEV}"),
    )
    .expect("write runx.toml");

    let output = runx_with_home(dir.path(), &home, &["dev", "--", "--port", "4000"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    let stdout = stdout_of(&output);
    for fragment in ["--port", "4000"] {
        assert!(
            stdout.contains(fragment),
            "passthrough arg {fragment:?} must reach the child:\n{stdout}"
        );
    }
    assert!(
        stdout.contains(&real_path(dir.path())),
        "child must still run in the project directory:\n{stdout}"
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
        vec!["doctor"],
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

// ── runx doctor ──────────────────────────────────────────────────────────────

/// A directory that looks like a runtime but holds no executable: the
/// fingerprint of a truncated install.
fn plant_broken_runtime(home: &Path, tool: &str, version: &str) -> std::path::PathBuf {
    let root = home.join("runtimes").join(tool).join(version);
    fs::create_dir_all(&root).expect("create runtime dirs");
    fs::write(root.join("partial-file"), vec![0u8; 8]).expect("write partial content");
    root
}

#[test]
fn doctor_reports_no_cache_as_healthy() {
    let dir = tmp();
    let home = dir.path().join("home");

    let output = runx_with_home(dir.path(), &home, &["doctor"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(stdout_of(&output).contains("no cache yet"));
}

#[test]
fn doctor_healthy_cache_exits_zero() {
    let dir = tmp();
    let home = dir.path().join("home");
    plant_runtime(&home, "node", "20.11.0", true);

    let output = runx_with_home(dir.path(), &home, &["doctor"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    let stdout = stdout_of(&output);
    assert!(stdout.contains("node 20.11.0"), "{stdout}");
    assert!(stdout.contains("everything looks healthy"), "{stdout}");
}

/// Doctor must show the exact PATH it would prepend for the current project's
/// runtimes, answering "why is the wrong version running" directly.
#[test]
fn doctor_shows_resolved_path_for_project_runtimes() {
    let dir = tmp();
    let home = dir.path().join("home");
    plant_runtime(&home, "node", "20.11.0", true);
    fs::write(
        config_path(dir.path()),
        "[runtimes]\nnode = \"20.11.0\"\n\n[run]\nbench = \"echo x\"\n",
    )
    .expect("write runx.toml");

    let output = runx_with_home(dir.path(), &home, &["doctor"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    let stdout = stdout_of(&output);
    // Derive the expected bin dir from the same source of truth the binary
    // uses: node's Windows layout has no `bin` subdirectory (the exe sits at
    // the install root), so hardcoding `.../bin` fails on Windows.
    let spec = runx::runtime::resolve_runtime("node", "20.11.0").expect("node resolves");
    // Reuse the library's root builder, joining components one at a time: a
    // single `join("node/20.11.0")` would keep the forward slashes on Windows
    // while the binary's construction yields backslashes.
    let root = runx::cache::runtime_root_in(&home, &spec.tool, &spec.version);
    let expected = format!(
        "Resolved PATH for `node`: {}",
        root.join(&spec.bin_dirs[0]).display()
    );
    assert!(
        stdout.contains(&expected),
        "expected:\n{expected}\nstdout:\n{stdout}"
    );
}

/// A runtime the project asks for but that is not cached is not fabricated:
/// doctor is diagnostic and must not resolve ranges or hint at downloads.
#[test]
fn doctor_skips_resolved_path_for_missing_runtimes() {
    let dir = tmp();
    let home = dir.path().join("home");
    plant_runtime(&home, "node", "20.11.0", true);
    fs::write(
        config_path(dir.path()),
        "[runtimes]\nnode = \"20.11.0\"\npython = \"3.11.7\"\n\n[run]\nbench = \"echo x\"\n",
    )
    .expect("write runx.toml");

    let output = runx_with_home(dir.path(), &home, &["doctor"]);
    assert_eq!(output.status.code(), Some(0));
    let stdout = stdout_of(&output);
    assert!(stdout.contains("Resolved PATH for `node`"), "{stdout}");
    assert!(
        !stdout.contains("Resolved PATH for `python`"),
        "an uncached runtime must not be reported as resolved:\n{stdout}"
    );
}

/// runtimes installed before the completion marker existed have a working
/// executable but no receipt; doctor must not scream at them, since they are
/// adopted automatically on next use.
#[test]
fn doctor_accepts_a_legacy_runtime_without_a_receipt() {
    let dir = tmp();
    let home = dir.path().join("home");
    plant_runtime(&home, "python", "3.11.7", false);

    // Python resolution normally consults the GitHub API to learn the asset
    // URLs. Plant a fresh release cache so this test stays offline, exactly
    // like the node tests: doctor only inspects the cache layout. The
    // `cached_at_secs` must be recent — the cache treats entries older than
    // 24h as stale and falls back to the network.
    let cache_file = home.join("python-release-cache.json");
    let platforms = [
        "x86_64-unknown-linux-gnu",
        "aarch64-unknown-linux-gnu",
        "x86_64-apple-darwin",
        "aarch64-apple-darwin",
        "x86_64-pc-windows-msvc",
        "aarch64-pc-windows-msvc",
    ];
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock after 1970")
        .as_secs();
    let entries = platforms
        .iter()
        .map(|platform| {
            format!(
                r#""{platform}": {{"url": "https://example.invalid/cpython.tgz",
                    "checksum_url": "https://example.invalid/cpython.tgz.sha256",
                    "cached_at_secs": {now}}}"#
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    fs::write(cache_file, format!(r#"{{"3.11.7": {{{entries}}}}}"#))
        .expect("write python release cache");

    let output = runx_with_home(dir.path(), &home, &["doctor"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(stdout_of(&output).contains("legacy install"));
}

#[test]
fn doctor_flags_a_truncated_runtime() {
    let dir = tmp();
    let home = dir.path().join("home");
    plant_broken_runtime(&home, "node", "20.11.0");

    let output = runx_with_home(dir.path(), &home, &["doctor"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(
        stderr_of(&output).contains("1 issue"),
        "stderr:\n{}",
        stderr_of(&output)
    );
}

#[test]
fn doctor_flags_empty_orphan_directories() {
    let dir = tmp();
    let home = dir.path().join("home");
    fs::create_dir_all(home.join("runtimes").join("node").join("0.0.0")).unwrap();

    let output = runx_with_home(dir.path(), &home, &["doctor"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(
        stdout_of(&output).contains("empty orphan"),
        "stdout:\n{}",
        stdout_of(&output)
    );
}

/// Downloads abandoned for over the 1-hour grace period (a killed process, a
/// crash) must surface so `runx cache prune --yes` can reclaim the space.
#[cfg(unix)]
#[test]
fn doctor_flags_abandoned_staging_downloads() {
    let dir = tmp();
    let home = dir.path().join("home");
    let staging = home
        .join("runtimes")
        .join("node")
        .join(".staging-20.11.0-1-1-1");
    fs::create_dir_all(&staging).unwrap();
    fs::write(staging.join("fragment"), vec![0u8; 8]).unwrap();

    let older = std::time::SystemTime::now() - std::time::Duration::from_secs(2 * 3600);
    fs::File::open(&staging)
        .unwrap()
        .set_modified(older)
        .unwrap();

    let output = runx_with_home(dir.path(), &home, &["doctor"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(
        stdout_of(&output).contains("abandoned download"),
        "stdout:\n{}",
        stdout_of(&output)
    );
}

// ── doctor --verify ──────────────────────────────────────────────────────────

/// A runtime whose executable matches the digest recorded in its receipt must
/// pass `doctor --verify` explicitly, not just the presence check.
#[test]
fn doctor_verify_confirms_untouched_runtimes() {
    let dir = tmp();
    let home = tmp();
    let root = plant_runtime(home.path(), "node", "20.11.0", true);

    // Rewrite the receipt with a digest of the actual planted bytes.
    use sha2::{Digest as _, Sha256};
    let exe_path = if cfg!(windows) {
        root.join("node.exe")
    } else {
        root.join("bin/node")
    };
    let digest = {
        let bytes = fs::read(&exe_path).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        format!("{:x}", hasher.finalize())
    };
    fs::write(
        root.join(".runx-complete.json"),
        format!(
            r#"{{"tool":"node","version":"20.11.0","installed_at_secs":0,
                "runx_version":"test","source_url":"","sha256":null,
                "executable_sha256":"{digest}"}}"#
        ),
    )
    .unwrap();

    let output = runx_with_home(dir.path(), home.path(), &["doctor", "--verify"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    let stdout = stdout_of(&output);
    assert!(stdout.contains("(verified)"), "stdout:\n{stdout}");
}

/// Bytes replaced after install must fail `doctor --verify` — this is the
/// corruption case the fast check cannot see.
#[test]
fn doctor_verify_catches_tampered_executables() {
    let dir = tmp();
    let home = tmp();
    let root = plant_runtime(home.path(), "node", "20.11.0", true);

    // Record the digest of the original 4096 zero bytes…
    use sha2::{Digest as _, Sha256};
    let exe_path = if cfg!(windows) {
        root.join("node.exe")
    } else {
        root.join("bin/node")
    };
    let original = {
        let bytes = fs::read(&exe_path).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        format!("{:x}", hasher.finalize())
    };
    fs::write(
        root.join(".runx-complete.json"),
        format!(
            r#"{{"tool":"node","version":"20.11.0","installed_at_secs":0,
                "runx_version":"test","source_url":"","sha256":null,
                "executable_sha256":"{original}"}}"#
        ),
    )
    .unwrap();

    // …then swap the bytes.
    fs::write(&exe_path, b"tampered").unwrap();

    let output = runx_with_home(dir.path(), home.path(), &["doctor", "--verify"]);
    assert_eq!(
        output.status.code(),
        Some(1),
        "a tampered runtime must fail doctor --verify; stderr: {}",
        stderr_of(&output)
    );
    let stdout = stdout_of(&output);
    assert!(
        stdout.contains("does not match the digest"),
        "stdout:\n{stdout}"
    );
}

/// Legacy receipts without an executable digest are reported as unverifiable
/// (a note), never as corruption.
#[test]
fn doctor_verify_reports_legacy_receipts_without_failing() {
    let dir = tmp();
    let home = tmp();
    plant_runtime(home.path(), "node", "20.11.0", true); // receipt has no digest field

    let output = runx_with_home(dir.path(), home.path(), &["doctor", "--verify"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    let stdout = stdout_of(&output);
    assert!(
        stdout.contains("no recorded executable digest"),
        "stdout:\n{stdout}"
    );
}

/// Plain `doctor` stays fast and does NOT hash anything: a tampered runtime
/// with a valid marker still passes the default check (that is exactly why
/// `--verify` exists).
#[test]
fn plain_doctor_does_not_hash_executables() {
    let dir = tmp();
    let home = tmp();
    let root = plant_runtime(home.path(), "node", "20.11.0", true);

    use sha2::{Digest as _, Sha256};
    let exe_path = if cfg!(windows) {
        root.join("node.exe")
    } else {
        root.join("bin/node")
    };
    let original = {
        let bytes = fs::read(&exe_path).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        format!("{:x}", hasher.finalize())
    };
    fs::write(
        root.join(".runx-complete.json"),
        format!(
            r#"{{"tool":"node","version":"20.11.0","installed_at_secs":0,
                "runx_version":"test","source_url":"","sha256":null,
                "executable_sha256":"{original}"}}"#
        ),
    )
    .unwrap();
    fs::write(&exe_path, b"tampered").unwrap();

    let output = runx_with_home(dir.path(), home.path(), &["doctor"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "plain doctor checks structure only; stderr: {}",
        stderr_of(&output)
    );
}

// ── --json / --quiet / --offline ─────────────────────────────────────────────

/// `--json` errors must be a single JSON object on stderr with an "error"
/// field, so CI can detect failure without parsing prose.
#[test]
fn json_mode_emits_machine_readable_errors() {
    let dir = tmp();
    fs::write(config_path(dir.path()), "[run]\ndev = \"echo hi\"\n").unwrap();
    let output = runx_with_home(dir.path(), dir.path(), &["--json", "nosuchkey"]);
    assert_eq!(output.status.code(), Some(1));

    let stderr = stderr_of(&output);
    let parsed: serde_json::Value = serde_json::from_str(&stderr)
        .unwrap_or_else(|err| panic!("stderr must be one JSON object ({err}):\n{stderr}"));
    let message = parsed["error"].as_str().expect("error must be a string");
    assert!(
        message.contains("nosuchkey"),
        "the error should name the bad key, got: {message}"
    );
}

/// `cache list --json` must emit valid JSON on stdout even when runx prints
/// informational lines elsewhere.
#[test]
fn json_cache_list_reports_runtimes() {
    let dir = tmp();
    let home = tmp();
    plant_runtime(home.path(), "node", "20.11.0", true);

    let output = runx_with_home(dir.path(), home.path(), &["--json", "cache", "list"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );

    let stdout = stdout_of(&output);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("stdout must be JSON ({err}):\n{stdout}"));
    let runtimes = parsed["runtimes"].as_array().expect("runtimes array");
    assert_eq!(runtimes.len(), 1);
    assert_eq!(runtimes[0]["tool"], "node");
    assert_eq!(runtimes[0]["version"], "20.11.0");
    assert_eq!(runtimes[0]["complete"], true);
}

/// `doctor --json --verify` must report per-runtime verification status as
/// structured data, and stay healthy for untouched installs.
#[test]
fn json_doctor_verify_reports_statuses() {
    let dir = tmp();
    let home = tmp();
    let root = plant_runtime(home.path(), "node", "20.11.0", true);

    use sha2::{Digest as _, Sha256};
    let exe_path = if cfg!(windows) {
        root.join("node.exe")
    } else {
        root.join("bin/node")
    };
    let digest = {
        let bytes = fs::read(&exe_path).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        format!("{:x}", hasher.finalize())
    };
    fs::write(
        root.join(".runx-complete.json"),
        format!(
            r#"{{"tool":"node","version":"20.11.0","installed_at_secs":0,
                "runx_version":"test","source_url":"","sha256":null,
                "executable_sha256":"{digest}"}}"#
        ),
    )
    .unwrap();

    let output = runx_with_home(dir.path(), home.path(), &["--json", "doctor", "--verify"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );

    let stdout = stdout_of(&output);
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|err| panic!("stdout must be JSON ({err}):\n{stdout}"));
    assert_eq!(parsed["healthy"], true);
    assert_eq!(parsed["runtimes"][0]["status"], "verified");
}

/// A corrupted runtime must surface in `doctor --json --verify` both as
/// status "corrupt" and as a non-zero exit with a JSON error on stderr.
#[test]
fn json_doctor_verify_flags_corruption_and_fails() {
    let dir = tmp();
    let home = tmp();
    let root = plant_runtime(home.path(), "node", "20.11.0", true);

    use sha2::{Digest as _, Sha256};
    let exe_path = if cfg!(windows) {
        root.join("node.exe")
    } else {
        root.join("bin/node")
    };
    let original = {
        let bytes = fs::read(&exe_path).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        format!("{:x}", hasher.finalize())
    };
    fs::write(
        root.join(".runx-complete.json"),
        format!(
            r#"{{"tool":"node","version":"20.11.0","installed_at_secs":0,
                "runx_version":"test","source_url":"","sha256":null,
                "executable_sha256":"{original}"}}"#
        ),
    )
    .unwrap();
    fs::write(&exe_path, b"tampered").unwrap();

    let output = runx_with_home(dir.path(), home.path(), &["--json", "doctor", "--verify"]);
    assert_eq!(output.status.code(), Some(1));

    let body = serde_json::Value::Object(
        [(
            "body".to_string(),
            serde_json::Value::String(stdout_of(&output)),
        )]
        .into_iter()
        .collect(),
    );
    drop(body);
    let stdout_parsed: serde_json::Value = serde_json::from_str(&stdout_of(&output))
        .unwrap_or_else(|err| panic!("stdout must be JSON ({err})"));
    assert_eq!(stdout_parsed["healthy"], false);
    assert_eq!(stdout_parsed["runtimes"][0]["status"], "corrupt");

    let stderr = stderr_of(&output);
    let stderr_parsed: serde_json::Value = serde_json::from_str(&stderr)
        .unwrap_or_else(|err| panic!("stderr must be JSON ({err}):\n{stderr}"));
    assert!(
        stderr_parsed["error"]
            .as_str()
            .is_some_and(|m| m.contains("1 issue")),
        "stderr error should summarize the failure: {stderr}"
    );
}

/// `--quiet` suppresses runx's own lines but not the command's output or
/// exit code.
#[test]
fn quiet_mode_silences_banners_but_not_the_command() {
    let dir = tmp();
    let home = tmp();
    plant_runtime(home.path(), "node", "20.11.0", true);
    fs::write(
        config_path(dir.path()),
        "[runtimes]\nnode = \"20.11.0\"\n\n[run]\ndev = \"echo CHILD_OUTPUT\"\n",
    )
    .unwrap();

    let loud = runx_with_home(dir.path(), home.path(), &["run", "dev"]);
    assert!(
        stdout_of(&loud).contains("Using cached"),
        "human mode keeps the banner:\n{}",
        stdout_of(&loud)
    );

    let quiet = runx_with_home(dir.path(), home.path(), &["--quiet", "run", "dev"]);
    assert_eq!(quiet.status.code(), Some(0));
    let stdout = stdout_of(&quiet);
    assert!(
        !stdout.contains("Using cached") && !stdout.contains("Running"),
        "quiet mode must drop banners:\n{stdout}"
    );
    assert!(
        stdout.contains("CHILD_OUTPUT"),
        "quiet mode keeps child output:\n{stdout}"
    );
}

/// `--offline` must allow cached exact pins to run untouched.
#[test]
fn offline_mode_runs_cached_exact_pins() {
    let dir = tmp();
    let home = tmp();
    plant_runtime(home.path(), "node", "20.11.0", true);
    fs::write(
        config_path(dir.path()),
        "[runtimes]\nnode = \"20.11.0\"\n\n[run]\ndev = \"echo OFFLINE_OK\"\n",
    )
    .unwrap();

    let output = runx_with_home(dir.path(), home.path(), &["--offline", "run", "dev"]);
    assert_eq!(
        output.status.code(),
        Some(0),
        "stderr: {}",
        stderr_of(&output)
    );
    assert!(stdout_of(&output).contains("OFFLINE_OK"));
}

/// `--offline` must fail fast and clearly for anything that would need the
/// network — here, an uncached runtime whose download is refused before any
/// HTTP happens.
#[test]
fn offline_mode_refuses_downloads() {
    let dir = tmp();
    let home = tmp();
    fs::write(
        config_path(dir.path()),
        "[runtimes]\nnode = \"20.11.0\"\n\n[run]\ndev = \"echo unreachable\"\n",
    )
    .unwrap();

    let output = runx_with_home(dir.path(), home.path(), &["--offline", "run", "dev"]);
    assert_eq!(output.status.code(), Some(1));
    let combined = format!("{}{}", stderr_of(&output), stdout_of(&output));
    assert!(
        combined.contains("--offline"),
        "the error must name the flag that caused it:\n{combined}"
    );
    // The refusal must happen before any download attempt.
    assert!(!combined.contains("Downloading"));
}

// ── Missing-dependencies hint ────────────────────────────────────────────────

/// When a command fails and the project has package.json + a lockfile but no
/// node_modules, stderr must hint at `bun install`.
#[cfg(unix)]
#[test]
fn failed_command_with_missing_node_modules_suggests_bun_install() {
    let dir = tmp();
    let home = dir.path().join("home");
    plant_executable(&home, "node", "0.0.0", "exit 1");
    // Package with bun.lock but no node_modules.
    fs::write(
        dir.path().join("package.json"),
        r#"{"scripts":{"dev":"exit 1"}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("bun.lock"), "{}\n").unwrap();
    fs::write(
        config_path(dir.path()),
        "[runtimes]\nnode = \"0.0.0\"\n\n[run]\ndev = \"exit 1\"\n",
    )
    .unwrap();

    let output = runx_with_home(dir.path(), &home, &["dev"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("bun install"),
        "should hint at bun install, got:\n{stderr}"
    );
}

/// Same for npm: package-lock.json → `npm install`.
#[cfg(unix)]
#[test]
fn failed_command_with_missing_node_modules_suggests_npm_install() {
    let dir = tmp();
    let home = dir.path().join("home");
    plant_executable(&home, "node", "0.0.0", "exit 1");
    fs::write(
        dir.path().join("package.json"),
        r#"{"scripts":{"dev":"exit 1"}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("package-lock.json"), "{}").unwrap();
    fs::write(
        config_path(dir.path()),
        "[runtimes]\nnode = \"0.0.0\"\n\n[run]\ndev = \"exit 1\"\n",
    )
    .unwrap();

    let output = runx_with_home(dir.path(), &home, &["dev"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("npm install"),
        "should hint at npm install, got:\n{stderr}"
    );
}

/// Legacy bun.lockb also suggests bun install.
#[cfg(unix)]
#[test]
fn failed_command_with_missing_node_modules_suggests_bun_install_from_lockb() {
    let dir = tmp();
    let home = dir.path().join("home");
    plant_executable(&home, "node", "0.0.0", "exit 1");
    fs::write(
        dir.path().join("package.json"),
        r#"{"scripts":{"dev":"exit 1"}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("bun.lockb"), "{}\n").unwrap();
    fs::write(
        config_path(dir.path()),
        "[runtimes]\nnode = \"0.0.0\"\n\n[run]\ndev = \"exit 1\"\n",
    )
    .unwrap();

    let output = runx_with_home(dir.path(), &home, &["dev"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("bun install"),
        "should hint at bun install from bun.lockb, got:\n{stderr}"
    );
}

/// yarn.lock → yarn install.
#[cfg(unix)]
#[test]
fn failed_command_with_missing_node_modules_suggests_yarn_install() {
    let dir = tmp();
    let home = dir.path().join("home");
    plant_executable(&home, "node", "0.0.0", "exit 1");
    fs::write(
        dir.path().join("package.json"),
        r#"{"scripts":{"dev":"exit 1"}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("yarn.lock"), "# yarn lockfile\n").unwrap();
    fs::write(
        config_path(dir.path()),
        "[runtimes]\nnode = \"0.0.0\"\n\n[run]\ndev = \"exit 1\"\n",
    )
    .unwrap();

    let output = runx_with_home(dir.path(), &home, &["dev"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("yarn install"),
        "should hint at yarn install, got:\n{stderr}"
    );
}

/// pnpm-lock.yaml → pnpm install.
#[cfg(unix)]
#[test]
fn failed_command_with_missing_node_modules_suggests_pnpm_install() {
    let dir = tmp();
    let home = dir.path().join("home");
    plant_executable(&home, "node", "0.0.0", "exit 1");
    fs::write(
        dir.path().join("package.json"),
        r#"{"scripts":{"dev":"exit 1"}}"#,
    )
    .unwrap();
    fs::write(
        dir.path().join("pnpm-lock.yaml"),
        "lockfileVersion: '6.0'\n",
    )
    .unwrap();
    fs::write(
        config_path(dir.path()),
        "[runtimes]\nnode = \"0.0.0\"\n\n[run]\ndev = \"exit 1\"\n",
    )
    .unwrap();

    let output = runx_with_home(dir.path(), &home, &["dev"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("pnpm install"),
        "should hint at pnpm install, got:\n{stderr}"
    );
}

/// package.json without a recognized lockfile falls back to npm install.
#[cfg(unix)]
#[test]
fn failed_command_with_missing_node_modules_fallbacks_to_npm() {
    let dir = tmp();
    let home = dir.path().join("home");
    plant_executable(&home, "node", "0.0.0", "exit 1");
    fs::write(
        dir.path().join("package.json"),
        r#"{"scripts":{"dev":"exit 1"}}"#,
    )
    .unwrap();
    fs::write(
        config_path(dir.path()),
        "[runtimes]\nnode = \"0.0.0\"\n\n[run]\ndev = \"exit 1\"\n",
    )
    .unwrap();

    let output = runx_with_home(dir.path(), &home, &["dev"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr_of(&output);
    assert!(
        stderr.contains("npm install"),
        "should fallback to npm install, got:\n{stderr}"
    );
}

/// No hint when node_modules already exists — dependencies are installed.
#[test]
fn failed_command_with_node_modules_does_not_hint() {
    let dir = tmp();
    let home = dir.path().join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"scripts":{"dev":"exit 1"}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("bun.lock"), "{}\n").unwrap();
    fs::create_dir_all(dir.path().join("node_modules")).unwrap();

    let output = runx_with_home(dir.path(), &home, &["dev"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr_of(&output);
    assert!(
        !stderr.contains("dependencies are not installed"),
        "should not hint when node_modules exists:\n{stderr}"
    );
}

/// No hint when there's no package.json — not a JS project.
#[test]
fn failed_command_without_package_json_does_not_hint() {
    let dir = tmp();
    let home = dir.path().join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(config_path(dir.path()), "[run]\ndev = \"exit 1\"\n").unwrap();

    let output = runx_with_home(dir.path(), &home, &["dev"]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = stderr_of(&output);
    assert!(
        !stderr.contains("dependencies are not installed"),
        "should not hint for non-JS projects:\n{stderr}"
    );
}
