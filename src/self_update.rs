//! `runx self update` — replace the running binary with the newest release.
//!
//! The update flow mirrors a runtime install: the release archive is
//! downloaded, verified against the SHA-256 checksums published alongside it,
//! extracted, and only then swapped in. The swap is atomic: the current binary
//! is renamed aside, the verified one renamed into place, and the backup
//! removed — restoring the old binary on any failure.
//!
//! Release assets follow the convention documented in the README:
//! `runx-{os}-{arch}.tar.gz` (or `.zip` on Windows) plus a `SHA256SUMS`
//! manifest. Signed release artifacts (Sigstore/cosign) are not yet verified;
//! see the README security section for the documented path.

use crate::error::UserError;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::{
    env, fs,
    path::{Path, PathBuf},
};

/// Where runx publishes releases and their metadata.
const RELEASES_LATEST_URL: &str = "https://api.github.com/repos/aryankahar31/runx/releases/latest";

/// The newest published release, as listed by the GitHub API.
#[derive(Debug, Deserialize)]
struct LatestRelease {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Debug, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

/// Replace the running binary with the newest release, if one is newer.
pub fn update() -> Result<()> {
    let current_version = env!("CARGO_PKG_VERSION");
    let token = platform_token()?;

    let body = crate::http::get(RELEASES_LATEST_URL)
        .call()
        .map_err(|err| match err {
            ureq::Error::Status(404, _) => UserError::new(
                "No releases found for aryankahar31/runx yet — nothing to update to.",
            )
            .into(),
            other => anyhow::Error::from(other),
        })
        .with_context(|| format!("Failed to check for updates at {RELEASES_LATEST_URL}"))?
        .into_string()
        .context("Failed to read the latest release metadata")?;

    let release: LatestRelease =
        serde_json::from_str(&body).context("Failed to decode the latest release metadata")?;

    let latest_version = release.tag_name.trim_start_matches(['v', 'V']).to_string();

    if !is_newer(&latest_version, current_version)? {
        println!("runx {current_version} is already the newest release.");
        return Ok(());
    }

    let (archive_url, checksum_url) = select_assets(&release, token).ok_or_else(|| {
        UserError::new(format!(
            "Release {latest_version} does not publish the expected assets for \
                 {token} (`runx-{token}.tar.gz`/`.zip` plus `SHA256SUMS`)."
        ))
    })?;

    println!("Updating runx {current_version} -> {latest_version}");
    let download = crate::downloader::download_to_temp(&archive_url, &checksum_url, None)?;

    let extract_dir = tempfile::tempdir().context("Failed to create a temporary directory")?;
    crate::extractor::extract_archive(download.path(), extract_dir.path(), archive_kind(token))?;
    drop(download);

    let new_binary = find_binary(extract_dir.path())
        .ok_or_else(|| UserError::new("The release archive did not contain a runx executable."))?;

    install_binary(&new_binary)?;
    println!("Updated to runx {latest_version}.");
    Ok(())
}

/// The platform token used in release asset names (`macos-arm64`, ...).
///
/// Must match the naming the release workflow publishes: `{os}-{arch}` with
/// os in linux/macos/windows and arch in x64/arm64.
fn platform_token() -> Result<&'static str> {
    match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => Ok("linux-x64"),
        ("linux", "aarch64") => Ok("linux-arm64"),
        ("macos", "x86_64") => Ok("macos-x64"),
        ("macos", "aarch64") => Ok("macos-arm64"),
        ("windows", "x86_64") => Ok("windows-x64"),
        ("windows", "aarch64") => Ok("windows-arm64"),
        (os, arch) => {
            Err(UserError::new(format!("runx self update is not supported on {os}/{arch}.")).into())
        }
    }
}

/// The archive format runx publishes for `token`'s platform.
fn archive_kind(token: &str) -> crate::runtime::ArchiveKind {
    if token.starts_with("windows-") {
        crate::runtime::ArchiveKind::Zip
    } else {
        crate::runtime::ArchiveKind::TarGz
    }
}

