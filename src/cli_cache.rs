//! `runx cache` subcommand implementations.
//!
//! Split out of `main.rs` so the binary's dispatch stays thin and the cache
//! presentation logic can grow its own unit tests without spawning the CLI.

use crate::cache;
use crate::error;
use crate::flags;

use anyhow::Result;
use std::path::Path;

/// Render an age in days as something readable.
pub fn format_age(entry: &cache::CacheEntry, now: u64) -> String {
    match entry.age_days(now) {
        Some(0) => "today".to_string(),
        Some(1) => "1 day ago".to_string(),
        Some(days) => format!("{days} days ago"),
        // A legacy install has no receipt and so no timestamp.
        None => "unknown".to_string(),
    }
}

pub fn cache_list() -> Result<()> {
    let home = cache::runx_home()?;
    let entries = cache::list_cached(&home)?;
    let runtimes = cache::runtimes_dir(&home);
    let staging = cache::list_staging(&home)?;

    if flags::json() {
        let now = cache::now_secs();
        let payload = serde_json::json!({
            "runtimes": entries.iter().map(|entry| serde_json::json!({
                "tool": entry.tool,
                "version": entry.version,
                "size_bytes": entry.size_bytes,
                "complete": entry.complete,
                "last_used_secs": entry.last_used_secs,
                "age_days": entry.age_days(now),
            })).collect::<Vec<_>>(),
            "incomplete_downloads": staging.len(),
            "cache_dir": runtimes.display().to_string(),
        });
        println!("{payload}");
        return Ok(());
    }

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

    if !staging.is_empty() {
        println!(
            "\n{} incomplete download{} taking up space; run `runx cache prune` to clear.",
            staging.len(),
            if staging.len() == 1 { "" } else { "s" }
        );
    }

    Ok(())
}

pub fn cache_size() -> Result<()> {
    let home = cache::runx_home()?;
    let entries = cache::list_cached(&home)?;
    let runtime_bytes: u64 = entries.iter().map(|entry| entry.size_bytes).sum();

    let staging = cache::list_staging(&home)?;
    let staging_bytes: u64 = staging.iter().map(|path| cache::directory_size(path)).sum();

    if flags::json() {
        let payload = serde_json::json!({
            "runtime_count": entries.len(),
            "runtime_bytes": runtime_bytes,
            "staging_count": staging.len(),
            "staging_bytes": staging_bytes,
            "total_bytes": runtime_bytes + staging_bytes,
        });
        println!("{payload}");
        return Ok(());
    }

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
            Ok(()) => flags::info(&format!(
                "Removed {} {} ({})",
                entry.tool,
                entry.version,
                cache::format_size(entry.size_bytes)
            )),
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

pub fn cache_clean(confirmed: bool) -> Result<()> {
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

pub fn cache_prune(older_than_days: u64, confirmed: bool) -> Result<()> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn entry_with_receipt(installed_at_secs: Option<u64>) -> cache::CacheEntry {
        cache::CacheEntry {
            tool: "node".into(),
            version: "20.11.0".into(),
            root: PathBuf::from("/tmp/fake"),
            size_bytes: 0,
            complete: true,
            installed_at_secs,
            last_used_secs: None,
        }
    }

    #[test]
    fn format_age_renders_today_and_day_counts() {
        let now = 1_000_000u64;
        // installed_at == now → 0 days.
        assert_eq!(format_age(&entry_with_receipt(Some(now)), now), "today");
        // 86_400s earlier → 1 day.
        assert_eq!(
            format_age(&entry_with_receipt(Some(now - 86_400)), now),
            "1 day ago"
        );
        // 10 days earlier.
        assert_eq!(
            format_age(&entry_with_receipt(Some(now - 10 * 86_400)), now),
            "10 days ago"
        );
    }

    #[test]
    fn format_age_unknown_for_legacy_receiptless_entry() {
        assert_eq!(format_age(&entry_with_receipt(None), 1_000_000), "unknown");
    }
}
