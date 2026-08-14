use crate::cache;
use crate::error::UserError;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    io::Read,
    path::{Path, PathBuf},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Number of seconds a cached Python release lookup remains valid (24 hours).
const PYTHON_CACHE_TTL_SECS: u64 = 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    TarGz,
    TarXz,
}

#[derive(Debug, Clone)]
pub struct RuntimeSpec {
    pub tool: String,
    pub version: String,
    pub url: String,
    /// URL of the checksum document used to verify the downloaded archive
    /// (Node `SHASUMS256.txt` or a python-build-standalone `.sha256` sidecar).
    ///
    /// Empty when the expected digest is carried directly in
    /// [`expected_sha256`](Self::expected_sha256) instead.
    pub checksum_url: String,
    /// The verified digest, when the publisher publishes it in the same
    /// document as the release metadata (Go's `go.dev/dl` JSON) rather than
    /// as a standalone checksum file. Takes precedence over `checksum_url`.
    pub expected_sha256: Option<String>,
    pub archive_kind: ArchiveKind,
    pub executable: String,
    pub bin_dirs: Vec<PathBuf>,
}

pub fn resolve_runtime(tool: &str, version: &str) -> Result<RuntimeSpec> {
    // Single security chokepoint. Every install path — explicit `runx.toml` and
    // auto-detection alike — reaches the filesystem and network through here,
    // so the version string is validated once, at the point where it is about
    // to be interpolated into `~/.runx/runtimes/<tool>/<version>` (which gets
    // `remove_dir_all`'d on reinstall) and into the release download URL.
    //
    // Validating in the callers instead would leave every future caller free to
    // reintroduce the traversal.
    crate::version::validate_concrete(tool, version).map_err(UserError::new)?;

    match normalized_tool(tool).as_str() {
        "node" => resolve_node(version),
        "python" => resolve_python(version),
        "bun" => resolve_bun(version),
        "go" => resolve_go(version),
        "deno" => resolve_deno(version),
        _ => Err(UserError::new(format!(
            "Unsupported runtime `{tool}`. Supported runtimes: node, python, bun, go, deno."
        ))
        .into()),
    }
}

fn normalized_tool(tool: &str) -> String {
    tool.trim().to_ascii_lowercase()
}

/// Node's platform token and archive format for the current host.
fn node_platform() -> Result<(&'static str, ArchiveKind)> {
    match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => Ok(("linux-x64", ArchiveKind::TarXz)),
        ("linux", "aarch64") => Ok(("linux-arm64", ArchiveKind::TarXz)),
        ("macos", "x86_64") => Ok(("darwin-x64", ArchiveKind::TarGz)),
        ("macos", "aarch64") => Ok(("darwin-arm64", ArchiveKind::TarGz)),
        ("windows", "x86_64") => Ok(("win-x64", ArchiveKind::Zip)),
        ("windows", "aarch64") => Ok(("win-arm64", ArchiveKind::Zip)),
        (os, arch) => {
            Err(UserError::new(format!("Node runtime is not supported on {os}/{arch}.")).into())
        }
    }
}

/// python-build-standalone's target triple for the current host.
fn python_platform() -> Result<&'static str> {
    match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        ("windows", "aarch64") => Ok("aarch64-pc-windows-msvc"),
        (os, arch) => {
            Err(UserError::new(format!("Python runtime is not supported on {os}/{arch}.")).into())
        }
    }
}

/// Bun's platform token for the current host.
///
/// The token doubles as the name of the directory inside Bun's zip archive
/// (`bun-darwin-aarch64/bun`), so [`resolve_bun`] uses it for both the URL and
/// the bin directory.
fn bun_platform() -> Result<(&'static str, ArchiveKind)> {
    match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => Ok(("linux-x64", ArchiveKind::Zip)),
        ("linux", "aarch64") => Ok(("linux-aarch64", ArchiveKind::Zip)),
        ("macos", "x86_64") => Ok(("darwin-x64", ArchiveKind::Zip)),
        ("macos", "aarch64") => Ok(("darwin-aarch64", ArchiveKind::Zip)),
        ("windows", "x86_64") => Ok(("windows-x64", ArchiveKind::Zip)),
        ("windows", "aarch64") => Ok(("windows-aarch64", ArchiveKind::Zip)),
        (os, arch) => {
            Err(UserError::new(format!("Bun runtime is not supported on {os}/{arch}.")).into())
        }
    }
}