/// True when `latest` is strictly newer than `current`, both plain versions.
/// A cosmetic leading `v` on either side is ignored.
fn is_newer(latest: &str, current: &str) -> Result<bool> {
    let latest = crate::version::Version::parse(latest.trim_start_matches(['v', 'V']))
        .ok_or_else(|| UserError::new(format!("Release tag `{latest}` is not a version.")))?;
    let current = crate::version::Version::parse(current.trim_start_matches(['v', 'V']))
        .ok_or_else(|| UserError::new(format!("Current version `{current}` is not a version.")))?;
    Ok(latest > current)
}

/// Locate the archive and its checksum manifest in a release.
///
/// Returns `(archive_url, checksum_url)` for the current platform.
fn select_assets(release: &LatestRelease, token: &str) -> Option<(String, String)> {
    let archive = release.assets.iter().find(|asset| {
        asset.name == format!("runx-{token}.tar.gz") || asset.name == format!("runx-{token}.zip")
    })?;
    let checksums = release
        .assets
        .iter()
        .find(|asset| asset.name == "SHA256SUMS")?;
    Some((
        archive.browser_download_url.clone(),
        checksums.browser_download_url.clone(),
    ))
}

/// Find the `runx` executable inside an extraction, a few levels deep at most.
///
/// The depth cap keeps the walk safe: a crafted archive could nest directories
/// to arbitrary depth, but the binary is never more than a couple of levels in
/// (at the root, under `bin/`, or under one wrapping directory).
fn find_binary(dir: &Path) -> Option<PathBuf> {
    let name = if cfg!(windows) { "runx.exe" } else { "runx" };

    let mut stack = vec![(dir.to_path_buf(), 0)];
    while let Some((current, depth)) = stack.pop() {
        let entries = fs::read_dir(&current).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            let file_type = entry.file_type().ok()?;
            if file_type.is_dir() {
                if depth < 3 {
                    stack.push((path, depth + 1));
                }
            } else if file_type.is_file() && path.file_name().is_some_and(|n| n == name) {
                return Some(path);
            }
        }
    }
    None
}

