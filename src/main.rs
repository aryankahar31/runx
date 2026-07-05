// The library crate (lib.rs) owns all modules.  The binary just imports them.
use runx::cache;
use runx::config;
use runx::downloader;
use runx::error;
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

    // Bug 5: walk up parent directories to find the project root.
    let project_dir = config::find_project_dir(&cwd).ok_or_else(|| {
        error::UserError::new(format!(
            "No runx.toml found in {} or any parent directory.\n\
             Hint: run `runx init` to create a starter config.",
            cwd.display()
        ))
    })?;

    // Load config from runx.toml, or fall back to auto-detection.
    let resolved = config::load_or_detect(&project_dir)?;

    // Print the transparency banner when auto-detection was used.
    if !resolved.detection_lines.is_empty() {
        println!("No runx.toml found — detected from project files:");
        for line in &resolved.detection_lines {
            println!("{line}");
        }
    }

    let config = resolved.inner;
    let command = config.command(command_key)?.to_string();

    // 1. Resolve all specs first (fast, no network).
    let specs: Vec<runtime::RuntimeSpec> = config
        .runtimes
        .iter()
        .map(|(tool, version)| {
            runtime::resolve_runtime(tool, version)
                .with_context(|| format!("Failed to resolve runtime {tool} {version}"))
        })
        .collect::<Result<_>>()?;

    // 2. Split into cached vs needs-download.
    let mut cached: Vec<cache::CachedRuntime> = Vec::new();
    let mut to_download: Vec<runtime::RuntimeSpec> = Vec::new();
    for spec in specs {
        if let Some(rt) = cache::cached_runtime(&spec)? {
            println!(
                "Using cached {} {} at {}",
                spec.tool,
                spec.version,
                rt.root.display()
            );
            cached.push(rt);
        } else {
            to_download.push(spec);
        }
    }

    // 3. Download all missing runtimes in parallel threads (Bug 7).
    let handles: Vec<_> = to_download
        .into_iter()
        .map(|spec| {
            std::thread::spawn(move || -> Result<cache::CachedRuntime> {
                println!("Installing {} {}", spec.tool, spec.version);
                let temp = downloader::download_to_temp(&spec.url, &spec.checksum_url)?;
                let root = cache::prepare_runtime_dir(&spec)?;
                let extraction = extractor::extract_archive(temp.path(), &root, spec.archive_kind);
                // Bug 6: dropping the NamedTempFile auto-deletes it, even if
                // extraction fails.
                drop(temp);
                extraction?;
                cache::finalize_cached_runtime(&root, &spec)
            })
        })
        .collect();

    for handle in handles {
        let rt = handle
            .join()
            .map_err(|_| anyhow::anyhow!("A runtime install thread panicked"))??;
        cached.push(rt);
    }

    let status = executor::execute(&command, &cached, &project_dir)?;
    process::exit(status.code().unwrap_or(1));
}