/// Go's platform token and archive format for the current host.
fn go_platform() -> Result<(&'static str, ArchiveKind)> {
    match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => Ok(("linux-amd64", ArchiveKind::TarGz)),
        ("linux", "aarch64") => Ok(("linux-arm64", ArchiveKind::TarGz)),
        ("macos", "x86_64") => Ok(("darwin-amd64", ArchiveKind::TarGz)),
        ("macos", "aarch64") => Ok(("darwin-arm64", ArchiveKind::TarGz)),
        ("windows", "x86_64") => Ok(("windows-amd64", ArchiveKind::Zip)),
        ("windows", "aarch64") => Ok(("windows-arm64", ArchiveKind::Zip)),
        (os, arch) => {
            Err(UserError::new(format!("Go runtime is not supported on {os}/{arch}.")).into())
        }
    }
}

/// Platform key used when querying and caching a release index.
pub fn registry_platform(tool: &str) -> Result<String> {
    match normalized_tool(tool).as_str() {
        "node" => Ok(node_platform()?.0.to_string()),
        "python" => Ok(python_platform()?.to_string()),
        "bun" => Ok(bun_platform()?.0.to_string()),
        "go" => Ok(go_platform()?.0.to_string()),
        "deno" => Ok(deno_platform()?.to_string()),
        other => Err(UserError::new(format!("Unsupported runtime `{other}`.")).into()),
    }
}

fn resolve_node(version: &str) -> Result<RuntimeSpec> {
    let platform = node_platform()?;

    let ext = match platform.1 {
        ArchiveKind::Zip => "zip",
        ArchiveKind::TarGz => "tar.gz",
        ArchiveKind::TarXz => "tar.xz",
    };
    let url = format!(
        "https://nodejs.org/dist/v{version}/node-v{version}-{}.{}",
        platform.0, ext
    );
    // Node publishes SHASUMS256.txt alongside every release.
    let checksum_url = format!("https://nodejs.org/dist/v{version}/SHASUMS256.txt");

    Ok(RuntimeSpec {
        tool: "node".to_string(),
        version: version.to_string(),
        url,
        checksum_url,
        expected_sha256: None,
        archive_kind: platform.1,
        executable: executable_name("node"),
        bin_dirs: node_bin_dirs(),
    })
}

fn resolve_python(version: &str) -> Result<RuntimeSpec> {
    let platform = python_platform()?;
    let asset = find_python_asset(version, platform)?;
    Ok(RuntimeSpec {
        tool: "python".to_string(),
        version: version.to_string(),
        url: asset.url,
        checksum_url: asset.checksum_url,
        expected_sha256: None,
        archive_kind: ArchiveKind::TarGz,
        executable: executable_name("python"),
        bin_dirs: python_bin_dirs(),
    })
}

fn resolve_bun(version: &str) -> Result<RuntimeSpec> {
    let platform = bun_platform()?;
    let token = platform.0;

    Ok(RuntimeSpec {
        tool: "bun".to_string(),
        version: version.to_string(),
        url: format!(
            "https://github.com/oven-sh/bun/releases/download/bun-v{version}/bun-{token}.zip"
        ),
        // Bun publishes a Node-style `hash  name` manifest per release.
        checksum_url: format!(
            "https://github.com/oven-sh/bun/releases/download/bun-v{version}/SHASUMS256.txt"
        ),
        expected_sha256: None,
        archive_kind: platform.1,
        executable: executable_name("bun"),
        // The zip wraps the binary in a directory named after the platform
        // (`bun-darwin-aarch64/bun`), but the extractor strips that single
        // wrapping directory, so the binary lands at the runtime root.
        bin_dirs: vec![PathBuf::from(".")],
    })
}

