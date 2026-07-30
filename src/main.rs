// The library crate (lib.rs) owns all modules.  The binary just imports them.
use runx::cache;
use runx::config;
use runx::downloader;
use runx::error;
use runx::executor;
use runx::extractor;
use runx::lock;
use runx::runtime;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use std::{
    env, fs,
    path::{Path, PathBuf},
    process,
};

/// Subcommand names that cannot be used as `[run]` keys, because clap resolves
/// them to built-in commands first.
///
/// A project that already defines one of these as a run command keeps working
/// via the explicit `runx run <key>` form, and [`warn_about_shadowed_keys`]
/// points that out rather than leaving the user to guess why `runx cache`
/// stopped running their script.
const RESERVED_COMMANDS: &[&str] = &[
    "init",
    "run",
    "lock",
    "cache",
    "doctor",
    "completions",
    "self",
];

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

        /// Fail instead of resolving anything runx.lock does not already pin.
        #[arg(long)]
        locked: bool,
    },

    /// Write runx.lock, pinning the exact runtimes this project resolves to.
    Lock,

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
        Some(Command::Run { key, locked }) => run_command(&key, locked),
        Some(Command::Lock) => lock_command(),
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

    run_command(&key, false)
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

/// A runtime that is present in the cache and ready to use.
struct Provisioned {
    spec: runtime::RuntimeSpec,
    cached: cache::CachedRuntime,
    /// The requirement in `runx.toml` (or detected) that produced this version.
    requirement: String,
}

/// Locate the project root and load its configuration.
///
/// Shared by `run` and `lock` so both agree on which directory is the project
/// and how auto-detection is reported.
fn load_project() -> Result<(PathBuf, config::RunxConfig)> {
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

    Ok((project_dir, resolved.inner))
}

/// Ensure every runtime the config asks for is installed, and return them.
///
/// Consults `runx.lock` first: a pinned version for an unchanged requirement is
/// used verbatim. Missing runtimes are downloaded in parallel, each into its own
/// staging directory, and renamed into place only after verification — so an
/// interrupted install can never leave a partial tree that looks cached, and a
/// failed install never destroys a runtime that already worked.
fn provision(
    project_dir: &Path,
    config: &config::RunxConfig,
    locked: bool,
) -> Result<Vec<Provisioned>> {
    let lockfile = lock::Lockfile::load(project_dir)?;
    let plan = lock::plan(&config.runtimes, lockfile.as_ref(), locked)?;

    for entry in &plan {
        if let Some(note) = &entry.note {
            eprintln!("Note: {note}");
        }
    }

    // Resolve specs first — fast, and no network for concrete versions.
    let mut specs: Vec<(runtime::RuntimeSpec, String)> = Vec::new();
    for entry in &plan {
        let requirement = config
            .runtimes
            .get(&entry.tool)
            .cloned()
            .unwrap_or_else(|| entry.version.clone());

        let spec = runtime::resolve_runtime(&entry.tool, &entry.version).with_context(|| {
            format!("Failed to resolve runtime {} {}", entry.tool, entry.version)
        })?;
        specs.push((spec, requirement));
    }

    // Split into already-cached and needs-download.
    let mut provisioned: Vec<Provisioned> = Vec::new();
    let mut to_download: Vec<(runtime::RuntimeSpec, String)> = Vec::new();
    for (spec, requirement) in specs {
        match cache::cached_runtime(&spec)? {
            Some(cached) => {
                println!(
                    "Using cached {} {} at {}",
                    spec.tool,
                    spec.version,
                    cached.root.display()
                );
                provisioned.push(Provisioned {
                    spec,
                    cached,
                    requirement,
                });
            }
            None => to_download.push((spec, requirement)),
        }
    }

    let home = cache::runx_home()?;
    let handles: Vec<_> = to_download
        .into_iter()
        .map(|(spec, requirement)| {
            let home = home.clone();
            std::thread::spawn(move || -> Result<Provisioned> {
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

                match result {
                    Ok(cached) => Ok(Provisioned {
                        spec,
                        cached,
                        requirement,
                    }),
                    Err(err) => {
                        cache::discard_staging(&staging);
                        Err(err)
                    }
                }
            })
        })
        .collect();

    for handle in handles {
        let entry = handle
            .join()
            .map_err(|_| anyhow::anyhow!("A runtime install thread panicked"))??;
        provisioned.push(entry);
    }

    Ok(provisioned)
}

fn run_command(command_key: &str, locked: bool) -> Result<()> {
    let (project_dir, config) = load_project()?;
    warn_about_shadowed_keys(&config);

    // Resolve the command before installing anything, so a typo fails fast
    // rather than after a long download.
    let command = config.command(command_key)?.to_string();

    let provisioned = provision(&project_dir, &config, locked)?;
    let runtimes: Vec<cache::CachedRuntime> =
        provisioned.into_iter().map(|entry| entry.cached).collect();

    let status = executor::execute(&command, &runtimes, &project_dir)?;
    process::exit(status.code().unwrap_or(1));
}

/// Write `runx.lock` pinning what this project currently resolves to.
///
/// Runtimes are installed as part of locking. A digest can only be recorded if
/// it was actually verified, so locking without installing would mean writing
/// hashes runx never checked.
fn lock_command() -> Result<()> {
    let (project_dir, config) = load_project()?;

    if config.runtimes.is_empty() {
        return Err(
            error::UserError::new("Nothing to lock: this project declares no [runtimes].").into(),
        );
    }

    let provisioned = provision(&project_dir, &config, false)?;

    // Preserve entries for other platforms recorded by teammates.
    let mut lockfile = lock::Lockfile::load(&project_dir)?.unwrap_or_else(lock::Lockfile::new);
    lockfile.version = lock::SCHEMA_VERSION;

    for entry in &provisioned {
        // The digest comes from the install receipt, so it reflects the bytes on
        // disk. A runtime adopted from a pre-receipt install has none; the
        // version is still pinned.
        let sha256 = cache::read_receipt(&entry.cached.root).and_then(|receipt| receipt.sha256);

        lockfile.record(
            &entry.spec.tool,
            &entry.requirement,
            &entry.spec.version,
            &entry.spec.url,
            sha256.as_deref(),
        );

        match &sha256 {
            Some(_) => println!("Locked {} {}", entry.spec.tool, entry.spec.version),
            None => println!(
                "Locked {} {} (version only — no recorded checksum for this install)",
                entry.spec.tool, entry.spec.version
            ),
        }
    }

    lockfile.save(&project_dir)?;
    println!("Wrote {}", lock::lock_path(&project_dir).display());
    Ok(())
}