/// Swap the verified binary into place next to the running executable.
///
/// The new binary is first copied into the executable's own directory so the
/// final renames never cross filesystems. The old binary is renamed aside,
/// the new one renamed onto the name, and the backup dropped; if the second
/// rename fails the old binary is restored.
fn install_binary(new_binary: &Path) -> Result<()> {
    let exe = env::current_exe().context("Failed to determine the current executable path")?;
    let exe_dir = exe
        .parent()
        .ok_or_else(|| UserError::new("The current executable has no parent directory."))?;

    // Same directory as the executable guarantees the rename stays on one
    // filesystem, whatever the tempdir sits on.
    let staged = tempfile::Builder::new()
        .prefix(".runx-update-")
        .tempfile_in(exe_dir)
        .context("Failed to create a staging file next to the executable")?;
    fs::copy(new_binary, staged.path())
        .with_context(|| format!("Failed to copy the new binary into {}", exe_dir.display()))?;

    // The downloaded archive is verified, but its extracted mode may not be
    // executable (e.g. a zip built on Windows). Ensure it can run.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(staged.path(), fs::Permissions::from_mode(0o755))
            .context("Failed to make the new binary executable")?;
    }

    let backup = exe.with_file_name(format!(
        "{}.old",
        exe.file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("runx")
    ));

    fs::rename(&exe, &backup).with_context(|| {
        format!(
            "Failed to move the current binary aside ({}).",
            backup.display()
        )
    })?;

    if let Err(err) = fs::rename(staged.path(), &exe) {
        // Put the old binary back before reporting the failure.
        let _ = fs::rename(&backup, &exe);
        return Err(err).with_context(|| {
            format!(
                "Failed to install the new binary at {}. \
                 The previous version was restored.",
                exe.display()
            )
        });
    }

    let _ = fs::remove_file(&backup);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(json: &str) -> LatestRelease {
        serde_json::from_str(json).expect("fixture parses")
    }

    #[test]
    fn selects_the_archive_and_checksums_for_the_platform() {
        let release = fixture(
            r#"{
            "tag_name": "v0.3.0",
            "assets": [
                {"name": "runx-linux-x64.tar.gz", "browser_download_url": "https://x/linux.tgz"},
                {"name": "runx-macos-arm64.tar.gz", "browser_download_url": "https://x/macos.tgz"},
                {"name": "runx-macos-arm64.tar.gz.sig", "browser_download_url": "https://x/darwin.sig"},
                {"name": "SHA256SUMS", "browser_download_url": "https://x/SHA256SUMS"}
            ]
        }"#,
        );

        let (url, checksums) = select_assets(&release, "macos-arm64").expect("assets found");
        assert_eq!(url, "https://x/macos.tgz");
        assert_eq!(checksums, "https://x/SHA256SUMS");
    }

    /// An exact filename match, never a prefix: `runx-macos-arm64.tar.gz`
    /// must not satisfy a lookup for `macos-arm64` — and a missing SHA256SUMS
    /// means the release is not trustworthy enough to update from.
    #[test]
    fn missing_or_foreign_assets_yield_nothing() {
        let release = fixture(
            r#"{
            "tag_name": "v0.3.0",
            "assets": [
                {"name": "runx-linux-x64.tar.gz", "browser_download_url": "https://x/linux.tgz"},
                {"name": "SHA256SUMS", "browser_download_url": "https://x/SHA256SUMS"}
            ]
        }"#,
        );
        assert_eq!(select_assets(&release, "macos-arm64"), None);

        let no_checksums = fixture(
            r#"{
            "tag_name": "v0.3.0",
            "assets": [
                {"name": "runx-macos-arm64.tar.gz", "browser_download_url": "https://x/d.tgz"}
            ]
        }"#,
        );
        assert_eq!(select_assets(&no_checksums, "macos-arm64"), None);
    }

    #[test]
    fn windows_platforms_use_zips() {
        assert_eq!(
            archive_kind("windows-x64"),
            crate::runtime::ArchiveKind::Zip
        );
        assert_eq!(
            archive_kind("darwin-arm64"),
            crate::runtime::ArchiveKind::TarGz
        );
    }

    #[test]
    fn version_comparison_decides_whether_to_update() {
        assert!(is_newer("0.3.0", "0.2.0").expect("comparable"));
        assert!(!is_newer("0.2.0", "0.2.0").expect("equal is not newer"));
        assert!(!is_newer("0.2.0", "0.3.0").expect("older is not newer"));
        assert!(is_newer("0.10.0", "0.9.0").expect("numeric, not lexical"));
        assert!(is_newer("v0.3.0", "0.2.0").expect("leading v is cosmetic"));
    }

    /// The name the extracted runx executable has on this platform.
    fn exe_name() -> &'static str {
        if cfg!(windows) {
            "runx.exe"
        } else {
            "runx"
        }
    }

    #[test]
    fn garbage_release_tags_are_rejected() {
        assert!(is_newer("latest", "0.2.0").is_err());
        assert!(is_newer("0.3.0", "not-a-version").is_err());
    }

    #[test]
    fn finds_the_binary_under_a_wrapping_directory() {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::create_dir_all(dir.path().join("runx-darwin-arm64/bin")).unwrap();
        fs::write(
            dir.path().join("runx-darwin-arm64/bin").join(exe_name()),
            b"bin",
        )
        .unwrap();

        let found = find_binary(dir.path()).expect("binary found");
        assert_eq!(
            found.file_name().unwrap().to_str(),
            Some(exe_name()),
            "the runx executable itself"
        );
    }

    /// The walk must not follow symlinks, or a crafted archive could loop it.
    #[test]
    fn binary_search_does_not_follow_symlinks() {
        let dir = tempfile::tempdir().expect("temp dir");
        fs::write(dir.path().join(exe_name()), b"real").unwrap();
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.path(), dir.path().join("loop")).unwrap();
        }
        assert_eq!(
            find_binary(dir.path()).expect("found").file_name().unwrap(),
            exe_name(),
            "must find the real file and not hang on the symlink"
        );
    }
}
