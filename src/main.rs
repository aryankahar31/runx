// The library crate (lib.rs) owns all modules.  The binary just imports them.
use runx::cache;
use runx::config;
use runx::downloader;
use runx::error;
use runx::executor;
use runx::extractor;
use runx::runtime;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::{env, fs, process};

/// Subcommand names that cannot be used as `[run]` keys, because clap resolves
/// them to built-in commands first.
///
/// A project that already defines one of these as a run command keeps working
/// via the explicit `runx run <key>` form, and [`warn_about_shadowed_keys`]
/// points that out rather than leaving the user to guess why `runx cache`
/// stopped running their script.
const RESERVED_COMMANDS: &[&str] = &["init", "run", "cache", "doctor", "completions", "self"];

#[derive(Debug, Parser)]
#[command(
    name = "runx",
    version,
    about = "Universal project launcher with portable runtimes",
    after_help = "Any other argument is treated as a command key from [run] in runx.toml,\n\
                  e.g. `runx dev`. If runx.toml is missing, runx auto-detects runtimes\n\
                  from standard project files (.nvmrc, .node-version, package.json,\n\
                  .python-version, pyproject.toml).\n\n\
                  Use `runx run <key>` if a command key collides with a built-in\n\
                  subcommand name."
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a starter runx.toml in the current directory.
    Init,

    /// Run a command from [run] in runx.toml by its key.
    ///
    /// Use this when a key collides with a built-in subcommand name.
    Run {
        /// The command key to run.
        key: String,
    },

    /// Any other word is treated as a [run] command key, so `runx dev` works.
    #[command(external_subcommand)]
    External(Vec<String>),
}

fn main() {
    if let Err(err) = run() {
        eprintln!("Error: {err:#}");
        process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(Command::Init) => init_config(),
        Some(Command::Run { key }) => run_command(&key),
        Some(Command::External(args)) => dispatch_external(args),
        None => print_help(),
    }
}

fn print_help() -> Result<()> {
    use clap::CommandFactory;
    Cli::command().print_help()?;
    println!();
    Ok(())
}

/// Handle a bare `runx <word>` invocation.
///
/// Clap routes anything that is not a known subcommand here. Extra arguments
/// are rejected explicitly rather than silently ignored, so a user who tries
/// `runx dev --port 3000` gets told that argument passthrough is not supported
/// instead of watching `--port 3000` disappear.
fn dispatch_external(args: Vec<String>) -> Result<()> {
    let mut args = args.into_iter();
    let Some(key) = args.next() else {
        return print_help();
    };

    let extra: Vec<String> = args.collect();
    if !extra.is_empty() {
        return Err(error::UserError::new(format!(
            "Unexpected argument{plural} after `{key}`: {extra}\n\
             runx does not yet forward arguments to the command it runs.\n\
             Hint: add a dedicated key under [run] in runx.toml.",
            plural = if extra.len() == 1 { "" } else { "s" },
            extra = extra.join(" ")
        ))
        .into());
    }

    run_command(&key)
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

/// Warn when a config defines run keys that a built-in subcommand shadows.
fn warn_about_shadowed_keys(config: &config::RunxConfig) {
    let shadowed: Vec<&str> = config
        .run
        .keys()
        .map(String::as_str)
        .filter(|key| RESERVED_COMMANDS.contains(key))
        .collect();

    if !shadowed.is_empty() {
        eprintln!(
            "Note: the [run] key{plural} {keys} {is} reserved for runx built-in \
             subcommands.\n\
             Use `runx run <key>` to invoke {them} reliably.",
            is = if shadowed.len() == 1 { "is" } else { "are" },
            plural = if shadowed.len() == 1 { "" } else { "s" },
            keys = shadowed
                .iter()
                .map(|key| format!("`{key}`"))
                .collect::<Vec<_>>()
                .join(", "),
            them = if shadowed.len() == 1 { "it" } else { "them" }
        );
    }
}

fn run_command(command_key: &str) -> Result<()> {
    let cwd = env::current_dir().context("Failed to determine current directory")?;

    // Walk up parent directories to find the project root.
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
    warn_about_shadowed_keys(&config);
    let command = config.command(command_key)?.to_string();

    // 1. Resolve all specs first (fast, no network for pinned versions).
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

    // 3. Download all missing runtimes in parallel threads.
    //
    // Each install extracts into its own staging directory and is only renamed
    // into place once verified, so an interrupted or failed install can never
    // leave a half-extracted tree that the next run mistakes for a valid cache
    // entry — and never destroys a runtime that already worked.
    let home = cache::runx_home()?;
    let handles: Vec<_> = to_download
        .into_iter()
        .map(|spec| {
            let home = home.clone();
            std::thread::spawn(move || -> Result<cache::CachedRuntime> {
                println!("Installing {} {}", spec.tool, spec.version);
                let download = downloader::download_to_temp(&spec.url, &spec.checksum_url)?;
                let staging = cache::staging_dir(&home, &spec)?;

                // Record the digest that was actually verified, so `runx.lock`
                // reflects the installed bytes rather than a second fetch.
                let sha256 = download.sha256.clone();
                let result =
                    extractor::extract_archive(download.path(), &staging, spec.archive_kind)
                        .and_then(|()| cache::commit_runtime(&home, &staging, &spec, Some(sha256)));

                // Dropping the download deletes the temp archive even on the
                // error path, so a failed install leaks nothing.
                drop(download);

                if result.is_err() {
                    cache::discard_staging(&staging);
                }
                result
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
