//! Process-wide output and network modes, set once from CLI flags.
//!
//! Three globals because the alternative is threading a config struct through
//! every module for values that never change during a run:
//!
//! * `--json` — machine-readable mode: command output (doctor, cache) becomes
//!   JSON on stdout, errors become one JSON object on stderr, and runx's own
//!   informational lines move to stderr so stdout stays parseable.
//! * `--quiet` — suppresses informational lines and progress bars entirely;
//!   warnings and errors still print.
//! * `--offline` — refuses any network access. Guards live at the network
//!   entry points ([`crate::downloader`], [`crate::registry`],
//!   [`crate::runtime`], [`crate::self_update`]) so cached/pinned work
//!   continues but nothing is fetched.

use anyhow::{bail, Result};
use std::sync::atomic::{AtomicBool, Ordering};

static JSON: AtomicBool = AtomicBool::new(false);
static QUIET: AtomicBool = AtomicBool::new(false);
static OFFLINE: AtomicBool = AtomicBool::new(false);
static FROZEN: AtomicBool = AtomicBool::new(false);

/// Record the CLI flags. Called once, before any command runs.
pub fn init(json: bool, quiet: bool, offline: bool) {
    init_with_frozen(json, quiet, offline, false)
}

/// Record the CLI flags including `--frozen` (which also implies offline).
pub fn init_with_frozen(json: bool, quiet: bool, offline: bool, frozen: bool) {
    JSON.store(json, Ordering::Relaxed);
    QUIET.store(quiet, Ordering::Relaxed);
    OFFLINE.store(offline || frozen, Ordering::Relaxed);
    FROZEN.store(frozen, Ordering::Relaxed);
}

pub fn json() -> bool {
    JSON.load(Ordering::Relaxed)
}

pub fn quiet() -> bool {
    QUIET.load(Ordering::Relaxed)
}

pub fn offline() -> bool {
    OFFLINE.load(Ordering::Relaxed)
}

/// True when `--frozen` was given.
pub fn frozen() -> bool {
    FROZEN.load(Ordering::Relaxed)
}

/// True when `RUNX_LOCKED` is set to a truthy value. Mirrors `--locked` for
/// every run command, so a CI setup can enforce the lockfile once.
pub fn locked_from_env() -> bool {
    std::env::var("RUNX_LOCKED")
        .map(|value| {
            !matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "" | "0" | "false" | "no"
            )
        })
        .unwrap_or(false)
}

/// Print an informational line.
///
/// Human mode: stdout, as always. JSON mode: stderr — stdout belongs to the
/// structured output. Quiet mode: dropped.
pub fn info(message: &str) {
    if json() {
        eprintln!("{message}");
    } else if !quiet() {
        println!("{message}");
    }
}

/// True when progress bars should stay hidden (non-interactive consumers).
pub fn hide_progress() -> bool {
    json() || quiet()
}

/// Refuse an operation that would touch the network under `--offline`.
pub fn ensure_network(what: &str) -> Result<()> {
    if offline() {
        bail!("--offline was given, so runx will not {what}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn info_routes_by_mode() {
        // info() writes to stdout/stderr; here we pin the routing predicate.
        init(false, false, false);
        assert!(!hide_progress());
        init(true, false, false);
        assert!(json());
        assert!(hide_progress(), "json implies no progress bars");
        init(false, true, false);
        assert!(quiet());
        assert!(hide_progress());
        init(true, true, true);
        assert!(offline());
        init(false, false, false);
    }
}
