use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressStyle};
use sha2::{Digest, Sha256};
use std::{
    fmt::Write as _,
    fs::File,
    io::{Read, Write},
    path::Path,
};
use tempfile::NamedTempFile;

/// Compile-time User-Agent string derived from the crate version, so it never
/// goes stale as the crate is bumped.
const USER_AGENT: &str = concat!("runx/", env!("CARGO_PKG_VERSION"));

/// Download `url` to a temporary file, verify its SHA-256 against the checksum
/// document published at `checksum_url`, and return the verified temp file.
///
/// The returned [`NamedTempFile`] auto-deletes on drop (including on panic or
/// early return), so the caller should extract from `temp.path()` and then drop
/// it. The checksum is verified *before* returning — a file that fails
/// verification is never handed back for extraction.
pub fn download_to_temp(url: &str, checksum_url: &str) -> Result<NamedTempFile> {
    println!("Downloading {url}");
    let response = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .with_context(|| format!("Failed to download {url}"))?;

    let total = response
        .header("Content-Length")
        .and_then(|value| value.parse::<u64>().ok());
    let progress = match total {
        Some(bytes) => ProgressBar::new(bytes),
        None => ProgressBar::new_spinner(),
    };
    progress.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} {bytes}/{total_bytes} [{bar:40.cyan/blue}] {bytes_per_sec}",
        )
        .unwrap_or_else(|_| ProgressStyle::default_bar()),
    );

    let mut temp = NamedTempFile::new().context("Failed to create temporary download file")?;
    let mut reader = response.into_reader();
    copy_with_progress(&mut reader, temp.as_file_mut(), &progress)?;
    progress.finish_and_clear();

    verify_checksum(url, checksum_url, &temp)?;

    // Do NOT call `.keep()` — returning the NamedTempFile lets it auto-delete on
    // drop, so a killed process never leaks the file on disk.
    Ok(temp)
}

/// Verify the SHA-256 of `temp` against the expected hash published at
/// `checksum_url`. Returns an error on mismatch without extracting anything.
fn verify_checksum(url: &str, checksum_url: &str, temp: &NamedTempFile) -> Result<()> {
    let filename = url.rsplit('/').next().unwrap_or(url);
    let document = fetch_checksum_document(checksum_url)?;
    let expected = extract_expected_hash(&document, filename).ok_or_else(|| {
        anyhow::anyhow!("Could not find a SHA-256 hash for {filename} in {checksum_url}")
    })?;

    let actual = compute_sha256(temp.path())?;
    if !actual.eq_ignore_ascii_case(&expected) {
        anyhow::bail!("SHA-256 mismatch for {filename}: expected {expected}, got {actual}");
    }
    println!("✓ Checksum verified");
    Ok(())
}

/// Fetch a checksum document (Node `SHASUMS256.txt` or a python
/// `.sha256` sidecar) into memory rather than to disk.
fn fetch_checksum_document(checksum_url: &str) -> Result<String> {
    ureq::get(checksum_url)
        .set("User-Agent", USER_AGENT)
        .call()
        .with_context(|| format!("Failed to download checksum from {checksum_url}"))?
        .into_string()
        .with_context(|| format!("Failed to read checksum body from {checksum_url}"))
}

/// Extract the expected hex hash for `filename` from a checksum document.
///
/// Supports both Node's `SHASUMS256.txt` (many `hash  filename` lines) and
/// python-build-standalone `.sha256` sidecars (a single hash, optionally
/// followed by a filename).
fn extract_expected_hash(document: &str, filename: &str) -> Option<String> {
    // Node style: locate the line that names this exact archive.
    for line in document.lines() {
        if line.contains(filename) {
            if let Some(token) = line.split_whitespace().next() {
                return Some(token.to_string());
            }
        }
    }
    // Python sidecar style: first token of the first non-empty line.
    document
        .lines()
        .find(|line| !line.trim().is_empty())
        .and_then(|line| line.split_whitespace().next())
        .map(|token| token.to_string())
}

/// Compute the lowercase hex SHA-256 digest of the file at `path`.
fn compute_sha256(path: &Path) -> Result<String> {
    let mut file = File::open(path)
        .with_context(|| format!("Failed to open {} for hashing", path.display()))?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let bytes = file
            .read(&mut buffer)
            .context("Failed while reading download for hashing")?;
        if bytes == 0 {
            break;
        }
        hasher.update(&buffer[..bytes]);
    }
    Ok(hex_encode(&hasher.finalize()))
}

/// Encode bytes as a lowercase hex string.
fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

fn copy_with_progress(
    reader: &mut dyn Read,
    writer: &mut File,
    progress: &ProgressBar,
) -> Result<()> {
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let bytes = reader
            .read(&mut buffer)
            .context("Failed while reading download stream")?;
        if bytes == 0 {
            break;
        }
        writer
            .write_all(&buffer[..bytes])
            .context("Failed while writing download file")?;
        progress.inc(bytes as u64);
    }
    Ok(())
}
