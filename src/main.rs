mod cache;
mod config;
mod downloader;
mod error;
mod executor;
mod extractor;
mod runtime;

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
    /// Command key from [run] in runx.toml, or `init`
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
    let config = config::RunxConfig::load_from_dir(&cwd)?;
    let command = config.command(command_key)?.to_string();

    let mut cached = Vec::new();
    for (tool, version) in &config.runtimes {
        let spec = runtime::resolve_runtime(tool, version)
            .with_context(|| format!("Failed to resolve runtime {tool} {version}"))?;
        if let Some(runtime) = cache::cached_runtime(&spec)? {
            println!(
                "Using cached {} {} at {}",
                spec.tool,
                spec.version,
                runtime.root.display()
            );
            cached.push(runtime);
            continue;
        }

        println!("Installing {} {}", spec.tool, spec.version);
        let archive = downloader::download_to_temp(&spec.url)?;
        let root = cache::prepare_runtime_dir(&spec)?;
        let extraction = extractor::extract_archive(&archive, &root, spec.archive_kind);
        downloader::remove_temp_file(&archive);
        extraction?;
        let runtime = cache::finalize_cached_runtime(&root, &spec)?;
        cached.push(runtime);
    }

    let status = executor::execute(&command, &cached, &cwd)?;
    process::exit(status.code().unwrap_or(1));
}
