//! Project readiness: what is prepared, what is missing, and how to fix it.
//!
//! The UX report (RX-UX-01/02/03/06) asked three different commands the same
//! question — "is this project ready to run?" — and got three different
//! answers. `runx install` reported a package manager's raw failure, a run
//! printed a bare hint after the fact, and `runx doctor` did not look at
//! dependencies at all.
//!
//! This module is the single answer. Each check reports a [`Status`], the
//! condition actually detected, and the exact command that fixes it, so every
//! caller renders the same wording.
//!
//! Everything here is read-only and offline: it inspects the filesystem and
//! the runtime cache, never the network, so it is safe to call before an
//! `--offline` run.

use crate::{cache, config, dep, lock, runtime, version};
use std::path::{Path, PathBuf};

/// Outcome of a single readiness check.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    /// Ready to use as-is.
    Pass,
    /// Usable, but something is worth saying.
    Warn,
    /// Not usable; the run will fail until it is fixed.
    Fail,
}

impl Status {
    /// The glyph doctor prints for this status.
    ///
    /// Matches the existing doctor vocabulary: `✓` healthy, `~` note, `✗`
    /// broken, so the new section does not introduce a second visual language.
    pub fn glyph(self) -> &'static str {
        match self {
            Status::Pass => "✓",
            Status::Warn => "~",
            Status::Fail => "✗",
        }
    }
}

/// One readiness check: what was examined, what was found, and the fix.
#[derive(Debug, Clone)]
pub struct Check {
    /// What was examined, e.g. `runtime node 20.11.0` or `dependencies (npm)`.
    pub subject: String,
    pub status: Status,
    /// The condition actually detected. The report asked for the *exact*
    /// condition rather than a generic "not ready".
    pub detail: String,
    /// The command that resolves a `Warn`/`Fail`, when one exists.
    pub fix: Option<String>,
}

impl Check {
    fn pass(subject: impl Into<String>, detail: impl Into<String>) -> Self {
        Self {
            subject: subject.into(),
            status: Status::Pass,
            detail: detail.into(),
            fix: None,
        }
    }

    fn warn(subject: impl Into<String>, detail: impl Into<String>, fix: &str) -> Self {
        Self {
            subject: subject.into(),
            status: Status::Warn,
            detail: detail.into(),
            fix: Some(fix.to_string()),
        }
    }

    fn fail(subject: impl Into<String>, detail: impl Into<String>, fix: &str) -> Self {
        Self {
            subject: subject.into(),
            status: Status::Fail,
            detail: detail.into(),
            fix: Some(fix.to_string()),
        }
    }

    /// `✓ subject: detail`, plus `Fix: …` on a second line indented to sit
    /// under the subject rather than under the glyph.
    pub fn render(&self) -> String {
        let mut out = format!("{} {}: {}", self.status.glyph(), self.subject, self.detail);
        if let Some(fix) = &self.fix {
            out.push_str(&format!("\n  Fix: {fix}"));
        }
        out
    }
}

/// The dependency state of one detected manager.
///
/// RX-UX-01 asked for these four to be distinguishable. They are mutually
/// exclusive and derived from the filesystem alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DepState {
    /// Installed and no newer lockfile edit since.
    Installed,
    /// Installed, but the lockfile requires packages the tree does not have.
    OutOfSync,
    /// Never installed: no `node_modules` / `.venv` / `go.sum`.
    Missing,
}

/// Classify the dependency state for one detected manager.
///
/// `OutOfSync` requires *evidence* that the installed tree does not satisfy the
/// lockfile, never an mtime comparison. mtimes are not a record of what was
/// installed: `cp -R`, `git checkout`, a restored backup or an unpacked archive
/// all reorder them, and `cp -R` in particular writes `node_modules` a few
/// hundred microseconds before `package-lock.json`, which made a freshly copied
/// project report "the lockfile changed since the last install".
///
/// `Missing` still defers to [`dep::deps_present`], and the install decision
/// still belongs to [`dep::deps_installed`] — this function only classifies
/// what to tell the user, so a report can never trigger an install.
pub fn dep_state(project_dir: &Path, manager: &dep::DepManager) -> DepState {
    if !dep::deps_present(project_dir, manager) {
        return DepState::Missing;
    }
    match lockfile_satisfied(project_dir, manager) {
        // Proven incomplete: something the lockfile requires is not installed.
        Some(false) => DepState::OutOfSync,
        // Satisfied, or no evidence either way. Both report `Installed`: a
        // warning no user can act on is worse than silence, and a real
        // mismatch surfaces as the package manager's own error on the next run.
        _ => DepState::Installed,
    }
}

