//! `runx self update` — replace the running binary with the newest release.
//!
//! The update flow mirrors a runtime install: the release archive is
//! downloaded, verified against the SHA-256 checksums published alongside it,
//! extracted, and only then swapped in. The swap is atomic: the current binary
//! is renamed aside, the verified one renamed into place, and the backup
//! removed — restoring the old binary on any failure.
//!
//! Release assets follow the convention documented in the README:
//! `runx-{os}-{arch}.tar.gz` (or `.zip` on Windows), a `SHA256SUMS` manifest,
//! and since v0.4.2 a Sigstore bundle (`<archive>.sigstore.json`) signed
//! keylessly by the release workflow. The signature, when `cosign` is
//! available, is verified after the checksum; see [`verify_release_signature`]
//! for the graceful-degradation policy.

use crate::error::UserError;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Where runx publishes releases and their metadata.
const RELEASES_LATEST_URL: &str = "https://api.github.com/repos/aryankahar31/runx/releases/latest";

/// The OIDC subject of the signing workflow's certificates.
///
/// Only runs of `.github/workflows/release.yml` on a `v*.*.*` tag (or `main`)
/// of `aryankahar31/runx` can produce a signature the verifier accepts.
/// Regexp, not an exact string, because the subject embeds the triggering
/// tag (`...@refs/tags/v0.4.2`).
const SIGSTORE_IDENTITY_PATTERN: &str = "^https://github\\.com/aryankahar31/runx/\\.github/workflows/release\\.yml@refs/(tags/v[0-9]+\\.[0-9]+\\.[0-9]+|heads/main)$";

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
    crate::flags::ensure_network("check for updates")?;
    let current_version = env!("CARGO_PKG_VERSION");
    let token = platform_token()?;

    let response = crate::http::get(RELEASES_LATEST_URL).call();
    let body = match response {
        Ok(resp) => resp
            .into_string()
            .context("Failed to read the latest release metadata")?,
        Err(err) => {
            return Err(map_github_api_error(err));
        }
    };

    let release: LatestRelease =
        serde_json::from_str(&body).context("Failed to decode the latest release metadata")?;

    let latest_version = release.tag_name.trim_start_matches(['v', 'V']).to_string();

    if !is_newer(&latest_version, current_version)? {
        println!("runx {current_version} is already the newest release.");
        return Ok(());
    }

    let (archive_url, checksum_url, bundle_url) =
        select_assets(&release, token).ok_or_else(|| {
            UserError::new(format!(
                "Release {latest_version} does not publish the expected assets for \
                 {token} (`runx-{token}.tar.gz`/`.zip` plus `SHA256SUMS`)."
            ))
        })?;

    println!("Updating runx {current_version} -> {latest_version}");
    let download = crate::downloader::download_to_temp(&archive_url, &checksum_url, None)?;

    if let Some(bundle_url) = bundle_url {
        verify_release_signature(download.path(), &bundle_url)?;
    } else {
        degraded("this release was published without a Sigstore signature bundle")?;
    }

    let extract_dir = tempfile::tempdir().context("Failed to create a temporary directory")?;
    // The release archives put the binary at the root; the runtime extractor
    // would strip (and drop) root-level entries, so keep the top level here.
    crate::extractor::extract_archive_keep_top_level(
        download.path(),
        extract_dir.path(),
        archive_kind(token),
    )?;
    drop(download);

    let new_binary = find_binary(extract_dir.path())
        .ok_or_else(|| UserError::new("The release archive did not contain a runx executable."))?;

    install_binary(&new_binary)?;
    println!("Updated to runx {latest_version}.");
    Ok(())
}

