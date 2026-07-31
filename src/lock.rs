//! `runx.lock` — reproducible runtime resolution across machines and CI.
//!
//! # Why this file is shaped the way it is
//!
//! Runtime archives are **platform-specific**. Node 20.11.0 on macOS/arm64 is
//! `node-v20.11.0-darwin-arm64.tar.gz`; on Linux/x64 it is
//! `node-v20.11.0-linux-x64.tar.xz`, a different artifact with a different
//! SHA-256. A lockfile that recorded one flat `{version, url, sha256}` would
//! therefore break the moment it was committed and used on another OS — the
//! exact scenario it exists to support.
//!
//! So entries are keyed `(tool, version) -> platform -> {url, sha256}`. The
//! *version* pin is cross-platform and is the main reproducibility win; the
//! digest is a per-platform integrity check layered on top.
//!
//! # Precedence
//!
//! `runx.toml` is the source of truth for *what* is wanted; the lockfile
//! records *exactly which bytes* satisfied it. If the two disagree — someone
//! bumped a version in `runx.toml` without re-locking — the config wins and the
//! lock entry is reported stale. A lockfile that silently overrode an explicit
//! version bump would be baffling to debug.
//!
//! # Partial locks are normal
//!
//! A lock generated on macOS has no Linux artifact. On Linux, runx installs
//! normally and records the new platform rather than failing, because failing
//! would make the lockfile unusable for any mixed-OS team. CI that wants
//! strictness passes `--locked`, which turns a missing entry into an error
//! (mirroring `cargo build --locked`).
//!
//! The file is only ever created by `runx lock`. Its absence changes nothing,
//! preserving the opt-in-by-absence guarantee.

use crate::error::UserError;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

/// Name of the lockfile, alongside `runx.toml` in the project root.
pub const LOCK_FILE: &str = "runx.lock";

/// Current schema version. Bumped only on an incompatible layout change; an
/// unknown version is reported rather than silently misread.
pub const SCHEMA_VERSION: u32 = 1;

/// One platform's artifact for a locked runtime.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    /// Exact download URL used on this platform.
    pub url: String,
    /// SHA-256 of the archive, lowercase hex.
    pub sha256: String,
}

/// A locked runtime: one version, plus the artifacts seen per platform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockedRuntime {
    /// The resolved concrete version, e.g. `20.11.0`.
    pub version: String,
    /// The requirement this version was resolved from, e.g. `>=20`.
    ///
    /// Stored so a changed requirement can be detected. Without it, editing
    /// `runx.toml` from `>=20` to `>=22` would leave the old pin looking valid.
    #[serde(default)]
    pub requirement: String,
    /// Per-platform artifacts, keyed by [`current_platform`].
    #[serde(default)]
    pub artifacts: BTreeMap<String, Artifact>,
}

/// The parsed `runx.lock`.
///
/// `BTreeMap` throughout keeps serialization order deterministic, so a
/// committed lockfile produces stable diffs.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Lockfile {
    /// Schema version.
    #[serde(default)]
    pub version: u32,
    /// Locked runtimes keyed by tool name.
    #[serde(default)]
    pub runtimes: BTreeMap<String, LockedRuntime>,
}

/// Why a lock entry could not be used as-is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Staleness {
    /// No entry for this tool.
    Missing,
    /// The requirement in `runx.toml` differs from the one that was locked.
    RequirementChanged { locked: String, current: String },
    /// The requirement matches, but no artifact is recorded for this platform.
    /// Expected when a teammate locked on a different OS.
    PlatformMissing { platform: String },
}

/// Stable identifier for the current OS/architecture pair.
///
/// Uses Rust's built-in constants so it cannot drift from the values
/// `runtime::resolve_*` matches on when building URLs.
pub fn current_platform() -> String {
    format!("{}-{}", env::consts::OS, env::consts::ARCH)
}

/// Path to the lockfile within `dir`.
pub fn lock_path(dir: &Path) -> PathBuf {
    dir.join(LOCK_FILE)
}

impl Lockfile {
    /// An empty lockfile at the current schema version.
    pub fn new() -> Self {
        Self {
            version: SCHEMA_VERSION,
            runtimes: BTreeMap::new(),
        }
    }