/// Whether the installed tree satisfies the lockfile, when that is knowable.
///
/// `None` means "no evidence available" — an unsupported manager, or metadata
/// that cannot be read or parsed. Only npm-family projects record enough to
/// answer: npm, pnpm and yarn write `node_modules/.package-lock.json`, a
/// mirror of what was actually installed.
fn lockfile_satisfied(project_dir: &Path, manager: &dep::DepManager) -> Option<bool> {
    match manager {
        dep::DepManager::Npm { lockfile }
        | dep::DepManager::Yarn { lockfile }
        | dep::DepManager::Pnpm { lockfile } => {
            let installed = installed_package_paths(
                &project_dir.join("node_modules").join(".package-lock.json"),
            )?;
            let locked = installed_package_paths(lockfile)?;
            // Every package the lockfile names must be present in the tree.
            // The reverse (extra installed packages) is normal: a lockfile
            // edit that *removes* a dependency leaves it on disk until the
            // next install, which does not stop the project from running.
            Some(locked.is_subset(&installed))
        }
        // Bun, Deno, Go and pip record no comparable manifest of what they
        // installed, so there is nothing to compare and no warning to give.
        _ => None,
    }
}

/// The `packages` keys of an npm-style lockfile, excluding the root entry.
fn installed_package_paths(path: &Path) -> Option<std::collections::BTreeSet<String>> {
    let raw = std::fs::read_to_string(path).ok()?;
    let parsed: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let packages = parsed.get("packages")?.as_object()?;
    Some(
        packages
            .keys()
            // "" is the project itself, present only in the real lockfile.
            .filter(|key| !key.is_empty())
            .cloned()
            .collect(),
    )
}

/// The package manager's own command that repairs this project.
///
/// Never `runx install`: this is used where `runx install` has just failed or
/// cannot help, so recommending it again would loop. For npm the distinction
/// matters — runx runs `npm ci`, which *refuses* an out-of-sync lockfile by
/// design, and only `npm install` rewrites it. That is the command the tester
/// in RX-UX-01 had to discover by hand.
fn repair_command(manager: &dep::DepManager) -> &'static str {
    match manager {
        dep::DepManager::Npm { .. } => "npm install",
        dep::DepManager::Yarn { .. } => "yarn install",
        dep::DepManager::Pnpm { .. } => "pnpm install",
        dep::DepManager::Bun { .. } => "bun install",
        dep::DepManager::PythonPyproject { .. } => "pip install -e .",
        dep::DepManager::PythonRequirements { .. } => "pip install -r requirements.txt",
        dep::DepManager::Go { .. } => "go mod download",
        dep::DepManager::Deno { .. } => "deno install",
    }
}

/// Dependency readiness for every manager detected in `project_dir`.
///
/// Empty when the project declares no dependencies at all — the
/// "nothing-to-install" state, which callers report separately since it is not
/// a problem.
pub fn dependency_checks(project_dir: &Path) -> Vec<Check> {
    let detections = dep::detect_all(project_dir);

    // A package.json with no lockfile declares dependencies that npm can still
    // install. `detect_all` keys off lockfiles, so without this a project in
    // that state reported "none declared" here while the post-run hint said
    // dependencies were missing — two different answers on one run.
    if detections.is_empty() && project_dir.join("package.json").is_file() {
        let subject = "dependencies (npm, no lockfile)".to_string();
        return vec![if project_dir.join("node_modules").is_dir() {
            Check::pass(subject, "installed")
        } else {
            Check::warn(
                subject,
                "package.json declares dependencies but none are installed",
                "npm install",
            )
        }];
    }

    detections
        .iter()
        .map(|detection| {
            let subject = format!("dependencies ({})", detection.label);
            match dep_state(project_dir, &detection.manager) {
                DepState::Installed => Check::pass(subject, "installed and up to date"),
                DepState::OutOfSync => Check::warn(
                    subject,
                    "installed, but the lockfile requires packages that are not in node_modules",
                    repair_command(&detection.manager),
                ),
                // `runx install` is the right fix here: nothing is installed,
                // so the manager's own clean-install path works.
                DepState::Missing => Check::fail(subject, "not installed", "runx install"),
            }
        })
        .collect()
}