/// Map GitHub API errors to actionable user messages.
fn map_github_api_error(err: ureq::Error) -> anyhow::Error {
    match err {
        ureq::Error::Status(403, resp) => {
            // GitHub API returns 403 for rate limiting (unauthenticated: 60 req/h)
            // and for other access-denied cases. Extract rate-limit headers if present.
            let remaining = resp.header("X-RateLimit-Remaining");
            let reset = resp.header("X-RateLimit-Reset");
            let retry_after = resp.header("Retry-After");

            let msg = if remaining == Some("0") {
                let wait_hint = if let Some(reset_str) = reset {
                    if let Ok(ts) = reset_str.parse::<u64>() {
                        let now = SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap_or(Duration::from_secs(0))
                            .as_secs();
                        if ts > now {
                            format!(" Try again after {} seconds.", ts - now)
                        } else {
                            String::new()
                        }
                    } else {
                        String::new()
                    }
                } else if let Some(retry_str) = retry_after {
                    format!(" Try again after {retry_str} seconds.")
                } else {
                    " Try again later.".to_string()
                };
                format!("Unable to check for updates: GitHub API rate limit reached.{wait_hint}")
            } else {
                // Some other 403 (e.g., repo not found, private repo, etc.)
                "Unable to check for updates: GitHub API access denied (HTTP 403). \
                 The repository may be private or the API token may lack permissions."
                    .to_string()
            };
            UserError::new(msg).into()
        }
        ureq::Error::Status(404, _) => {
            UserError::new("No releases found for aryankahar31/runx yet — nothing to update to.")
                .into()
        }
        other => anyhow::Error::from(other),
    }
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

/// Locate the archive, its checksum manifest, and its Sigstore bundle in a
/// release.
///
/// Returns `(archive_url, checksum_url, bundle_url)` for the current platform.
/// The bundle is `None` for releases published before signature support —
/// callers degrade to a warning then.
fn select_assets(release: &LatestRelease, token: &str) -> Option<(String, String, Option<String>)> {
    let archive = release.assets.iter().find(|asset| {
        asset.name == format!("runx-{token}.tar.gz") || asset.name == format!("runx-{token}.zip")
    })?;
    let checksums = release
        .assets
        .iter()
        .find(|asset| asset.name == "SHA256SUMS")?;
    let bundle = release
        .assets
        .iter()
        .find(|asset| {
            asset.name == format!("runx-{token}.tar.gz.sigstore.json")
                || asset.name == format!("runx-{token}.zip.sigstore.json")
        })
        .map(|asset| asset.browser_download_url.clone());
    Some((
        archive.browser_download_url.clone(),
        checksums.browser_download_url.clone(),
        bundle,
    ))
}

/// Verify the release archive's Sigstore signature with `cosign`, keyless
/// (OIDC) signing by the release workflow.
///
/// Graceful-degradation policy, mirroring the install scripts:
///
/// * `cosign` missing, or the release predating signature bundles → a warning
///   and the update proceeds. The SHA-256 checksum already verified the
///   archive and still fails closed on its own, and making `cosign` a hard
///   dependency of self-update would break it for the majority of users who
///   don't have it. `RUNX_REQUIRE_SIGNATURE=1` escalates either case into a
///   hard error for environments that demand signatures.
/// * `cosign` present and verification failing → always fatal. That is the
///   tamper/compromise case the signature exists to detect; the update stops.
///
/// The identity and issuer pins tie the signature to a run of
/// `release.yml` on `aryankahar31/runx` — never to any other workflow,
/// repository, or OpenID provider.
fn verify_release_signature(archive: &Path, bundle_url: &str) -> Result<()> {
    if !cosign_available() {
        return degraded("cosign is not installed");
    }

    let bundle = tempfile::Builder::new()
        .prefix("runx-signature-")
        .tempfile()
        .context("Failed to create a temporary file for the signature bundle")?;
    download_file(bundle_url, bundle.path())?;

    println!("Verifying release signature with cosign...");
    let output = Command::new("cosign")
        .arg("verify-blob")
        .arg(archive)
        .args(["--bundle", bundle.path().to_str().unwrap()])
        .args(["--certificate-identity-regexp", SIGSTORE_IDENTITY_PATTERN])
        .args([
            "--certificate-oidc-issuer",
            "https://token.actions.githubusercontent.com",
        ])
        .output()
        .context("Failed to run cosign")?;

    if output.status.success() {
        println!("Signature verified.");
        return Ok(());
    }

    Err(UserError::new(format!(
        "Signature verification FAILED for the release archive.\n{}\n{}\n\
         This indicates a tampered or misattributed release. Aborting.",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    ))
    .into())
}

/// True when a working `cosign` binary is on `PATH`.
fn cosign_available() -> bool {
    // `--version` was removed in modern cosign; the `version` subcommand is
    // the portable check across the supported v2/v3 lines.
    matches!(
        Command::new("cosign").arg("version").output(),
        Ok(out) if out.status.success()
    )
}

/// The graceful side of signature verification: warn and continue, unless
/// `RUNX_REQUIRE_SIGNATURE=1` demands a hard stop.
fn degraded(reason: &str) -> Result<()> {
    if env::var("RUNX_REQUIRE_SIGNATURE").as_deref() == Ok("1") {
        return Err(UserError::new(format!(
            "{reason}; refusing to update because RUNX_REQUIRE_SIGNATURE=1"
        ))
        .into());
    }
    eprintln!(
        "Warning: {reason} — release signature verification skipped; \
         the SHA-256 checksum was still verified and enforced. \
         Set RUNX_REQUIRE_SIGNATURE=1 to require signatures."
    );
    Ok(())
}

/// Download `url` into `path` with runx's standard client. Used for the
/// signature bundle, which has no checksum entry of its own.
fn download_file(url: &str, path: &Path) -> Result<()> {
    let mut reader = crate::http::get(url)
        .call()
        .map_err(|err| UserError::new(format!("Failed to download the signature bundle: {err}")))?
        .into_reader();
    let mut file = fs::File::create(path).context("Failed to create the signature bundle file")?;
    std::io::copy(&mut reader, &mut file).context("Failed to save the signature bundle")?;
    Ok(())
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
                {"name": "runx-macos-arm64.tar.gz.sigstore.json", "browser_download_url": "https://x/macos.tgz.sigstore.json"},
                {"name": "SHA256SUMS", "browser_download_url": "https://x/SHA256SUMS"}
            ]
        }"#,
        );

        let (url, checksums, bundle) =
            select_assets(&release, "macos-arm64").expect("assets found");
        assert_eq!(url, "https://x/macos.tgz");
        assert_eq!(checksums, "https://x/SHA256SUMS");
        assert_eq!(
            bundle.as_deref(),
            Some("https://x/macos.tgz.sigstore.json"),
            "the Sigstore bundle travels with its archive"
        );
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

    /// Releases published before signature support have no bundle; the
    /// caller degrades to a warning instead of failing the update.
    #[test]
    fn pre_signature_releases_yield_no_bundle() {
        let release = fixture(
            r#"{
            "tag_name": "v0.4.1",
            "assets": [
                {"name": "runx-macos-arm64.tar.gz", "browser_download_url": "https://x/macos.tgz"},
                {"name": "SHA256SUMS", "browser_download_url": "https://x/SHA256SUMS"}
            ]
        }"#,
        );
        let (_, _, bundle) = select_assets(&release, "macos-arm64").expect("assets found");
        assert_eq!(
            bundle, None,
            "no bundle asset means no verification attempt"
        );
    }

    /// `RUNX_REQUIRE_SIGNATURE=1` turns the graceful warning into a hard
    /// error; the default stays a warning. Single test touches the variable
    /// so parallel tests cannot interleave mutations.
    #[test]
    fn require_signature_env_escalates_degradation() {
        std::env::set_var("RUNX_REQUIRE_SIGNATURE", "1");
        let escalated = degraded("cosign is not installed");
        std::env::remove_var("RUNX_REQUIRE_SIGNATURE");

        assert!(escalated.is_err(), "must refuse to proceed when required");
        assert!(
            degraded("cosign is not installed").is_ok(),
            "default: warn and continue"
        );
    }

    /// The real signature path against the real v0.4.2 release: download the
    /// release archive and its Sigstore bundle from GitHub, verify with the
    /// pinned identity, then tamper with the archive and confirm the same
    /// verification fails. Requires `cosign` on PATH and the network; run
    /// explicitly with `cargo test -- --ignored`.
    #[test]
    #[ignore = "downloads real release assets and requires cosign"]
    fn real_release_signature_verifies_and_tampering_is_detected() {
        let token = platform_token().expect("this platform publishes releases");
        let kind = archive_kind(token);
        let ext = if kind == crate::runtime::ArchiveKind::Zip {
            "zip"
        } else {
            "tar.gz"
        };
        let asset = format!("runx-{token}.{ext}");
        let base = format!("https://github.com/aryankahar31/runx/releases/download/v0.4.2/{asset}");

        let archive = tempfile::NamedTempFile::new().expect("temp archive");
        download_file(&format!("{base}.sigstore.json"), archive.path())
            .expect("bundle pre-download sanity: URL resolves");
        download_file(&base, archive.path()).expect("archive downloads");

        verify_release_signature(archive.path(), &format!("{base}.sigstore.json"))
            .expect("the real release signature must verify");

        // Flip one byte in the archive: the same bundle must now fail.
        {
            use std::io::{Seek, Write};
            let mut file = fs::OpenOptions::new()
                .write(true)
                .open(archive.path())
                .expect("archive open");
            file.seek(std::io::SeekFrom::Start(500)).expect("seek");
            file.write_all(b"X").expect("archive write");
        }
        let tampered = verify_release_signature(archive.path(), &format!("{base}.sigstore.json"));
        assert!(
            tampered.is_err(),
            "a modified archive must fail signature verification"
        );
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

    /// 404 still returns the special "no releases" message.
    #[test]
    fn map_github_api_error_404() {
        let response = ureq::Response::new(404, "Not Found", "").expect("build test response");
        let err = ureq::Error::Status(404, response);
        let mapped = map_github_api_error(err);
        let msg = format!("{mapped:#}");
        assert!(msg.contains("No releases found"), "msg: {msg}");
    }

    /// 403 maps to an actionable UserError (not a raw ureq error).
    /// Headers can't be set on test Response in ureq; the real server provides them.
    #[test]
    fn map_github_api_error_403_is_user_error() {
        let response = ureq::Response::new(403, "Forbidden", "").expect("build test response");
        let err = ureq::Error::Status(403, response);
        let mapped = map_github_api_error(err);
        let msg = format!("{mapped:#}");
        // Should be a UserError with a clear message, not a raw "status code 403"
        assert!(
            msg.contains("Unable to check for updates") || msg.contains("access denied"),
            "msg: {msg}"
        );
        assert!(
            !msg.contains("status code 403"),
            "should not leak raw HTTP status"
        );
    }
}