fn resolve_go(version: &str) -> Result<RuntimeSpec> {
    let platform = go_platform()?;
    let asset = find_go_asset(version, platform.0)?;

    Ok(RuntimeSpec {
        tool: "go".to_string(),
        version: version.to_string(),
        url: asset.url,
        // Go publishes the digest in the release metadata itself, not as a
        // sidecar, so it is carried as an expected hash instead of a document.
        checksum_url: String::new(),
        expected_sha256: Some(asset.sha256),
        archive_kind: platform.1,
        executable: executable_name("go"),
        bin_dirs: vec![PathBuf::from("bin")],
    })
}

/// Number of seconds a cached Go release lookup remains valid (24 hours).
const GO_CACHE_TTL_SECS: u64 = 86_400;

/// The download URL and verified digest for one Go release file.
#[derive(Debug, Clone)]
struct GoAsset {
    url: String,
    sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedGoAsset {
    url: String,
    sha256: String,
    cached_at_secs: u64,
}

/// Cache layout: `version -> platform -> CachedGoAsset`.
type GoReleaseCache = BTreeMap<String, BTreeMap<String, CachedGoAsset>>;

/// Resolve the download URL and SHA-256 for a Go release file.
///
/// Go publishes the digest in the same document as the release metadata
/// (`go.dev/dl/?mode=json`), so resolving it means one request. The result is
/// cached in `~/.runx/go-release-cache.json` for [`GO_CACHE_TTL_SECS`] so a
/// runtime that is already installed costs no network on later runs — the same
/// pattern as the Python release cache.
fn find_go_asset(version: &str, platform: &str) -> Result<GoAsset> {
    let cache_path = cache::runx_home()?.join("go-release-cache.json");

    if let Some(asset) = read_go_cache(&cache_path, version, platform) {
        return Ok(asset);
    }

    let body = crate::http::get(crate::registry::GO_INDEX_URL)
        .call()
        .with_context(|| {
            format!(
                "Failed to fetch the Go release index from {}",
                crate::registry::GO_INDEX_URL
            )
        })?
        .into_string()
        .context("Failed to read the Go release index")?;

    let releases: Vec<crate::registry::GoRelease> =
        serde_json::from_str(&body).context("Failed to decode the Go release index")?;

    let prefix = format!("go{version}.{platform}.");
    let mut found: Option<GoAsset> = None;
    for release in releases {
        if release.version != format!("go{version}") {
            continue;
        }
        for file in &release.files {
            if !file.filename.starts_with(&prefix) {
                continue;
            }
            // The archive itself — not the .pkg installer or .msi, and not the
            // source tarball, which is what `go1.26.5.src.tar.gz` would match.
            if file.filename.contains(".src.") || file.sha256.is_empty() {
                continue;
            }
            found = Some(GoAsset {
                url: format!("https://go.dev/dl/{}", file.filename),
                sha256: file.sha256.to_ascii_lowercase(),
            });
            break;
        }
        if found.is_some() {
            break;
        }
    }

    let asset = found.ok_or_else(|| {
        UserError::new(format!(
            "No Go {version} archive found for {platform} in the go.dev release index."
        ))
    })?;

    write_go_cache(&cache_path, version, platform, &asset);
    Ok(asset)
}

/// Return a still-fresh cached Go asset for `(version, platform)`, if any.
fn read_go_cache(path: &Path, version: &str, platform: &str) -> Option<GoAsset> {
    let raw = fs::read_to_string(path).ok()?;
    let cache: GoReleaseCache = serde_json::from_str(&raw).ok()?;
    let entry = cache.get(version)?.get(platform)?;
    if now_secs().saturating_sub(entry.cached_at_secs) < GO_CACHE_TTL_SECS {
        Some(GoAsset {
            url: entry.url.clone(),
            sha256: entry.sha256.clone(),
        })
    } else {
        None
    }
}

/// Merge a resolved Go asset into the on-disk cache, preserving entries for
/// other versions/platforms. Any failure is reported as a warning and ignored
/// so a cache problem never aborts an install.
fn write_go_cache(path: &Path, version: &str, platform: &str, asset: &GoAsset) {
    if let Err(err) = try_write_go_cache(path, version, platform, asset) {
        eprintln!("Warning: failed to update Go release cache: {err}");
    }
}

/// Fallible core of [`write_go_cache`].
fn try_write_go_cache(path: &Path, version: &str, platform: &str, asset: &GoAsset) -> Result<()> {
    let mut cache: GoReleaseCache = match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => GoReleaseCache::new(),
    };