/// Runtime readiness for every runtime the config asks for.
///
/// `offline` only changes the wording of the fix: a missing runtime is a hard
/// failure offline, and a download away otherwise.
pub fn runtime_checks(config: &config::RunxConfig, offline: bool) -> Vec<Check> {
    let home = cache::runx_home().ok();
    config
        .runtimes
        .iter()
        .map(|(tool, requirement)| runtime_check(home.as_deref(), tool, requirement, offline))
        .collect()
}

/// `home` is the cache root to probe; `None` means the cache is unreachable,
/// so every runtime reads as uncached. Passed explicitly rather than read from
/// the environment so tests can point at a scratch dir without mutating
/// process-wide state.
fn runtime_check(home: Option<&Path>, tool: &str, requirement: &str, offline: bool) -> Check {
    let subject = format!("runtime {tool} {requirement}");

    // A range cannot be checked against the cache without the release index,
    // which is a network lookup — exactly what this module must not do.
    if version::validate_concrete(tool, requirement).is_err() {
        return Check::warn(
            subject,
            "requirement is a range, so the exact version is resolved at run time",
            &format!(
                "pin an exact version in {}, then `runx lock`",
                config::CONFIG_FILE
            ),
        );
    }

    match runtime::resolve_runtime(tool, requirement) {
        Ok(spec) => {
            // `cached_runtime_in` also adopts legacy receipt-less installs, so
            // a working older cache does not read as missing.
            let cached = home
                .is_some_and(|home| matches!(cache::cached_runtime_in(home, &spec), Ok(Some(_))));
            if cached {
                Check::pass(subject, "cached and ready")
            } else if offline {
                Check::fail(
                    subject,
                    "not in the runtime cache, and --offline forbids downloading it",
                    "run once without --offline, or `runx lock` on a connected machine",
                )
            } else {
                Check::warn(
                    subject,
                    "not in the runtime cache; it is downloaded on first use",
                    "runx install",
                )
            }
        }
        // An unsupported tool or a rejected version is a config error, not a
        // cache state. Surface the validation message itself.
        Err(err) => Check::fail(
            subject,
            format!("{err}"),
            &format!("correct the [runtimes] entry in {}", config::CONFIG_FILE),
        ),
    }
}

/// Lockfile readiness: present, absent, or out of step with the config.
///
/// Read-only, like everything else here. A malformed lockfile surfaces as a
/// `Fail` rather than an error so doctor can report it alongside other checks
/// instead of aborting.
pub fn lockfile_check(project_dir: &Path, config: &config::RunxConfig) -> Check {
    let subject = "lockfile runx.lock".to_string();
    if config.runtimes.is_empty() {
        return Check::pass(subject, "not needed: this project declares no [runtimes]");
    }

    let path = lock::lock_path(project_dir);
    if !path.exists() {
        return Check::warn(
            subject,
            "absent, so runtime versions are not pinned",
            "runx lock",
        );
    }

    let lockfile = match lock::Lockfile::load(project_dir) {
        Ok(Some(lockfile)) => lockfile,
        // `load` returning None with the file present means it vanished
        // between the two calls; treat it as absent.
        Ok(None) => {
            return Check::warn(
                subject,
                "absent, so runtime versions are not pinned",
                "runx lock",
            );
        }
        Err(err) => {
            return Check::fail(
                subject,
                format!("cannot be read: {}", first_line(&format!("{err:#}"))),
                "delete runx.lock and run `runx lock` to regenerate it",
            );
        }
    };

    let mut stale = Vec::new();
    for (tool, requirement) in &config.runtimes {
        match lockfile.resolve(tool, requirement) {
            Ok(_) => {}
            Err(lock::Staleness::Missing) => stale.push(format!("{tool} is not pinned")),
            Err(lock::Staleness::RequirementChanged { locked, current }) => stale.push(format!(
                "{tool} is pinned from `{locked}` but {} asks for `{current}`",
                config::CONFIG_FILE
            )),
            Err(lock::Staleness::PlatformMissing { platform }) => {
                stale.push(format!("{tool} has no recorded artifact for {platform}"))
            }
        }
    }

    if stale.is_empty() {
        Check::pass(subject, "present and matches runx.toml")
    } else {
        Check::warn(
            subject,
            format!("out of date: {}", stale.join("; ")),
            "runx lock",
        )
    }
}

