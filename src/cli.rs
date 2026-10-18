use clap::{Parser, Subcommand};

/// Subcommand names that cannot be used as `[run]` keys, because clap resolves
/// them to built-in commands first.
///
/// A project that already defines one of these as a run command keeps working
/// via the explicit `runx run <key>` form, and `warn_about_shadowed_keys`
/// points that out rather than leaving the user to guess why `runx cache`
/// stopped running their script.
pub(crate) const RESERVED_COMMANDS: &[&str] = &[
    "init",
    "run",
    "lock",
    "cache",
    "doctor",
    "completions",
    "self",
    "install",
];

#[derive(Debug, Parser)]
#[command(
    name = "runx",
    version,
    about = "Universal project launcher with portable runtimes",
    after_help = "Run a command key from [run] in runx.toml, e.g. `runx dev`.\n\
                  Without runx.toml, package.json scripts can become command keys.\n\
                  To run an executable directly, first define a [run] key such as\n\
                  `node_version = \"node --version\"`, then run `runx node_version`.\n\n\
                  Pass arguments through after `--`, e.g. `runx dev -- --port 3000`.\n\
                  Use `runx run <key>` if a command key collides with a built-in\n\
                  subcommand name."
)]
pub(crate) struct Cli {
    /// Emit machine-readable output: command results as JSON on stdout,
    /// errors as one JSON object on stderr. runx's own progress lines move to
    /// stderr so stdout stays parseable.
    #[arg(long, global = true)]
    pub(crate) json: bool,

    /// Suppress informational output and progress bars; warnings and errors
    /// still print.
    #[arg(long, global = true)]
    pub(crate) quiet: bool,

    /// Refuse runx's network operations, not child-process networking.
    /// Cached runtimes work only when required asset metadata is also cached;
    /// downloads and online resolution fail with an explicit error.
    #[arg(long, global = true)]
    pub(crate) offline: bool,

    /// Install project dependencies before running the command.
    #[arg(long, global = true)]
    pub(crate) install: bool,

    #[command(subcommand)]
    pub(crate) command: Option<Command>,
}

#[derive(Debug, Subcommand)]
pub(crate) enum Command {
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

        /// Arguments passed through to the run command, after `--`.
        #[arg(last = true)]
        passthrough: Vec<String>,
    },

    /// Write runx.lock, pinning the exact runtimes this project resolves to.
    Lock,

    /// Inspect and maintain the runtime cache.
    Cache {
        #[command(subcommand)]
        action: CacheAction,
    },

    /// Diagnose problems with the cache and PATH.
    Doctor {
        /// Also verify each runtime's executable against the digest recorded
        /// at install time. Slower (hashes every entry-point binary) but
        /// detects at-rest corruption that the fast check cannot see.
        #[arg(long)]
        verify: bool,
    },

    /// Print a shell completion script for the given shell.
    Completions {
        /// One of bash, zsh, fish, powershell, elvish.
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },

    /// Install project dependencies (e.g. `npm ci`) using the runx-managed
    /// runtime. Does not start any command.
    Install,

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
pub(crate) enum SelfAction {
    /// Download and install the newest runx release, verifying its checksum.
    Update,
}

#[derive(Debug, Subcommand)]
pub(crate) enum CacheAction {
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