    cache.entry(version.to_string()).or_default().insert(
        platform.to_string(),
        CachedGoAsset {
            url: asset.url.clone(),
            sha256: asset.sha256.clone(),
            cached_at_secs: now_secs(),
        },
    );

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let serialized = serde_json::to_string_pretty(&cache)?;
    fs::write(path, serialized)?;
    Ok(())
}

#[derive(Debug, Deserialize)]
struct GithubRelease {
    assets: Vec<GithubAsset>,
}

#[derive(Debug, Clone, Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

/// A resolved Python archive together with the URL of its checksum document.
#[derive(Debug, Clone)]
struct PythonAsset {
    url: String,
    checksum_url: String,
}

/// The checksum document URL for `archive_name` within `release`: the
/// per-archive `.sha256` sidecar when published, else the combined
/// `SHA256SUMS` manifest newer releases ship instead.
fn checksum_url_for(release: &GithubRelease, archive_name: &str) -> Option<String> {
    release
        .assets
        .iter()
        .find(|asset| asset.name == format!("{archive_name}.sha256"))
        .or_else(|| {
            release
                .assets
                .iter()
                .find(|asset| asset.name == "SHA256SUMS")
        })
        .map(|asset| asset.browser_download_url.clone())
}

/// On-disk cache entry for a resolved Python asset.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedPythonAsset {
    url: String,
    checksum_url: String,
    cached_at_secs: u64,
}

/// Cache layout: `version -> platform -> CachedPythonAsset`.
type PythonReleaseCache = BTreeMap<String, BTreeMap<String, CachedPythonAsset>>;

/// Resolve the download URL and checksum sidecar for a portable Python build.
///
/// Results are cached in `~/.runx/python-release-cache.json` for
/// [`PYTHON_CACHE_TTL_SECS`] to avoid exhausting GitHub's unauthenticated API
/// rate limit (60 requests/hour). A fresh cache hit returns without any network
/// request; otherwise the GitHub API is paginated and the resolved asset is
/// merged back into the cache.
fn find_python_asset(version: &str, platform: &str) -> Result<PythonAsset> {
    let cache_path = cache::runx_home()?.join("python-release-cache.json");

    // 1. Fresh cache hit — no API call.
    if let Some(asset) = read_python_cache(&cache_path, version, platform) {
        return Ok(asset);
    }

    // 2. Paginate GitHub API, stopping as soon as a match is found.
    let prefix = format!("cpython-{version}+");
    for page in 1..=20 {
        let url = format!(
            "https://api.github.com/repos/astral-sh/python-build-standalone/releases?per_page=10&page={page}"
        );
        let releases = fetch_python_release_page(&url)?;

        if releases.is_empty() {
            break;
        }

        for release in releases {
            let Some(archive) = release.assets.iter().find(|asset| {
                asset.name.starts_with(&prefix)
                    && asset.name.contains(platform)
                    && asset.name.contains("install_only")
                    && asset.name.ends_with(".tar.gz")
            }) else {
                continue;
            };

            // Locate the checksum document for the archive in the same release.
            // Historically every archive had a `.sha256` sidecar; newer
            // releases dropped those for a single combined `SHA256SUMS`
            // manifest, which the checksum verifier already parses.
            let checksum_url = checksum_url_for(&release, &archive.name).ok_or_else(|| {
                let sidecar_name = format!("{}.sha256", archive.name);
                UserError::new(format!(
                    "Found Python {version} archive for {platform} but neither the \
                     `{sidecar_name}` sidecar nor a `SHA256SUMS` manifest in the release."
                ))
            })?;

            let asset = PythonAsset {
                url: archive.browser_download_url.clone(),
                checksum_url,
            };

            // 3. Persist to cache. Failure here must not abort the install.
            write_python_cache(&cache_path, version, platform, &asset);

            return Ok(asset);
        }
    }

    Err(UserError::new(format!(
        "No portable Python {version} archive found for {platform} in python-build-standalone releases."
    ))
    .into())
}

