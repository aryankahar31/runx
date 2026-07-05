// The library crate (lib.rs) owns all modules.  The binary just imports them.
use runx::cache;
use runx::config;
use runx::downloader;
use runx::executor;
use runx::extractor;
use runx::runtime;

use anyhow::{Context, Result};
use clap::{CommandFactory, Parser};
use std::{env, fs, process};

#[derive(Debug, Parser)]
#[command(
    name = "runx",
    version,
    about = "Universal project launcher with portable runtimes"
)]
struct Cli {
    /// Command key from [run] in runx.toml, or `init`.
    /// If runx.toml is missing, runx auto-detects from standard project files (.nvmrc, .node-version, package.json, pyproject.toml, etc.).
    command: Option<String>,
}

fn main() {
    if let Err(err) = run() {
        eprintln!("Error: {err:#}");
        process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command.as_deref() {
        Some("init") => init_config(),
        Some(command) => run_command(command),
        None => {
            Cli::command().print_help()?;
            println!();
            Ok(())
        }
    }
}

fn init_config() -> Result<()> {
    let cwd = env::current_dir().context("Failed to determine current directory")?;
    let path = cwd.join(config::CONFIG_FILE);
    if path.exists() {
        anyhow::bail!(
            "{} already exists; refusing to overwrite it.",
            path.display()
        );
    }
    fs::write(&path, config::starter_config())
        .with_context(|| format!("Failed to write {}", path.display()))?;
    println!("Created {}", path.display());
    Ok(())
}

fn run_command(command_key: &str) -> Result<()> {
    let cwd = env::current_dir().context("Failed to determine current directory")?;

    // Load config from runx.toml, or fall back to auto-detection.
    let resolved = config::load_or_detect(&cwd)?;

    // Print the transparency banner when auto-detection was used.
    if !resolved.detection_lines.is_empty() {
        println!("No runx.toml found — detected from project files:");
        for line in &resolved.detection_lines {
            println!("{line}");
        }
    }

    let config = resolved.inner;
    let command = config.command(command_key)?.to_string();

    let mut cached = Vec::new();
    for (tool, version) in &config.runtimes {
        let spec = runtime::resolve_runtime(tool, version)
            .with_context(|| format!("Failed to resolve runtime {tool} {version}"))?;
        if let Some(rt) = cache::cached_runtime(&spec)? {
            println!(
                "Using cached {} {} at {}",
                spec.tool,
                spec.version,
                rt.root.display()
            );
            cached.push(rt);
            continue;
        }

        println!("Installing {} {}", spec.tool, spec.version);
        let archive = downloader::download_to_temp(&spec.url)?;
        let root = cache::prepare_runtime_dir(&spec)?;
        let extraction = extractor::extract_archive(&archive, &root, spec.archive_kind);
        downloader::remove_temp_file(&archive);
        extraction?;
        let rt = cache::finalize_cached_runtime(&root, &spec)?;
        cached.push(rt);
    }

    let status = executor::execute(&command, &cached, &cwd)?;
    process::exit(status.code().unwrap_or(1));
}
