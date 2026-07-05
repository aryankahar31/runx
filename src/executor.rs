use crate::cache::CachedRuntime;
use anyhow::{Context, Result};
use std::{
    env,
    ffi::OsString,
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
};

pub fn execute(
    command: &str,
    runtimes: &[CachedRuntime],
    project_dir: &Path,
) -> Result<ExitStatus> {
    let path = isolated_path(runtimes)?;
    println!("Running `{command}`");

    let mut child = shell_command(command);
    child
        .current_dir(project_dir)
        .env("PATH", path)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());

    child
        .status()
        .with_context(|| format!("Failed to start command `{command}`"))
}

fn shell_command(command: &str) -> Command {
    if cfg!(windows) {
        let mut cmd = Command::new("cmd");
        cmd.arg("/C").arg(command);
        cmd
    } else {
        let mut cmd = Command::new("/bin/sh");
        cmd.arg("-c").arg(command);
        cmd
    }
}

/// Build a PATH that prepends runtime bin directories to the existing system
/// PATH. Runtime dirs come first so cached versions take priority, while all
/// system tools (git, make, curl, Homebrew, etc.) remain accessible.
fn isolated_path(runtimes: &[CachedRuntime]) -> Result<OsString> {
    let mut paths: Vec<PathBuf> = Vec::new();

    // Runtime bin dirs first — take priority over system installs.
    for runtime in runtimes {
        for dir in &runtime.bin_dirs {
            paths.push(dir.clone());
        }
    }

    // Append existing system PATH so git, make, curl, etc. still work.
    if let Some(system_path) = env::var_os("PATH") {
        for entry in env::split_paths(&system_path) {
            paths.push(entry);
        }
    }

    env::join_paths(paths).context("Failed to build isolated PATH")
}