/// Return a still-fresh cached Python asset for `(version, platform)`, if any.
fn read_python_cache(path: &Path, version: &str, platform: &str) -> Option<PythonAsset> {
    let raw = fs::read_to_string(path).ok()?;
    let cache: PythonReleaseCache = serde_json::from_str(&raw).ok()?;
    let entry = cache.get(version)?.get(platform)?;
    if now_secs().saturating_sub(entry.cached_at_secs) < PYTHON_CACHE_TTL_SECS {
        Some(PythonAsset {
            url: entry.url.clone(),
            checksum_url: entry.checksum_url.clone(),
        })
    } else {
        None
    }
}

/// Merge a resolved Python asset into the on-disk cache, preserving entries for
/// other versions/platforms. Any failure is reported as a warning and ignored
/// so a cache problem never aborts an install.
fn write_python_cache(path: &Path, version: &str, platform: &str, asset: &PythonAsset) {
    if let Err(err) = try_write_python_cache(path, version, platform, asset) {
        eprintln!("Warning: failed to update Python release cache: {err}");
    }
}

/// Fallible core of [`write_python_cache`].
fn try_write_python_cache(
    path: &Path,
    version: &str,
    platform: &str,
    asset: &PythonAsset,
) -> Result<()> {
    let mut cache: PythonReleaseCache = match fs::read_to_string(path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => PythonReleaseCache::new(),
    };

    cache.entry(version.to_string()).or_default().insert(
        platform.to_string(),
        CachedPythonAsset {
            url: asset.url.clone(),
            checksum_url: asset.checksum_url.clone(),
            cached_at_secs: now_secs(),
        },
    );

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let serialized = serde_json::to_string_pretty(&cache)?;
    fs::write(path, serialized)?;
    Ok(())
}

/// Current Unix timestamp in seconds (saturating to 0 before the epoch).
fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn fetch_python_release_page(url: &str) -> Result<Vec<GithubRelease>> {
    let mut last_error: Option<anyhow::Error> = None;
    for attempt in 1..=3 {
        match crate::http::get(url).call() {
            Ok(response) => {
                // Read the body via into_reader, not into_string: ureq's
                // into_string caps the body at a hardcoded 10 MiB (each page
                // here is ~18 MiB), panics through into_json on a body that
                // fails mid-transfer (see ureq's own "TODO: This expect can
                // actually panic" in response.rs), and a server that closes
                // early is exactly the transient failure the retry loop exists
                // for.
                let parse = (|| -> Result<Vec<GithubRelease>> {
                    let mut raw = String::new();
                    response
                        .into_reader()
                        .read_to_string(&mut raw)
                        .with_context(|| {
                            "Failed to read python-build-standalone release metadata"
                        })?;
                    serde_json::from_str(&raw).context("Failed to decode release metadata")
                })();
                match parse {
                    Ok(releases) => return Ok(releases),
                    Err(err) => {
                        last_error = Some(err);
                        if attempt < 3 {
                            thread::sleep(Duration::from_secs(attempt));
                        }
                    }
                }
            }
            Err(err) => {
                last_error = Some(err.into());
                if attempt < 3 {
                    thread::sleep(Duration::from_secs(attempt));
                }
            }
        }
    }

    let err = last_error
        .map(|err| err.to_string())
        .unwrap_or_else(|| "unknown error".to_string());
    Err(anyhow::anyhow!(
        "Failed to query python-build-standalone releases at {url}: {err}"
    ))
}

fn executable_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// Deno's platform token for the current host — the same target triples
/// python-build-standalone uses, which Deno also publishes.
fn deno_platform() -> Result<&'static str> {
    match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("windows", "x86_64") => Ok("x86_64-pc-windows-msvc"),
        ("windows", "aarch64") => Ok("aarch64-pc-windows-msvc"),
        (os, arch) => {
            Err(UserError::new(format!("Deno runtime is not supported on {os}/{arch}.")).into())
        }
    }
}

