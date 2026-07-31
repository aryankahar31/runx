// The library crate (lib.rs) owns all modules.  The binary just imports them.
use runx::cache;
use runx::config;
use runx::downloader;
use runx::error;
use runx::executor;
use runx::extractor;
use runx::lock;
use runx::registry;
use runx::runtime;
use runx::self_update;
use runx::version;

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

    /// Inspect and maintain the runtime cache.
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },

    /// Diagnose problems with the cache and PATH.
    Doctor,

    /// Print a shell completion script for the given shell.
    Completions {
        /// One of bash, zsh, fish, powershell, elvish.
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },

    /// Manage runx itself.
    Self_ {
        #[command(subcommand)]
        action: SelfAction,
    },

    /// Any other word is treated as a [run] command key, so `runx dev` works.
    #[command(external_subcommand)]
    External(Vec<String>),
}

#[derive(Debug, Subcommand)]
enum SelfAction {
    /// Download and install the newest runx release, verifying its checksum.
    Update,
}

#[derive(Debug, Subcommand)]
enum CacheAction {
    /// List every cached runtime with its size and age.
    List,

    /// Report total cache size on disk.
    Size,

    /// Remove every cached runtime.
    Clean {
        /// Actually delete. Without this, the runtimes that would be removed
        /// are only listed.
        #[arg(long)]
        yes: bool,
    },