    /// Load `runx.lock` from `dir`.
    ///
    /// Returns `Ok(None)` when absent — that is the normal, unlocked case, not
    /// an error. A malformed or future-versioned file *is* an error, because
    /// silently ignoring it would mean silently losing the guarantee the user
    /// added the file to obtain.
    pub fn load(dir: &Path) -> Result<Option<Self>> {
        let path = lock_path(dir);
        let raw = match fs::read_to_string(&path) {
            Ok(raw) => raw,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(err) => {
                return Err(err).with_context(|| format!("Failed to read {}", path.display()))
            }
        };

        let lock: Self =
            toml::from_str(&raw).with_context(|| format!("Failed to parse {}", path.display()))?;

        if lock.version > SCHEMA_VERSION {
            return Err(UserError::new(format!(
                "{} was written by a newer runx (lock schema v{}, this runx supports v{}).\n\
                 Hint: upgrade runx, or delete the lockfile to regenerate it.",
                path.display(),
                lock.version,
                SCHEMA_VERSION
            ))
            .into());
        }

        Ok(Some(lock))
    }

    /// Write the lockfile into `dir`.
    pub fn save(&self, dir: &Path) -> Result<()> {
        let path = lock_path(dir);
        let body = toml::to_string_pretty(self).context("Failed to serialize the lockfile")?;
        let contents = format!(
            "# Generated by runx. Commit this file to pin runtime versions.\n\
             # Run `runx lock` after changing [runtimes] in runx.toml.\n\n{body}"
        );
        fs::write(&path, contents).with_context(|| format!("Failed to write {}", path.display()))
    }

    /// Look up the locked entry for `tool`.
    pub fn get(&self, tool: &str) -> Option<&LockedRuntime> {
        self.runtimes.get(tool)
    }

    /// Record a resolved runtime for the current platform.
    ///
    /// Artifacts for *other* platforms are preserved, so locking on macOS does
    /// not discard a teammate's Linux entry. If the version changed, stale
    /// artifacts for the old version are dropped, since their digests no longer
    /// describe this version.
    /// `sha256` is optional: a runtime adopted from a pre-receipt install has no
    /// recorded digest. In that case the *version* is still pinned (the main
    /// cross-platform guarantee) and only the per-platform integrity entry is
    /// omitted, rather than refusing to lock at all.
    pub fn record(
        &mut self,
        tool: &str,
        requirement: &str,
        version: &str,
        url: &str,
        sha256: Option<&str>,
    ) {
        let entry = self
            .runtimes
            .entry(tool.to_string())
            .or_insert_with(|| LockedRuntime {
                version: version.to_string(),
                requirement: requirement.to_string(),
                artifacts: BTreeMap::new(),
            });

        if entry.version != version {
            entry.artifacts.clear();
            entry.version = version.to_string();
        }
        entry.requirement = requirement.to_string();

        // Without a digest there is nothing to record for this platform beyond
        // the version, which is already pinned above.
        if let Some(sha256) = sha256 {
            entry.artifacts.insert(
                current_platform(),
                Artifact {
                    url: url.to_string(),
                    sha256: sha256.to_ascii_lowercase(),
                },
            );
        }
    }

    /// Resolve `tool` against this lockfile for the current platform.
    ///
    /// `Ok(artifact)` means the lock fully determines the install. `Err(reason)`
    /// explains what to fall back to.
    pub fn resolve(
        &self,
        tool: &str,
        requirement: &str,
    ) -> std::result::Result<(&str, &Artifact), Staleness> {
        let Some(entry) = self.runtimes.get(tool) else {
            return Err(Staleness::Missing);
        };

        // An empty locked requirement means the file predates requirement
        // tracking; accept it rather than declaring every old lock stale.
        if !entry.requirement.is_empty() && entry.requirement != requirement {
            return Err(Staleness::RequirementChanged {
                locked: entry.requirement.clone(),
                current: requirement.to_string(),
            });
        }

        let platform = current_platform();
        match entry.artifacts.get(&platform) {
            Some(artifact) => Ok((entry.version.as_str(), artifact)),
            None => Err(Staleness::PlatformMissing { platform }),
        }
    }

    /// The version pinned for `tool`, regardless of platform coverage.
    ///
    /// This is what makes a cross-platform lock useful: even without an
    /// artifact for this OS, the *version* is still pinned, so a teammate on
    /// another platform installs the same release.
    pub fn pinned_version(&self, tool: &str, requirement: &str) -> Option<&str> {
        let entry = self.runtimes.get(tool)?;
        if !entry.requirement.is_empty() && entry.requirement != requirement {
            return None;
        }
        Some(entry.version.as_str())
    }
}

/// One runtime to provision, after consulting the lockfile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    pub tool: String,
    /// The concrete version to install.
    pub version: String,
    /// True when `version` came from the lockfile rather than the config.
    pub from_lock: bool,
    /// A note to show the user, when the lock could not be used as-is.
    pub note: Option<String>,
}