/// The first line of a multi-line error, for one-line check output.
fn first_line(text: &str) -> &str {
    text.lines().next().unwrap_or(text).trim_end()
}

/// A full readiness report for a project.
#[derive(Debug, Clone)]
pub struct Report {
    pub runtimes: Vec<Check>,
    pub dependencies: Vec<Check>,
    pub lockfile: Check,
    /// True when the project declares no dependency manifest at all.
    pub no_dependencies: bool,
    offline: bool,
}

impl Report {
    /// Inspect `project_dir` against `config`. Read-only and offline.
    pub fn collect(project_dir: &Path, config: &config::RunxConfig, offline: bool) -> Self {
        let dependencies = dependency_checks(project_dir);
        Self {
            runtimes: runtime_checks(config, offline),
            no_dependencies: dependencies.is_empty(),
            dependencies,
            lockfile: lockfile_check(project_dir, config),
            offline,
        }
    }

    /// Every check, in reporting order.
    pub fn checks(&self) -> impl Iterator<Item = &Check> {
        self.runtimes
            .iter()
            .chain(self.dependencies.iter())
            .chain(std::iter::once(&self.lockfile))
    }

    /// True when nothing would block execution.
    pub fn is_ready(&self) -> bool {
        !self.checks().any(|check| check.status == Status::Fail)
    }

    /// The checks that would block execution.
    pub fn blockers(&self) -> Vec<&Check> {
        self.checks()
            .filter(|check| check.status == Status::Fail)
            .collect()
    }

    /// The pre-flight block printed before an `--offline` run.
    ///
    /// RX-UX-03 asked for runtime cache status, dependency state and the
    /// network policy *before* execution, so a user can tell a missing runtime
    /// from a missing dependency without reading a download URL.
    pub fn render_preflight(&self) -> String {
        let mut out = String::from("runx readiness:\n");
        for check in &self.runtimes {
            out.push_str(&format!("  {}\n", indent_fix(&check.render())));
        }
        if self.no_dependencies {
            out.push_str("  ✓ dependencies: none declared by this project\n");
        } else {
            for check in &self.dependencies {
                out.push_str(&format!("  {}\n", indent_fix(&check.render())));
            }
        }
        out.push_str(&format!(
            "  network: {}\n",
            if self.offline {
                "disabled by --offline (no downloads, no version lookups)"
            } else {
                "available (downloads allowed)"
            }
        ));
        out
    }
}

/// Re-indent the continuation lines of a rendered check for nesting under a
/// two-space list.
fn indent_fix(rendered: &str) -> String {
    rendered.replace('\n', "\n  ")
}

/// The readiness checks as indented lines, for `runx doctor`.
///
/// Returned as lines rather than printed so doctor owns the section header and
/// the surrounding format.
pub fn render_section(report: &Report) -> Vec<String> {
    let mut lines = Vec::new();
    for check in &report.runtimes {
        lines.push(format!("  {}", indent_fix(&check.render())));
    }
    if report.no_dependencies {
        lines.push("  ✓ dependencies: none declared by this project".to_string());
    } else {
        for check in &report.dependencies {
            lines.push(format!("  {}", indent_fix(&check.render())));
        }
    }
    lines.push(format!("  {}", indent_fix(&report.lockfile.render())));
    lines
}