/// Deno publishes one zip per platform on GitHub Releases (`v2.9.6` tags),
/// each containing a single executable at the archive root, plus a per-asset
/// `.sha256sum` sidecar whose single `hash  filename` line the downloader
/// already matches exactly.
///
/// The sidecar exists only from v2.0.1: the 1.x line and v2.0.0 ship no
/// archive checksum (v2.0.0 carries a bare-binary hash, which cannot verify
/// the zip pre-extraction), so those versions install TLS-only, with a
/// warning from the downloader. Verified against the live release assets of
/// 1.46.0, 2.0.0, 2.0.1 and 2.9.5.
fn resolve_deno(version: &str) -> Result<RuntimeSpec> {
    let platform = deno_platform()?;
    let url = format!(
        "https://github.com/denoland/deno/releases/download/v{version}/deno-{platform}.zip"
    );
    let has_sidecar = crate::version::Version::parse(version)
        .is_some_and(|v| v >= crate::version::Version::parse("2.0.1").expect("static min"));
    let checksum_url = if has_sidecar {
        format!("{url}.sha256sum")
    } else {
        String::new()
    };
    Ok(RuntimeSpec {
        tool: "deno".to_string(),
        version: version.to_string(),
        url,
        checksum_url,
        expected_sha256: None,
        archive_kind: ArchiveKind::Zip,
        executable: executable_name("deno"),
        bin_dirs: vec![PathBuf::from(".")],
    })
}

fn node_bin_dirs() -> Vec<PathBuf> {
    if cfg!(windows) {
        vec![PathBuf::from(".")]
    } else {
        vec![PathBuf::from("bin")]
    }
}