    /// Remove runtimes that have not been used recently.
    Prune {
        /// Age threshold in days, measured from last use (or install).
        #[arg(long, default_value_t = 30)]
        older_than: u64,

        /// Actually delete. Without this, the runtimes that would be removed
        /// are only listed.
        #[arg(long)]
        yes: bool,
    },
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
        Some(Command::Cache { action }) => match action {
            CacheAction::List => cache_list(),
            CacheAction::Size => cache_size(),
            CacheAction::Clean { yes } => cache_clean(yes),
            CacheAction::Prune { older_than, yes } => cache_prune(older_than, yes),
        },
        Some(Command::Doctor) => doctor_command(),
        Some(Command::Completions { shell }) => completions_command(shell),
        Some(Command::Self_ { action }) => match action {
            SelfAction::Update => self_update::update(),
        },
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

/// Print a shell completion script to stdout.
///
/// Source the output from your shell profile, e.g. for bash:
/// `runx completions bash >> ~/.bashrc`.
fn completions_command(shell: clap_complete::Shell) -> Result<()> {
    use clap::CommandFactory;
    clap_complete::generate(shell, &mut Cli::command(), "runx", &mut std::io::stdout());
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

    // Turn each requirement into a concrete version, then into a spec.
    //
    // A lockfile pin is already concrete and is used verbatim — that is what
    // makes a locked install reproducible. Anything else may be a range, which
    // resolves to the newest published release satisfying it.
    let mode = registry::Resolution::from_env();
    let mut specs: Vec<(runtime::RuntimeSpec, String)> = Vec::new();
    for entry in &plan {
        let requirement = config
            .runtimes
            .get(&entry.tool)
            .cloned()
            .unwrap_or_else(|| entry.version.clone());

        let version = if entry.from_lock {
            entry.version.clone()
        } else {
            let chosen = registry::resolve_requirement(&entry.tool, &entry.version, mode)
                .with_context(|| format!("Failed to resolve {} `{}`", entry.tool, entry.version))?;

            if let Some(note) = &chosen.note {
                eprintln!("Note: {}: {note}", entry.tool);
            }
            if chosen.was_range {
                println!(
                    "Resolved {} `{}` to {}",
                    entry.tool, entry.version, chosen.version
                );
            }
            chosen.version
        };

        let spec = runtime::resolve_runtime(&entry.tool, &version)
            .with_context(|| format!("Failed to resolve runtime {} {version}", entry.tool))?;
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
                let download = downloader::download_to_temp(
                    &spec.url,
                    &spec.checksum_url,
                    spec.expected_sha256.as_deref(),
                )?;
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
    // RUNX_TIMINGS=1 mirrors mise's MISE_TIMINGS=1: opt-in per-phase timings
    // printed to stderr, used by benchmarks/shell-overhead.sh.
    let timings = env::var_os("RUNX_TIMINGS").is_some();
    let t0 = std::time::Instant::now();

    let (project_dir, config) = load_project()?;
    warn_about_shadowed_keys(&config);

    // Resolve the command before installing anything, so a typo fails fast
    // rather than after a long download.
    let command = config.command(command_key)?.to_string();
    let t1 = std::time::Instant::now();

    let provisioned = provision(&project_dir, &config, locked)?;
    let t2 = std::time::Instant::now();

    // Record use before running, so `runx cache prune` measures real activity.
    // Without this, age falls back to the install date and a runtime used every
    // day for a year would look untouched since installation — and get pruned.
    for entry in &provisioned {
        cache::touch_last_used(&entry.cached.root);
    }

    let runtimes: Vec<cache::CachedRuntime> =
        provisioned.into_iter().map(|entry| entry.cached).collect();

    if timings {
        eprintln!("runx timing: config: {:?}", t1.duration_since(t0));
        eprintln!("runx timing: cache: {:?}", t2.duration_since(t1));
    }

    let status = executor::execute(&command, &runtimes, &project_dir)?;
    process::exit(status.code().unwrap_or(1));
}

// ── Cache subcommands ────────────────────────────────────────────────────────

/// Render an age in days as something readable.
fn format_age(entry: &cache::CacheEntry, now: u64) -> String {
    match entry.age_days(now) {
        Some(0) => "today".to_string(),
        Some(1) => "1 day ago".to_string(),
        Some(days) => format!("{days} days ago"),
        // A legacy install has no receipt and so no timestamp.
        None => "unknown".to_string(),
    }
}

fn cache_list() -> Result<()> {
    let home = cache::runx_home()?;
    let entries = cache::list_cached(&home)?;
    let runtimes = cache::runtimes_dir(&home);

    if entries.is_empty() {
        println!("No cached runtimes in {}.", runtimes.display());
        return Ok(());
    }

    println!("Cached runtimes in {}:", runtimes.display());
    let now = cache::now_secs();
    for entry in &entries {
        println!(
            "  {tool:<8} {version:<12} {size:>10}  last used {age}{status}",
            tool = entry.tool,
            version = entry.version,
            size = cache::format_size(entry.size_bytes),
            age = format_age(entry, now),
            status = if entry.complete {
                ""
            } else {
                "  (incomplete — see `runx doctor`)"
            },
        );
    }

    let staging = cache::list_staging(&home)?;
    if !staging.is_empty() {
        println!(
            "\n{} incomplete download{} taking up space; run `runx cache prune` to clear.",
            staging.len(),
            if staging.len() == 1 { "" } else { "s" }
        );
    }

    Ok(())
}

fn cache_size() -> Result<()> {
    let home = cache::runx_home()?;
    let entries = cache::list_cached(&home)?;
    let runtime_bytes: u64 = entries.iter().map(|entry| entry.size_bytes).sum();

    let staging = cache::list_staging(&home)?;
    let staging_bytes: u64 = staging.iter().map(|path| cache::directory_size(path)).sum();

    println!(
        "{count} runtime{plural}: {size}",
        count = entries.len(),
        plural = if entries.len() == 1 { "" } else { "s" },
        size = cache::format_size(runtime_bytes)
    );

    if staging_bytes > 0 {
        println!(
            "{count} incomplete download{plural}: {size}",
            count = staging.len(),
            plural = if staging.len() == 1 { "" } else { "s" },
            size = cache::format_size(staging_bytes)
        );
    }

    println!(
        "Total: {} in {}",
        cache::format_size(runtime_bytes + staging_bytes),
        cache::runtimes_dir(&home).display()
    );
    Ok(())
}

/// Delete the given runtimes, reporting rather than aborting on failure.
///
/// One undeletable runtime (a file still in use, which is routine on Windows)
/// must not stop the rest from being cleaned.
fn remove_all(entries: &[cache::CacheEntry]) -> Result<()> {
    let mut failures = Vec::new();

    for entry in entries {
        match cache::remove_entry(&entry.root) {
            Ok(()) => println!(
                "Removed {} {} ({})",
                entry.tool,
                entry.version,
                cache::format_size(entry.size_bytes)
            ),
            Err(err) => failures.push(format!("  {} {}: {err:#}", entry.tool, entry.version)),
        }
    }

    if !failures.is_empty() {
        return Err(error::UserError::new(format!(
            "Failed to remove {} runtime{}:\n{}\n\
             Hint: on Windows a runtime cannot be deleted while a process is using it.",
            failures.len(),
            if failures.len() == 1 { "" } else { "s" },
            failures.join("\n")
        ))
        .into());
    }
    Ok(())
}

/// Clear stale staging directories, returning how many were removed.
fn clear_stale_staging(home: &Path) -> Result<usize> {
    let stale = cache::stale_staging(home)?;
    for path in &stale {
        cache::discard_staging(path);
    }
    Ok(stale.len())
}

/// Print what would be removed and how to confirm.
fn report_dry_run(entries: &[cache::CacheEntry], command: &str) {
    let total: u64 = entries.iter().map(|entry| entry.size_bytes).sum();
    let now = cache::now_secs();

    println!(
        "Would remove {count} runtime{plural}, freeing {size}:",
        count = entries.len(),
        plural = if entries.len() == 1 { "" } else { "s" },
        size = cache::format_size(total)
    );
    for entry in entries {
        println!(
            "  {tool:<8} {version:<12} {size:>10}  last used {age}",
            tool = entry.tool,
            version = entry.version,
            size = cache::format_size(entry.size_bytes),
            age = format_age(entry, now)
        );
    }
    println!("\nRe-run with `{command} --yes` to delete.");
}

fn cache_clean(confirmed: bool) -> Result<()> {
    let home = cache::runx_home()?;
    let entries = cache::list_cached(&home)?;

    if entries.is_empty() {
        let cleared = clear_stale_staging(&home)?;
        println!(
            "Nothing to clean.{}",
            if cleared > 0 {
                format!(" Removed {cleared} incomplete download(s).")
            } else {
                String::new()
            }
        );
        return Ok(());
    }

    // Deleting every runtime is destructive and easy to mistype, so it is a
    // dry run unless explicitly confirmed.
    if !confirmed {
        report_dry_run(&entries, "runx cache clean");
        return Ok(());
    }

    remove_all(&entries)?;
    let cleared = clear_stale_staging(&home)?;
    if cleared > 0 {
        println!("Removed {cleared} incomplete download(s).");
    }
    println!("Cache cleaned.");
    Ok(())
}

fn cache_prune(older_than_days: u64, confirmed: bool) -> Result<()> {
    let home = cache::runx_home()?;
    let now = cache::now_secs();

    let stale: Vec<cache::CacheEntry> = cache::list_cached(&home)?
        .into_iter()
        // A runtime with no timestamp at all is left alone: without evidence of
        // age, deleting it would be a guess. `runx doctor` reports these.
        .filter(|entry| {
            entry
                .age_days(now)
                .is_some_and(|age| age >= older_than_days)
        })
        .collect();

    if stale.is_empty() {
        let cleared = clear_stale_staging(&home)?;
        println!(
            "No runtimes unused for {older_than_days}+ days.{}",
            if cleared > 0 {
                format!(" Removed {cleared} incomplete download(s).")
            } else {
                String::new()
            }
        );
        return Ok(());
    }

    if !confirmed {
        report_dry_run(
            &stale,
            &format!("runx cache prune --older-than {older_than_days}"),
        );
        return Ok(());
    }

    remove_all(&stale)?;
    let cleared = clear_stale_staging(&home)?;
    if cleared > 0 {
        println!("Removed {cleared} incomplete download(s).");
    }
    Ok(())
}

/// Diagnose the cache and PATH.
///
/// Checks each cached runtime directory for a valid completion receipt or a
/// missing executable (truncated install), plus empty orphan directories,
/// abandoned staging directories, and stray files; and checks PATH for stale
/// shims pointing into the runx cache.
///
/// Exits non-zero when something needs fixing.
fn doctor_command() -> Result<()> {
    let home = cache::runx_home()?;
    let runtimes = cache::runtimes_dir(&home);
    let stale = cache::stale_staging(&home)?;
    let mut broken: Vec<String> = Vec::new();
    let mut notes: Vec<String> = Vec::new();

    println!("runx doctor — checking {}", runtimes.display());

    if !runtimes.exists() {
        println!("  ✓ no cache yet — nothing to diagnose");
    } else {
        for tool_dir in read_dir_sorted(&runtimes)? {
            let tool = file_name_of(&tool_dir);
            if !tool_dir.is_dir() {
                broken.push(format!("stray file {}", tool_dir.display()));
                continue;
            }

            let versions = read_dir_sorted(&tool_dir)?;
            let mut saw_runtime = false;
            for version_dir in versions {
                let name = file_name_of(&version_dir);
                if cache::is_staging_name(&name) {
                    if stale.contains(&version_dir) {
                        broken.push(format!(
                            "abandoned download {tool}/{name} (interrupted install)"
                        ));
                    } else {
                        notes.push(format!(
                            "download in progress or recently interrupted: {tool}/{name}"
                        ));
                    }
                    continue;
                }
                saw_runtime = true;

                let spec = runtime::resolve_runtime(&tool, &name).ok();
                if cache::is_complete(&version_dir) {
                    println!("  ✓ {tool} {name}");
                } else if spec
                    .as_ref()
                    .is_some_and(|s| cache::has_expected_executable(&version_dir, s))
                {
                    // Pre-marker install; healed on next use.
                    notes.push(format!(
                        "{tool} {name}: legacy install without a receipt; will be adopted on next use"
                    ));
                } else if read_dir_sorted(&version_dir)?.is_empty() {
                    broken.push(format!("empty orphan directory {tool}/{name}"));
                } else {
                    broken.push(format!(
                        "{tool} {name}: incomplete — missing the expected executable"
                    ));
                }
            }
            if !saw_runtime {
                notes.push(format!("no runtimes installed for {tool}"));
            }
        }
    }

    let home_canonical = fs::canonicalize(&home).unwrap_or_else(|_| home.clone());
    for (tool, shim) in runx_shims_on_path(&home_canonical) {
        broken.push(format!(
            "`{tool}` on PATH points into the runx cache ({}) — stale shim, remove it",
            shim.display()
        ));
    }

    // Show the exact PATH runx would prepend for the current project, if run
    // from one — the direct answer to "why is the wrong version running".
    if let Ok(cwd) = env::current_dir() {
        if let Some(project_dir) = config::find_project_dir(&cwd) {
            let resolved = config::load_or_detect(&project_dir)?;
            print_resolved_paths(&project_dir, &resolved.inner)?;
        }
    }

    for note in &notes {
        println!("  ~ {note}");
    }
    if broken.is_empty() {
        println!("✓ everything looks healthy");
        return Ok(());
    }

    for issue in &broken {
        println!("  ✗ {issue}");
    }
    Err(error::UserError::new(format!(
        "runx doctor found {} issue{}.\n\
         Fixes: `runx cache clean --yes` removes everything (recommended when\n\
         truncation is suspected), `runx cache prune --yes` clears abandoned\n\
         downloads, or reinstall a runtime by simply running its project command\n\
         again — a broken cache entry is replaced automatically.",
        broken.len(),
        if broken.len() == 1 { "" } else { "s" }
    ))
    .into())
}

/// Print the exact PATH runx would prepend for the current project's runtimes.
///
/// Only runtimes that are already cached are shown: doctor is diagnostic and
/// never triggers a download or a release-list lookup, so ranges without a
/// lockfile pin are skipped rather than resolved over the network.
fn print_resolved_paths(project_dir: &Path, config: &config::RunxConfig) -> Result<()> {
    let lockfile = lock::Lockfile::load(project_dir)?;
    let plan = lock::plan(&config.runtimes, lockfile.as_ref(), false)?;

    for entry in &plan {
        let requirement = config
            .runtimes
            .get(&entry.tool)
            .cloned()
            .unwrap_or_else(|| entry.version.clone());
        let version = if entry.from_lock {
            entry.version.clone()
        } else {
            requirement
        };
        if version::validate_concrete(&entry.tool, &version).is_err() {
            continue;
        }
        let Ok(spec) = runtime::resolve_runtime(&entry.tool, &version) else {
            continue;
        };
        if let Some(cached) = cache::cached_runtime(&spec)? {
            if let Some(bin) = cached.bin_dirs.first() {
                println!("Resolved PATH for `{}`: {}", entry.tool, bin.display());
            }
        }
    }
    Ok(())
}

/// Directory entries sorted by name, so doctor output is stable across runs.
fn read_dir_sorted(path: &Path) -> Result<Vec<PathBuf>> {
    let mut entries: Vec<PathBuf> = fs::read_dir(path)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    entries.sort();
    Ok(entries)
}

fn file_name_of(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_string()
}

/// Managed tool names that could be shimmed on the user's PATH.
const MANAGED_TOOLS: &[&str] = &["node", "python", "python3", "bun", "go"];

/// Find PATH entries resolving to one of the managed tools *inside* the runx
/// cache. Such a shim survives `runx cache clean` and then silently runs
/// nothing, so doctor flags it.
fn runx_shims_on_path(home: &Path) -> Vec<(String, PathBuf)> {
    let mut found = Vec::new();
    for dir in env::split_paths(&env::var_os("PATH").unwrap_or_default()) {
        if dir == Path::new("") {
            continue;
        }
        for tool in MANAGED_TOOLS {
            let candidate = dir.join(tool);
            if candidate.is_file() {
                let real = fs::canonicalize(&candidate).unwrap_or(candidate);
                if real.starts_with(home) {
                    found.push(((*tool).to_string(), real));
                }
            }
        }
    }
    found
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