/// Explain a failed dependency install: the condition, what changed on disk,
/// and the exact recovery command.
///
/// RX-UX-01: a package manager's own failure says nothing about which of the
/// four states runx detected or whether the tree was left half-written. `before`
/// and `after` are sampled around the install, so "no files changed" is an
/// observation rather than an assumption.
pub fn install_failure_context(
    detection: &dep::DepDetection,
    before: DepState,
    after: DepState,
    project_dir: &Path,
) -> String {
    let manifest = manifest_path(&detection.manager);
    // Checked before `before` is consulted: npm exits EUSAGE when package.json
    // and the lockfile disagree, a condition the pre-install state cannot see.
    // The tree may be entirely absent (`Missing`) while the real fault is that
    // the two manifests are out of sync — which is what the user must fix, and
    // `npm install` is the only command that does it.
    let condition = if let Some(missing) = manifests_out_of_sync(project_dir, &detection.manager) {
        format!(
            "Detected condition: package.json and {} are out of sync — {} \
             declared in package.json but absent from the lockfile. \
             `npm ci` refuses this by design; it is a lockfile problem, not a failed download.",
            manifest
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| "the lockfile".to_string()),
            describe_missing(&missing),
        )
    } else {
        match before {
            DepState::OutOfSync => format!(
                "Detected condition: {} requires packages that are not installed, so the \
                 lockfile and the installed tree disagree.",
                manifest.display()
            ),
            DepState::Missing => format!(
                "Detected condition: no dependencies were installed for {}.",
                detection.label
            ),
            // The install only runs when deps are not already usable, so this
            // means the state changed underneath us.
            DepState::Installed => format!(
                "Detected condition: dependencies for {} looked installed when the \
                 install started.",
                detection.label
            ),
        }
    };

    let changed = if before == after {
        "File state: unchanged — the failed install wrote nothing that fixes this."
    } else {
        "File state: the dependency tree changed before the failure, so it may be partial."
    };

    format!(
        "Installing {} dependencies failed in {}.\n\
         {condition}\n\
         {changed}\n\
         Recovery: run `{}` in {}, then `runx install` again.",
        detection.label,
        project_dir.display(),
        repair_command(&detection.manager),
        project_dir.display(),
    )
}

/// The dependencies `package.json` declares that the lockfile does not record.
///
/// This is the precise condition npm reports as `EUSAGE ... must be in sync`.
/// Detected from the two manifests rather than by matching npm's output, since
/// the install streams straight to the terminal (so runx never sees its text)
/// and npm's wording changes between majors. `None` means in sync, not
/// npm-family, or unreadable.
fn manifests_out_of_sync(project_dir: &Path, manager: &dep::DepManager) -> Option<Vec<String>> {
    let lockfile = match manager {
        dep::DepManager::Npm { lockfile }
        | dep::DepManager::Yarn { lockfile }
        | dep::DepManager::Pnpm { lockfile } => lockfile,
        _ => return None,
    };

    let raw = std::fs::read_to_string(project_dir.join("package.json")).ok()?;
    let manifest: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let mut declared = std::collections::BTreeSet::new();
    for field in ["dependencies", "devDependencies", "optionalDependencies"] {
        if let Some(table) = manifest.get(field).and_then(|value| value.as_object()) {
            declared.extend(table.keys().cloned());
        }
    }
    if declared.is_empty() {
        return None;
    }

    // Lockfile `packages` keys are paths like `node_modules/is-odd`; compare on
    // the final component so nested entries still match their package name.
    let locked: std::collections::BTreeSet<String> = installed_package_paths(lockfile)?
        .iter()
        .filter_map(|key| key.rsplit("node_modules/").next().map(str::to_string))
        .collect();

    let missing: Vec<String> = declared.difference(&locked).cloned().collect();
    if missing.is_empty() {
        None
    } else {
        Some(missing)
    }
}

/// Render missing package names for a one-line condition, capped so a large
/// mismatch does not flood the terminal.
fn describe_missing(missing: &[String]) -> String {
    const MAX: usize = 3;
    let shown: Vec<&str> = missing.iter().take(MAX).map(String::as_str).collect();
    let rendered = shown.join(", ");
    match missing.len() {
        1 => format!("`{rendered}` is"),
        n if n <= MAX => format!("`{rendered}` are"),
        n => format!("`{rendered}` and {} more are", n - MAX),
    }
}