fn python_bin_dirs() -> Vec<PathBuf> {
    if cfg!(windows) {
        vec![PathBuf::from("."), PathBuf::from("Scripts")]
    } else {
        vec![PathBuf::from("bin")]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        },
        thread,
        time::{Duration, Instant},
    };

    // ── Bun spec resolution ───────────────────────────────────────────────────

    #[test]
    fn bun_spec_uses_github_release_urls_and_zip_kind() {
        let spec = resolve_runtime("bun", "1.3.14").expect("bun resolves");

        assert_eq!(spec.tool, "bun");
        assert_eq!(spec.version, "1.3.14");
        assert_eq!(spec.archive_kind, ArchiveKind::Zip);
        assert!(
            spec.expected_sha256.is_none(),
            "bun verifies via SHASUMS256.txt"
        );
        assert!(
            spec.url.ends_with("/bun-v1.3.14/bun-darwin-aarch64.zip")
                || spec.url.ends_with("/bun-v1.3.14/bun-darwin-x64.zip")
                || spec.url.ends_with("/bun-v1.3.14/bun-linux-x64.zip")
                || spec.url.ends_with("/bun-v1.3.14/bun-windows-x64.zip"),
            "unexpected bun url: {}",
            spec.url
        );
        assert!(
            spec.checksum_url.ends_with("/bun-v1.3.14/SHASUMS256.txt"),
            "unexpected bun checksum url: {}",
            spec.checksum_url
        );
        assert!(
            spec.executable.starts_with("bun"),
            "executable should be bun, got {}",
            spec.executable
        );
        assert_eq!(spec.bin_dirs, vec![PathBuf::from(".")]);
        assert_eq!(spec.executable, executable_name("bun"));
    }

    // ── Deno spec resolution ─────────────────────────────────────────────────

    #[test]
    fn deno_spec_uses_denoland_github_release_urls_and_zip_kind() {
        let spec = resolve_runtime("deno", "2.9.6").expect("deno resolves");

        assert_eq!(spec.tool, "deno");
        assert_eq!(spec.version, "2.9.6");
        assert_eq!(spec.archive_kind, ArchiveKind::Zip);
        assert!(
            spec.expected_sha256.is_none(),
            "deno verifies via a .sha256sum sidecar"
        );
        let platform = deno_platform().expect("deno platform");
        assert_eq!(
            spec.url,
            format!(
                "https://github.com/denoland/deno/releases/download/v2.9.6/deno-{platform}.zip"
            )
        );
        assert_eq!(
            spec.checksum_url,
            format!("{}.sha256sum", spec.url),
            "deno publishes one hash  name sidecar per asset"
        );
        assert_eq!(spec.executable, executable_name("deno"));
        assert_eq!(spec.bin_dirs, vec![PathBuf::from(".")]);
    }

    /// Deno release tags are prefixed `v`; the version must not leak into
    /// the URL unvalidated, and must be a full three-part pin.
    #[test]
    fn deno_spec_rejects_unvalidated_versions() {
        for bad in ["../../etc", "2.9.6 && rm -rf /", "v2.9.6", "2.9"] {
            assert!(
                resolve_runtime("deno", bad).is_err(),
                "{bad:?} must not resolve to a deno spec"
            );
        }
    }

    /// The per-asset sidecar exists only from v2.0.1; older releases must not
    /// reference a sidecar that 404s (the v1.46.0 regression) and must fall
    /// back to TLS-only instead.
    #[test]
    fn deno_legacy_versions_fall_back_to_tls_only() {
        for legacy in ["1.46.0", "1.48.0", "2.0.0"] {
            let spec = resolve_runtime("deno", legacy).expect("legacy deno resolves");
            assert_eq!(
                spec.checksum_url, "",
                "{legacy} publishes no archive checksum and must not fetch one"
            );
        }
        for modern in ["2.0.1", "2.9.6"] {
            let spec = resolve_runtime("deno", modern).expect("modern deno resolves");
            assert!(
                spec.checksum_url.ends_with(".zip.sha256sum"),
                "{modern} should verify against the sidecar, got {}",
                spec.checksum_url
            );
        }
    }

    /// Bun release tags are prefixed `bun-v`; the version must not leak into
    /// the URL unvalidated.
    #[test]
    fn bun_spec_rejects_unvalidated_versions() {
        for bad in ["../../etc", "1.3.14 && rm -rf /", "v1.3.14", "1.3"] {
            assert!(
                resolve_runtime("bun", bad).is_err(),
                "{bad:?} must not resolve to a bun spec"
            );
        }
    }

    // ── Go spec resolution ────────────────────────────────────────────────────

    #[test]
    fn go_spec_uses_golang_org_urls_and_bin_dir() {
        let spec = resolve_runtime("go", "1.26.5").expect("go resolves");

        assert_eq!(spec.tool, "go");
        assert_eq!(spec.version, "1.26.5");
        assert_eq!(spec.executable, executable_name("go"));
        assert_eq!(spec.bin_dirs, vec![PathBuf::from("bin")]);
        // The URL names the platform file, whichever host we run on.
        let filename = spec.url.rsplit('/').next().expect("url has a filename");
        assert!(
            filename.starts_with("go1.26.5."),
            "unexpected go url: {}",
            spec.url
        );
        match env::consts::OS {
            "linux" => assert!(filename.contains(".linux-"), "got {filename}"),
            "macos" => assert!(filename.contains(".darwin-"), "got {filename}"),
            "windows" => assert!(filename.contains(".windows-"), "got {filename}"),
            _ => {}
        }
        assert!(
            spec.checksum_url.is_empty(),
            "go has no checksum document; the digest is carried directly"
        );
        // The digest is resolved from the network; a pure spec check must not
        // assert on it, only that the field exists for a fetched asset.
        let _ = spec.expected_sha256;
    }

    // ── Go release cache ──────────────────────────────────────────────────────

    #[test]
    fn go_cache_round_trips_and_respects_the_ttl() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join("go-release-cache.json");
        let asset = GoAsset {
            url: "https://go.dev/dl/go1.26.5.darwin-arm64.tar.gz".to_string(),
            sha256: "abcd".repeat(16),
        };

        write_go_cache(&path, "1.26.5", "darwin-arm64", &asset);
        let loaded =
            read_go_cache(&path, "1.26.5", "darwin-arm64").expect("fresh cache entry should hit");
        assert_eq!(loaded.url, asset.url);
        assert_eq!(loaded.sha256, asset.sha256);

        // Other platforms are preserved and a stale entry is a miss.
        assert!(read_go_cache(&path, "1.26.5", "linux-amd64").is_none());
        let mut cache: GoReleaseCache =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).expect("cache parses");
        cache
            .get_mut("1.26.5")
            .unwrap()
            .get_mut("darwin-arm64")
            .unwrap()
            .cached_at_secs = now_secs() - GO_CACHE_TTL_SECS - 1;
        fs::write(&path, serde_json::to_string(&cache).unwrap()).unwrap();
        assert!(read_go_cache(&path, "1.26.5", "darwin-arm64").is_none());
    }

    // ── Python checksum lookup ────────────────────────────────────────────────

    fn release(assets: &[(&str, &str)]) -> GithubRelease {
        GithubRelease {
            assets: assets
                .iter()
                .map(|(name, url)| GithubAsset {
                    name: name.to_string(),
                    browser_download_url: url.to_string(),
                })
                .collect(),
        }
    }

    #[test]
    fn prefers_the_per_archive_sidecar_over_the_combined_manifest() {
        let rel = release(&[
            (
                "cpython-3.14.6+20260728-aarch64-apple-darwin-install_only.tar.gz",
                "https://example.invalid/a.tar.gz",
            ),
            (
                "cpython-3.14.6+20260728-aarch64-apple-darwin-install_only.tar.gz.sha256",
                "https://example.invalid/a.sha256",
            ),
            ("SHA256SUMS", "https://example.invalid/SHA256SUMS"),
        ]);

        assert_eq!(
            checksum_url_for(
                &rel,
                "cpython-3.14.6+20260728-aarch64-apple-darwin-install_only.tar.gz"
            )
            .as_deref(),
            Some("https://example.invalid/a.sha256")
        );
    }

    /// Newer python-build-standalone releases dropped the sidecars; the
    /// combined manifest must be picked up instead, or installs of current
    /// releases fail with "no matching checksum".
    #[test]
    fn falls_back_to_the_combined_sha256sums_manifest() {
        let rel = release(&[
            (
                "cpython-3.14.6+20260728-aarch64-apple-darwin-install_only.tar.gz",
                "https://example.invalid/a.tar.gz",
            ),
            ("SHA256SUMS", "https://example.invalid/SHA256SUMS"),
        ]);

        assert_eq!(
            checksum_url_for(
                &rel,
                "cpython-3.14.6+20260728-aarch64-apple-darwin-install_only.tar.gz"
            )
            .as_deref(),
            Some("https://example.invalid/SHA256SUMS")
        );

        let bare = release(&[(
            "cpython-3.14.6+20260728-aarch64-apple-darwin-install_only.tar.gz",
            "https://example.invalid/a.tar.gz",
        )]);
        assert_eq!(
            checksum_url_for(
                &bare,
                "cpython-3.14.6+20260728-aarch64-apple-darwin-install_only.tar.gz"
            ),
            None
        );
    }

    #[test]
    fn retries_on_decode_failure_before_succeeding() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        listener
            .set_nonblocking(true)
            .expect("configure nonblocking listener");
        let addr = listener.local_addr().expect("listener address");
        let request_count = Arc::new(AtomicUsize::new(0));
        let request_count_for_server = Arc::clone(&request_count);

        let server = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut served = 0usize;

            while served < 2 && Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        served += 1;
                        request_count_for_server.fetch_add(1, Ordering::SeqCst);

                        let mut request = [0u8; 1024];
                        let _ = stream.read(&mut request);

                        let body = if served == 1 {
                            r#"[{"assets":[{"name":"cpython-3.11.7+"#
                        } else {
                            r#"[{"assets":[{"name":"cpython-3.11.7+x86_64-unknown-linux-gnu-install_only.tar.gz","browser_download_url":"https://example.invalid/python.tar.gz"}]}]"#
                        };
                        let response = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                            body.len(),
                            body
                        );
                        stream
                            .write_all(response.as_bytes())
                            .expect("write test response");
                        stream.flush().expect("flush test response");

                        // Keep the socket open until the client closes it:
                        // dropping it here can race ureq's body read, which
                        // panics on macOS instead of surfacing an error.
                        let mut drain = [0u8; 64];
                        while stream.read(&mut drain).is_ok_and(|read| read > 0) {}
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(10));
                    }
                    Err(err) => panic!("listener accept failed: {err}"),
                }
            }

            assert_eq!(served, 2, "expected two requests to reach the test server");
        });

        let url = format!("http://{addr}/releases");
        let releases = fetch_python_release_page(&url).expect("request should retry and succeed");

        server.join().expect("server thread should exit cleanly");
        assert_eq!(request_count.load(Ordering::SeqCst), 2);
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].assets.len(), 1);
    }
}
