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

fn isolated_path(runtimes: &[CachedRuntime]) -> Result<OsString> {
    let mut paths: Vec<PathBuf> = Vec::new();
    for runtime in runtimes {
        for dir in &runtime.bin_dirs {
            paths.push(dir.clone());
        }
    }
    paths.extend(minimal_safe_path());
    env::join_paths(paths).context("Failed to build isolated PATH")
}

fn minimal_safe_path() -> Vec<PathBuf> {
    if cfg!(windows) {
        vec![
            PathBuf::from(r"C:\Windows\System32"),
            PathBuf::from(r"C:\Windows"),
            PathBuf::from(r"C:\Windows\System32\Wbem"),
        ]
    } else {
        vec![PathBuf::from("/usr/bin"), PathBuf::from("/bin")]
    }
}
