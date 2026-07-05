use crate::cache;
use crate::error::UserError;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Compile-time User-Agent string derived from the crate version, so it never
/// goes stale as the crate is bumped.
const USER_AGENT: &str = concat!("runx/", env!("CARGO_PKG_VERSION"));

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
    pub checksum_url: String,
    pub archive_kind: ArchiveKind,
    pub executable: String,
    pub bin_dirs: Vec<PathBuf>,
}

pub fn resolve_runtime(tool: &str, version: &str) -> Result<RuntimeSpec> {
    match normalized_tool(tool).as_str() {
        "node" => resolve_node(version),
        "python" => resolve_python(version),
        _ => Err(UserError::new(format!(
            "Unsupported runtime `{tool}`. Supported runtimes: node, python."
        ))
        .into()),
    }
}

fn normalized_tool(tool: &str) -> String {
    tool.trim().to_ascii_lowercase()
}

fn resolve_node(version: &str) -> Result<RuntimeSpec> {
    let platform = match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => ("linux-x64", ArchiveKind::TarXz),
        ("linux", "aarch64") => ("linux-arm64", ArchiveKind::TarXz),
        ("macos", "x86_64") => ("darwin-x64", ArchiveKind::TarGz),
        ("macos", "aarch64") => ("darwin-arm64", ArchiveKind::TarGz),
        ("windows", "x86_64") => ("win-x64", ArchiveKind::Zip),
        ("windows", "aarch64") => ("win-arm64", ArchiveKind::Zip),
        (os, arch) => {
            return Err(
                UserError::new(format!("Node runtime is not supported on {os}/{arch}.")).into(),
            )
        }
    };

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
        archive_kind: platform.1,
        executable: executable_name("node"),
        bin_dirs: node_bin_dirs(),
    })
}

fn resolve_python(version: &str) -> Result<RuntimeSpec> {
    let platform = match (env::consts::OS, env::consts::ARCH) {
        ("linux", "x86_64") => "x86_64-unknown-linux-gnu",
        ("linux", "aarch64") => "aarch64-unknown-linux-gnu",
        ("macos", "x86_64") => "x86_64-apple-darwin",
        ("macos", "aarch64") => "aarch64-apple-darwin",
        ("windows", "x86_64") => "x86_64-pc-windows-msvc",
        ("windows", "aarch64") => "aarch64-pc-windows-msvc",
        (os, arch) => {
            return Err(
                UserError::new(format!("Python runtime is not supported on {os}/{arch}.")).into(),
            )
        }
    };

    let asset = find_python_asset(version, platform)?;
    Ok(RuntimeSpec {
        tool: "python".to_string(),
        version: version.to_string(),
        url: asset.url,
        checksum_url: asset.checksum_url,
        archive_kind: ArchiveKind::TarGz,
        executable: executable_name("python"),
        bin_dirs: python_bin_dirs(),
    })
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

/// A resolved Python archive together with the URL of its `.sha256` sidecar.
#[derive(Debug, Clone)]
struct PythonAsset {
    url: String,
    checksum_url: String,
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

            // Locate the `.sha256` sidecar for the archive in the same release.
            let sidecar_name = format!("{}.sha256", archive.name);
            let checksum_url = release
                .assets
                .iter()
                .find(|asset| asset.name == sidecar_name)
                .map(|asset| asset.browser_download_url.clone())
                .ok_or_else(|| {
                    UserError::new(format!(
                        "Found Python {version} archive for {platform} but no matching \
                         `{sidecar_name}` checksum sidecar in the release."
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
        match ureq::get(url).set("User-Agent", USER_AGENT).call() {
            Ok(response) => {
                match response
                    .into_json()
                    .with_context(|| "Failed to decode python-build-standalone release metadata")
                {
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
    use super::fetch_python_release_page;
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