/// Decide what to install for each configured runtime.
///
/// `requirements` maps tool to the requirement written in `runx.toml` (or
/// detected). When the lockfile pins a version for an unchanged requirement,
/// that pin wins; otherwise the requirement is used directly.
///
/// # `--locked`
///
/// Under `locked`, a missing tool or a changed requirement is an error: the
/// lockfile genuinely does not describe what the config now asks for. A missing
/// *platform artifact* is **not** an error, because the version pin — the actual
/// cross-platform reproducibility guarantee — still holds, and the download is
/// verified against the publisher's own checksum document regardless. Failing
/// there would make `--locked` unusable for any team spanning two operating
/// systems, which is most of them.
pub fn plan(
    requirements: &BTreeMap<String, String>,
    lockfile: Option<&Lockfile>,
    locked: bool,
) -> Result<Vec<Planned>> {
    let mut planned = Vec::new();

    for (tool, requirement) in requirements {
        let Some(lockfile) = lockfile else {
            if locked {
                return Err(UserError::new(format!(
                    "--locked was given but no {LOCK_FILE} is present.\n\
                     Hint: run `runx lock` and commit the result."
                ))
                .into());
            }
            planned.push(Planned {
                tool: tool.clone(),
                version: requirement.clone(),
                from_lock: false,
                note: None,
            });
            continue;
        };

        match lockfile.resolve(tool, requirement) {
            Ok((version, _artifact)) => planned.push(Planned {
                tool: tool.clone(),
                version: version.to_string(),
                from_lock: true,
                note: None,
            }),

            // Version still pinned; only this platform's digest is absent.
            Err(Staleness::PlatformMissing { platform }) => {
                let version = lockfile
                    .pinned_version(tool, requirement)
                    .unwrap_or(requirement)
                    .to_string();
                planned.push(Planned {
                    tool: tool.clone(),
                    version,
                    from_lock: true,
                    note: Some(format!(
                        "{tool}: {LOCK_FILE} has no entry for {platform}; \
                         using the pinned version and verifying upstream checksums. \
                         Run `runx lock` to record it."
                    )),
                });
            }

            Err(Staleness::Missing) => {
                if locked {
                    return Err(UserError::new(format!(
                        "--locked was given but {LOCK_FILE} has no entry for `{tool}`.\n\
                         Hint: run `runx lock` to update it."
                    ))
                    .into());
                }
                planned.push(Planned {
                    tool: tool.clone(),
                    version: requirement.clone(),
                    from_lock: false,
                    note: Some(format!(
                        "{tool}: not in {LOCK_FILE}; run `runx lock` to pin it."
                    )),
                });
            }

            Err(Staleness::RequirementChanged {
                locked: was,
                current,
            }) => {
                if locked {
                    return Err(UserError::new(format!(
                        "--locked was given but {LOCK_FILE} is out of date for `{tool}`: \
                         it pins `{was}` while runx.toml asks for `{current}`.\n\
                         Hint: run `runx lock` to update it."
                    ))
                    .into());
                }
                planned.push(Planned {
                    tool: tool.clone(),
                    version: requirement.clone(),
                    from_lock: false,
                    note: Some(format!(
                        "{tool}: {LOCK_FILE} pins `{was}` but runx.toml asks for \
                         `{current}`; using runx.toml. Run `runx lock` to update."
                    )),
                });
            }
        }
    }

    Ok(planned)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> tempfile::TempDir {
        tempfile::tempdir().expect("create temp dir")
    }

    const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OTHER_DIGEST: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    fn locked() -> Lockfile {
        let mut lock = Lockfile::new();
        lock.record(
            "node",
            "20.11.0",
            "20.11.0",
            "https://nodejs.org/dist/v20.11.0/node.tar.gz",
            Some(DIGEST),
        );
        lock
    }

    // ── Round-trip ───────────────────────────────────────────────────────────

    #[test]
    fn absent_lockfile_is_not_an_error() {
        let dir = tmp();
        assert_eq!(Lockfile::load(dir.path()).expect("should succeed"), None);
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = tmp();
        let lock = locked();
        lock.save(dir.path()).expect("save");

        let loaded = Lockfile::load(dir.path())
            .expect("load should succeed")
            .expect("lockfile should be present");

        assert_eq!(loaded, lock);
        assert_eq!(loaded.version, SCHEMA_VERSION);
    }

    #[test]
    fn saved_file_is_commentable_toml() {
        let dir = tmp();
        locked().save(dir.path()).expect("save");

        let raw = fs::read_to_string(lock_path(dir.path())).expect("read");
        assert!(
            raw.starts_with('#'),
            "should carry a header comment, got:\n{raw}"
        );
        assert!(
            raw.contains("runx lock"),
            "header should mention regenerating"
        );
        assert!(raw.contains("20.11.0"));
    }

    #[test]
    fn malformed_lockfile_is_an_error_not_ignored() {
        let dir = tmp();
        fs::write(lock_path(dir.path()), "this is not : valid toml [[[").unwrap();

        assert!(
            Lockfile::load(dir.path()).is_err(),
            "a corrupt lockfile must be reported, not silently ignored"
        );
    }

    #[test]
    fn future_schema_version_is_rejected_with_a_hint() {
        let dir = tmp();
        fs::write(
            lock_path(dir.path()),
            format!("version = {}\n", SCHEMA_VERSION + 1),
        )
        .unwrap();

        let err = Lockfile::load(dir.path()).expect_err("should reject");
        let message = format!("{err:#}");
        assert!(
            message.contains("newer runx"),
            "should explain the version gap, got: {message}"
        );
    }

    // ── Resolution ───────────────────────────────────────────────────────────

    #[test]
    fn resolves_a_matching_entry_for_this_platform() {
        let lock = locked();
        let (version, artifact) = lock.resolve("node", "20.11.0").expect("should resolve");

        assert_eq!(version, "20.11.0");
        assert_eq!(artifact.sha256, DIGEST);
    }

    #[test]
    fn reports_missing_tool() {
        assert_eq!(
            locked().resolve("python", "3.11.7"),
            Err(Staleness::Missing)
        );
    }

    /// Bumping a version in runx.toml must invalidate the pin, or the lock would
    /// silently keep installing the old release.
    #[test]
    fn reports_a_changed_requirement() {
        let lock = locked();
        match lock.resolve("node", "22.0.0") {
            Err(Staleness::RequirementChanged { locked, current }) => {
                assert_eq!(locked, "20.11.0");
                assert_eq!(current, "22.0.0");
            }
            other => panic!("expected RequirementChanged, got {other:?}"),
        }
    }

    /// The cross-platform case that dictates the schema: a lock written on
    /// another OS has no artifact here, but the version is still pinned.
    #[test]
    fn foreign_platform_lock_still_pins_the_version() {
        let mut lock = Lockfile::new();
        lock.runtimes.insert(
            "node".to_string(),
            LockedRuntime {
                version: "20.11.0".to_string(),
                requirement: ">=20".to_string(),
                artifacts: BTreeMap::from([(
                    // A platform that is definitely not the one running tests.
                    "plan9-sparc".to_string(),
                    Artifact {
                        url: "https://example.invalid/node.tar.gz".to_string(),
                        sha256: DIGEST.to_string(),
                    },
                )]),
            },
        );

        match lock.resolve("node", ">=20") {
            Err(Staleness::PlatformMissing { platform }) => {
                assert_eq!(platform, current_platform());
            }
            other => panic!("expected PlatformMissing, got {other:?}"),
        }

        assert_eq!(
            lock.pinned_version("node", ">=20"),
            Some("20.11.0"),
            "the version pin must survive across platforms — this is the point of the lockfile"
        );
    }

    #[test]
    fn pinned_version_is_dropped_when_the_requirement_changes() {
        assert_eq!(locked().pinned_version("node", "22.0.0"), None);
        assert_eq!(locked().pinned_version("node", "20.11.0"), Some("20.11.0"));
    }

    // ── Recording ────────────────────────────────────────────────────────────

    /// Locking on one OS must not discard a teammate's entry for another.
    #[test]
    fn recording_preserves_other_platforms() {
        let mut lock = Lockfile::new();
        lock.runtimes.insert(
            "node".to_string(),
            LockedRuntime {
                version: "20.11.0".to_string(),
                requirement: ">=20".to_string(),
                artifacts: BTreeMap::from([(
                    "plan9-sparc".to_string(),
                    Artifact {
                        url: "https://example.invalid/foreign.tar.gz".to_string(),
                        sha256: OTHER_DIGEST.to_string(),
                    },
                )]),
            },
        );

        lock.record(
            "node",
            ">=20",
            "20.11.0",
            "https://example.invalid/local.tar.gz",
            Some(DIGEST),
        );

        let entry = lock.get("node").expect("entry");
        assert_eq!(entry.artifacts.len(), 2, "both platforms should be present");
        assert_eq!(
            entry.artifacts["plan9-sparc"].sha256, OTHER_DIGEST,
            "the foreign artifact must be untouched"
        );
        assert_eq!(entry.artifacts[&current_platform()].sha256, DIGEST);
    }

    /// A version change makes every recorded digest obsolete, since they
    /// describe the old release.
    #[test]
    fn recording_a_new_version_clears_stale_artifacts() {
        let mut lock = Lockfile::new();
        lock.record("node", ">=20", "20.11.0", "https://a.invalid", Some(DIGEST));
        lock.runtimes.get_mut("node").unwrap().artifacts.insert(
            "plan9-sparc".to_string(),
            Artifact {
                url: "https://old.invalid".to_string(),
                sha256: OTHER_DIGEST.to_string(),
            },
        );

        lock.record("node", ">=20", "22.1.0", "https://b.invalid", Some(DIGEST));

        let entry = lock.get("node").expect("entry");
        assert_eq!(entry.version, "22.1.0");
        assert_eq!(
            entry.artifacts.len(),
            1,
            "digests for the previous version must not survive"
        );
        assert!(entry.artifacts.contains_key(&current_platform()));
    }

    #[test]
    fn recording_updates_the_requirement() {
        let mut lock = locked();
        lock.record("node", ">=20", "20.11.0", "https://a.invalid", Some(DIGEST));
        assert_eq!(lock.get("node").unwrap().requirement, ">=20");
    }

    #[test]
    fn digests_are_normalised_to_lowercase() {
        let mut lock = Lockfile::new();
        lock.record(
            "node",
            "20.11.0",
            "20.11.0",
            "https://a.invalid",
            Some(&DIGEST.to_uppercase()),
        );
        assert_eq!(
            lock.get("node").unwrap().artifacts[&current_platform()].sha256,
            DIGEST
        );
    }

    // ── Format stability ─────────────────────────────────────────────────────

    /// A committed lockfile must produce stable diffs, so serialization order
    /// cannot depend on hash iteration order.
    #[test]
    fn serialization_is_deterministic() {
        let mut first = Lockfile::new();
        first.record(
            "python",
            "3.11.7",
            "3.11.7",
            "https://p.invalid",
            Some(DIGEST),
        );
        first.record(
            "node",
            "20.11.0",
            "20.11.0",
            "https://n.invalid",
            Some(DIGEST),
        );

        let mut second = Lockfile::new();
        second.record(
            "node",
            "20.11.0",
            "20.11.0",
            "https://n.invalid",
            Some(DIGEST),
        );
        second.record(
            "python",
            "3.11.7",
            "3.11.7",
            "https://p.invalid",
            Some(DIGEST),
        );

        assert_eq!(
            toml::to_string_pretty(&first).unwrap(),
            toml::to_string_pretty(&second).unwrap(),
            "insertion order must not affect the file"
        );
    }

    /// Older locks without a `requirement` field must keep working.
    #[test]
    fn tolerates_entries_written_without_a_requirement() {
        let raw = format!(
            "version = 1\n\n\
             [runtimes.node]\n\
             version = \"20.11.0\"\n\n\
             [runtimes.node.artifacts.{platform}]\n\
             url = \"https://example.invalid/node.tar.gz\"\n\
             sha256 = \"{DIGEST}\"\n",
            platform = current_platform()
        );

        let lock: Lockfile = toml::from_str(&raw).expect("should parse");
        assert!(
            lock.resolve("node", "anything").is_ok(),
            "a requirement-less entry should not be treated as stale"
        );
    }

    #[test]
    fn platform_key_is_stable_and_specific() {
        let platform = current_platform();
        assert!(platform.contains('-'), "expected OS-ARCH, got {platform}");
        assert!(platform.contains(env::consts::OS));
        assert!(platform.contains(env::consts::ARCH));
    }

    // ── Planning ─────────────────────────────────────────────────────────────

    fn requirements(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(tool, req)| (tool.to_string(), req.to_string()))
            .collect()
    }

    #[test]
    fn plan_without_a_lockfile_uses_the_config() {
        let plan = plan(&requirements(&[("node", "20.11.0")]), None, false).expect("plan");
        assert_eq!(plan.len(), 1);
        assert_eq!(plan[0].version, "20.11.0");
        assert!(!plan[0].from_lock);
        assert!(plan[0].note.is_none());
    }

    #[test]
    fn plan_prefers_the_locked_version() {
        let lock = locked();
        let plan = plan(&requirements(&[("node", "20.11.0")]), Some(&lock), false).expect("plan");
        assert_eq!(plan[0].version, "20.11.0");
        assert!(plan[0].from_lock, "should come from the lockfile");
    }

    /// runx.toml is the source of truth for *what* is wanted. A stale pin must
    /// not silently override an explicit version bump.
    #[test]
    fn config_wins_over_a_stale_pin_and_says_so() {
        let lock = locked();
        let plan = plan(&requirements(&[("node", "22.1.0")]), Some(&lock), false).expect("plan");

        assert_eq!(plan[0].version, "22.1.0", "runx.toml must win");
        assert!(!plan[0].from_lock);
        let note = plan[0].note.as_deref().unwrap_or_default();
        assert!(
            note.contains("runx lock"),
            "should tell the user how to fix it: {note}"
        );
    }

    #[test]
    fn missing_tool_falls_back_with_a_note() {
        let lock = locked();
        let plan = plan(&requirements(&[("python", "3.11.7")]), Some(&lock), false).expect("plan");
        assert_eq!(plan[0].version, "3.11.7");
        assert!(!plan[0].from_lock);
        assert!(plan[0].note.is_some());
    }

    /// The cross-OS case: no artifact for this platform, but the version pin
    /// still applies, so the install stays reproducible.
    #[test]
    fn foreign_platform_still_uses_the_pinned_version() {
        let mut lock = Lockfile::new();
        lock.runtimes.insert(
            "node".to_string(),
            LockedRuntime {
                version: "20.11.0".to_string(),
                requirement: ">=20".to_string(),
                artifacts: BTreeMap::from([(
                    "plan9-sparc".to_string(),
                    Artifact {
                        url: "https://example.invalid/n.tar.gz".to_string(),
                        sha256: DIGEST.to_string(),
                    },
                )]),
            },
        );

        let plan = plan(&requirements(&[("node", ">=20")]), Some(&lock), false).expect("plan");
        assert_eq!(
            plan[0].version, "20.11.0",
            "the pinned version must be used even without a local artifact"
        );
        assert!(plan[0].from_lock);
        assert!(
            plan[0].note.is_some(),
            "should mention the missing platform"
        );
    }

    // ── --locked strictness ──────────────────────────────────────────────────

    #[test]
    fn locked_requires_a_lockfile() {
        let err = plan(&requirements(&[("node", "20.11.0")]), None, true)
            .expect_err("--locked without a lockfile should fail");
        assert!(format!("{err:#}").contains("runx lock"));
    }

    #[test]
    fn locked_rejects_a_missing_tool() {
        let lock = locked();
        assert!(
            plan(&requirements(&[("python", "3.11.7")]), Some(&lock), true).is_err(),
            "--locked must fail when a tool is absent from the lockfile"
        );
    }

    #[test]
    fn locked_rejects_a_changed_requirement() {
        let lock = locked();
        let err = plan(&requirements(&[("node", "22.1.0")]), Some(&lock), true)
            .expect_err("--locked must fail on a stale pin");
        assert!(format!("{err:#}").contains("out of date"));
    }

    /// A teammate on another OS must not break `--locked` in CI: the version is
    /// still pinned and the download is still checksum-verified upstream.
    #[test]
    fn locked_tolerates_a_missing_platform_artifact() {
        let mut lock = Lockfile::new();
        lock.runtimes.insert(
            "node".to_string(),
            LockedRuntime {
                version: "20.11.0".to_string(),
                requirement: "20.11.0".to_string(),
                artifacts: BTreeMap::from([(
                    "plan9-sparc".to_string(),
                    Artifact {
                        url: "https://example.invalid/n.tar.gz".to_string(),
                        sha256: DIGEST.to_string(),
                    },
                )]),
            },
        );

        let plan = plan(&requirements(&[("node", "20.11.0")]), Some(&lock), true)
            .expect("--locked should tolerate a foreign-platform lock");
        assert_eq!(plan[0].version, "20.11.0");
    }

    #[test]
    fn plan_covers_every_configured_runtime() {
        let lock = locked();
        let plan = plan(
            &requirements(&[("node", "20.11.0"), ("python", "3.11.7")]),
            Some(&lock),
            false,
        )
        .expect("plan");
        assert_eq!(plan.len(), 2);
        let tools: Vec<&str> = plan.iter().map(|p| p.tool.as_str()).collect();
        assert!(tools.contains(&"node") && tools.contains(&"python"));
    }
}