/// The path to a project's package manager manifest, for display.
pub fn manifest_path(manager: &dep::DepManager) -> PathBuf {
    match manager {
        dep::DepManager::Npm { lockfile }
        | dep::DepManager::Yarn { lockfile }
        | dep::DepManager::Pnpm { lockfile }
        | dep::DepManager::Bun { lockfile }
        | dep::DepManager::Deno { lockfile } => lockfile.clone(),
        dep::DepManager::PythonPyproject { pyproject } => pyproject.clone(),
        dep::DepManager::PythonRequirements { requirements } => requirements.clone(),
        dep::DepManager::Go { gomod } => gomod.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn tmp() -> TempDir {
        tempfile::tempdir().expect("create temp dir")
    }

    /// A config built through the validating constructor, since
    /// `RunxConfig` deliberately has no `Default` — every construction path
    /// must pass validation.
    fn config_with(runtimes: &[(&str, &str)]) -> config::RunxConfig {
        let map = runtimes
            .iter()
            .map(|(tool, req)| (tool.to_string(), req.to_string()))
            .collect();
        config::RunxConfig::from_parts(map, std::collections::BTreeMap::new())
            .expect("test config must be valid")
    }

    fn npm_project(dir: &Path) -> dep::DepManager {
        fs::write(dir.join("package.json"), "{}").unwrap();
        fs::write(dir.join("package-lock.json"), "{}").unwrap();
        dep::DepManager::Npm {
            lockfile: dir.join("package-lock.json"),
        }
    }

    /// Write npm's hidden lockfile, the record of what was actually installed.
    fn write_installed_manifest(dir: &Path, packages: &[&str]) {
        let entries: Vec<String> = packages
            .iter()
            .map(|name| format!("\"node_modules/{name}\": {{\"version\": \"1.0.0\"}}"))
            .collect();
        fs::create_dir_all(dir.join("node_modules")).unwrap();
        fs::write(
            dir.join("node_modules").join(".package-lock.json"),
            format!("{{\"packages\": {{{}}}}}", entries.join(",")),
        )
        .unwrap();
    }

    /// Write a `package-lock.json` naming `packages`, plus the root entry.
    fn write_lockfile(dir: &Path, packages: &[&str]) {
        let mut entries = vec!["\"\": {\"name\": \"p\"}".to_string()];
        entries.extend(
            packages
                .iter()
                .map(|name| format!("\"node_modules/{name}\": {{\"version\": \"1.0.0\"}}")),
        );
        fs::write(
            dir.join("package-lock.json"),
            format!("{{\"packages\": {{{}}}}}", entries.join(",")),
        )
        .unwrap();
    }

    /// Issue #3: mtime order must never decide staleness. `cp -R` writes
    /// node_modules a few hundred microseconds before the lockfile, which made
    /// a freshly copied project report a changed lockfile.
    #[test]
    fn a_lockfile_newer_than_node_modules_is_not_stale_on_its_own() {
        let dir = tmp();
        fs::write(dir.path().join("package.json"), "{}").unwrap();
        write_lockfile(dir.path(), &["is-odd"]);
        write_installed_manifest(dir.path(), &["is-odd"]);
        let manager = dep::DepManager::Npm {
            lockfile: dir.path().join("package-lock.json"),
        };

        // Make the lockfile unambiguously newer, the exact mtime ordering that
        // used to produce a false warning.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        write_lockfile(dir.path(), &["is-odd"]);

        assert_eq!(
            dep_state(dir.path(), &manager),
            DepState::Installed,
            "a newer lockfile with the same packages installed is not out of sync"
        );
    }

    /// Issue #3: a genuine mismatch must still be caught — the lockfile
    /// requires a package that is not in node_modules.
    #[test]
    fn a_package_missing_from_node_modules_is_out_of_sync() {
        let dir = tmp();
        fs::write(dir.path().join("package.json"), "{}").unwrap();
        write_lockfile(dir.path(), &["is-odd", "left-pad"]);
        write_installed_manifest(dir.path(), &["is-odd"]);
        let manager = dep::DepManager::Npm {
            lockfile: dir.path().join("package-lock.json"),
        };

        assert_eq!(
            dep_state(dir.path(), &manager),
            DepState::OutOfSync,
            "left-pad is locked but not installed"
        );
    }

    /// Issue #3: a manager that records nothing about what it installed gets
    /// no warning, rather than a guess. Bun has no hidden lockfile.
    #[test]
    fn a_manager_without_an_install_record_is_never_reported_stale() {
        let dir = tmp();
        fs::write(dir.path().join("bun.lock"), "{}").unwrap();
        fs::create_dir_all(dir.path().join("node_modules")).unwrap();
        let manager = dep::DepManager::Bun {
            lockfile: dir.path().join("bun.lock"),
        };

        std::thread::sleep(std::time::Duration::from_millis(1100));
        fs::write(dir.path().join("bun.lock"), "{}").unwrap();

        assert_eq!(
            dep_state(dir.path(), &manager),
            DepState::Installed,
            "no evidence means no warning the user cannot act on"
        );
    }

    /// Issue #2: when package.json declares a dependency the lockfile lacks,
    /// the condition names the out-of-sync manifests — the state npm reports
    /// as EUSAGE — even though no dependencies are installed at all.
    #[test]
    fn install_failure_names_out_of_sync_manifests() {
        let dir = tmp();
        fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies":{"is-odd":"3.0.1"}}"#,
        )
        .unwrap();
        write_lockfile(dir.path(), &[]);
        let detection = dep::DepDetection {
            manager: dep::DepManager::Npm {
                lockfile: dir.path().join("package-lock.json"),
            },
            label: "npm",
        };

        let context =
            install_failure_context(&detection, DepState::Missing, DepState::Missing, dir.path());
        assert!(
            context.contains("package.json and package-lock.json are out of sync"),
            "must name the out-of-sync manifests, got:\n{context}"
        );
        assert!(
            context.contains("is-odd"),
            "must name the offending package:\n{context}"
        );
        assert!(
            context.contains("npm install"),
            "must give the repair command:\n{context}"
        );
    }

    /// Issue #2: a failure with manifests *in* sync keeps the plain
    /// missing-dependencies wording, so the new branch is not a blanket label.
    #[test]
    fn install_failure_without_a_mismatch_reports_missing_dependencies() {
        let dir = tmp();
        fs::write(
            dir.path().join("package.json"),
            r#"{"dependencies":{"is-odd":"3.0.1"}}"#,
        )
        .unwrap();
        write_lockfile(dir.path(), &["is-odd"]);
        let detection = dep::DepDetection {
            manager: dep::DepManager::Npm {
                lockfile: dir.path().join("package-lock.json"),
            },
            label: "npm",
        };

        let context =
            install_failure_context(&detection, DepState::Missing, DepState::Missing, dir.path());
        assert!(
            !context.contains("out of sync"),
            "in-sync manifests must not be called out of sync:\n{context}"
        );
        assert!(
            context.contains("no dependencies were installed"),
            "{context}"
        );
    }

    /// The three dependency states RX-UX-01 asked to distinguish, each from a
    /// filesystem layout a user can actually produce.
    #[test]
    fn dep_state_separates_missing_installed_and_out_of_sync() {
        let dir = tmp();
        let manager = npm_project(dir.path());

        assert_eq!(
            dep_state(dir.path(), &manager),
            DepState::Missing,
            "no node_modules at all"
        );

        write_lockfile(dir.path(), &["is-odd"]);
        write_installed_manifest(dir.path(), &["is-odd"]);
        assert_eq!(
            dep_state(dir.path(), &manager),
            DepState::Installed,
            "every locked package is installed"
        );

        // The lockfile now requires a package that is not in node_modules.
        write_lockfile(dir.path(), &["is-odd", "left-pad"]);
        assert_eq!(
            dep_state(dir.path(), &manager),
            DepState::OutOfSync,
            "a locked package is missing from the installed tree"
        );
    }

    /// An out-of-sync npm project must recommend `npm install`, not `npm ci`:
    /// `npm ci` is the command that fails in this state, which is the exact
    /// confusion reported in RX-UX-01.
    #[test]
    fn out_of_sync_npm_recommends_npm_install() {
        let dir = tmp();
        npm_project(dir.path());
        write_lockfile(dir.path(), &["is-odd", "left-pad"]);
        write_installed_manifest(dir.path(), &["is-odd"]);

        let checks = dependency_checks(dir.path());
        let npm = checks
            .iter()
            .find(|check| check.subject.contains("npm"))
            .expect("npm must be detected");
        assert_eq!(npm.status, Status::Warn);
        assert_eq!(npm.fix.as_deref(), Some("npm install"));
        assert!(
            npm.detail.contains("not in node_modules"),
            "must name the condition, got: {}",
            npm.detail
        );
    }

    /// A project with no manifest is "nothing to install", not a failure.
    #[test]
    fn no_manifest_yields_no_dependency_checks() {
        let dir = tmp();
        let report = Report::collect(dir.path(), &config_with(&[]), false);
        assert!(report.no_dependencies);
        assert!(report.is_ready());
        assert!(report.render_preflight().contains("none declared"));
    }

    /// Offline turns an uncached runtime into a hard failure, and the
    /// pre-flight names the policy rather than printing a download URL.
    #[test]
    fn offline_uncached_runtime_fails_with_policy_stated() {
        let home = tmp();
        let check = runtime_check(Some(home.path()), "node", "20.11.0", true);

        assert_eq!(check.status, Status::Fail, "{}", check.detail);
        assert!(check.detail.contains("--offline"), "{}", check.detail);
        assert!(
            !check.detail.contains("https://"),
            "must not surface a download URL: {}",
            check.detail
        );
    }

    /// The same uncached runtime is only a warning when downloads are allowed.
    #[test]
    fn uncached_runtime_online_is_only_a_warning() {
        let home = tmp();
        let check = runtime_check(Some(home.path()), "node", "20.11.0", false);
        assert_eq!(check.status, Status::Warn, "{}", check.detail);
    }

    /// A range cannot be resolved offline, so it is a warning that names the
    /// reason rather than a spurious cache miss.
    #[test]
    fn range_requirement_warns_instead_of_claiming_a_cache_miss() {
        let checks = runtime_checks(&config_with(&[("node", ">=20")]), false);
        assert_eq!(checks.len(), 1);
        assert_eq!(checks[0].status, Status::Warn);
        assert!(checks[0].detail.contains("range"), "{}", checks[0].detail);
    }

    /// An absent lockfile is a warning with `runx lock` as the fix; a project
    /// with no runtimes needs no lockfile at all.
    #[test]
    fn lockfile_check_reports_absent_and_not_needed() {
        let dir = tmp();

        let empty = config_with(&[]);
        let check = lockfile_check(dir.path(), &empty);
        assert_eq!(check.status, Status::Pass);
        assert!(check.detail.contains("no [runtimes]"), "{}", check.detail);

        let check = lockfile_check(dir.path(), &config_with(&[("node", "20.11.0")]));
        assert_eq!(check.status, Status::Warn);
        assert_eq!(check.fix.as_deref(), Some("runx lock"));
    }

    /// A malformed lockfile is reported as a check, not an abort, and the fix
    /// is stated. Doctor relies on this to keep listing other checks.
    #[test]
    fn malformed_lockfile_is_a_failed_check_with_a_fix() {
        let dir = tmp();
        fs::write(dir.path().join("runx.lock"), "not toml {{{\n").unwrap();

        let check = lockfile_check(dir.path(), &config_with(&[("node", "20.11.0")]));
        assert_eq!(check.status, Status::Fail);
        assert!(check.detail.contains("cannot be read"), "{}", check.detail);
        assert!(check.fix.is_some(), "a malformed lockfile needs a fix");
        assert!(
            !check.detail.contains('\n'),
            "the detail must stay on one line: {}",
            check.detail
        );
    }

    /// A stale lockfile names both versions, so the user does not have to diff
    /// the files by hand.
    #[test]
    fn stale_lockfile_names_both_versions() {
        let dir = tmp();
        fs::write(
            dir.path().join("runx.lock"),
            "version = 1\n\n[runtimes.node]\nversion = \"18.20.0\"\nrequirement = \"18.20.0\"\n",
        )
        .unwrap();

        let check = lockfile_check(dir.path(), &config_with(&[("node", "20.11.0")]));
        assert_eq!(check.status, Status::Warn);
        assert!(check.detail.contains("18.20.0"), "{}", check.detail);
        assert!(check.detail.contains("20.11.0"), "{}", check.detail);
    }
}
