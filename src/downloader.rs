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

/// A verified download: the archive plus the digest that was checked.
///
/// The digest is returned rather than discarded so `runx.lock` can record what
/// was actually installed, instead of re-hashing the file or trusting a second
/// fetch of the checksum document.
pub struct Download {
    pub temp: NamedTempFile,
    pub sha256: String,
}

impl Download {
    /// Path of the downloaded archive.
    pub fn path(&self) -> &Path {
        self.temp.path()
    }
}

/// Download `url` to a temporary file, verify its SHA-256 against the checksum
/// document published at `checksum_url`, and return the verified archive.
///
/// The inner [`NamedTempFile`] auto-deletes on drop (including on panic or
/// early return), so the caller should extract from `path()` and then drop it.
/// The checksum is verified *before* returning — a file that fails verification
/// is never handed back for extraction.
pub fn download_to_temp(url: &str, checksum_url: &str) -> Result<Download> {
    println!("Downloading {url}");
    let response = crate::http::get(url)
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

    let sha256 = verify_checksum(url, checksum_url, &temp)?;

    // Do NOT call `.keep()` — returning the NamedTempFile lets it auto-delete on
    // drop, so a killed process never leaks the file on disk.
    Ok(Download { temp, sha256 })
}

/// Verify the SHA-256 of `temp` against the expected hash published at
/// `checksum_url`, returning the verified digest.
///
/// Returns an error on mismatch without extracting anything.
fn verify_checksum(url: &str, checksum_url: &str, temp: &NamedTempFile) -> Result<String> {
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
    Ok(actual)
}

/// Fetch a checksum document (Node `SHASUMS256.txt` or a python
/// `.sha256` sidecar) into memory rather than to disk.
fn fetch_checksum_document(checksum_url: &str) -> Result<String> {
    crate::http::get(checksum_url)
        .call()
        .with_context(|| format!("Failed to download checksum from {checksum_url}"))?
        .into_string()
        .with_context(|| format!("Failed to read checksum body from {checksum_url}"))
}

/// A SHA-256 digest is exactly 64 hex characters.
const SHA256_HEX_LEN: usize = 64;

/// True if `token` looks like a SHA-256 hex digest.
fn is_sha256_hex(token: &str) -> bool {
    token.len() == SHA256_HEX_LEN && token.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Extract the expected hex hash for `filename` from a checksum document.
///
/// Two formats are supported:
///
/// * A manifest of `hash  filename` lines (Node's `SHASUMS256.txt`). The
///   filename must match **exactly**; `hash *filename` (coreutils binary-mode
///   marker) is also accepted.
/// * A sidecar containing a single bare digest (python-build-standalone's
///   `.sha256` files).
///
/// Returns `None` rather than falling back to an unrelated hash. The previous
/// implementation matched with `line.contains(filename)` and, on failure, took
/// the first token of the first line — so a manifest that did not list our
/// archive at all still yielded *some* hash, and a substring match could pick
/// the digest of a different artifact whose name merely contained ours (e.g.
/// `...tar.gz` also matching the `...tar.gz.asc` line). Both produce a
/// confusing "SHA-256 mismatch" for what is really a lookup failure.
fn extract_expected_hash(document: &str, filename: &str) -> Option<String> {
    let mut saw_manifest_line = false;

    for line in document.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let mut tokens = line.split_whitespace();
        let Some(hash) = tokens.next() else {
            continue;
        };

        match tokens.next() {
            // `hash  name` — a manifest entry.
            Some(name) => {
                saw_manifest_line = true;
                // Strip the coreutils binary-mode marker, then compare the
                // final path component so a manifest listing `./node-...` or
                // `dist/node-...` still matches.
                let name = name.trim_start_matches('*');
                let basename = name.rsplit(['/', '\\']).next().unwrap_or(name);
                if basename == filename && is_sha256_hex(hash) {
                    return Some(hash.to_ascii_lowercase());
                }
            }
            // A lone token: only usable as a bare sidecar digest.
            None => {
                if !saw_manifest_line && is_sha256_hex(hash) {
                    return Some(hash.to_ascii_lowercase());
                }
            }
        }
    }

    None
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

#[cfg(test)]
mod tests {
    use super::{extract_expected_hash, is_sha256_hex};

    const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn finds_hash_in_node_style_manifest() {
        let document = format!(
            "{HASH_A}  node-v20.11.0-linux-x64.tar.xz\n\
             {HASH_B}  node-v20.11.0-darwin-arm64.tar.gz\n"
        );
        assert_eq!(
            extract_expected_hash(&document, "node-v20.11.0-darwin-arm64.tar.gz").as_deref(),
            Some(HASH_B)
        );
    }

    /// A manifest that does not list our archive must yield `None`, not the
    /// first hash it happens to contain. Returning an unrelated hash turns a
    /// lookup failure into a bogus "SHA-256 mismatch".
    #[test]
    fn missing_entry_returns_none_instead_of_an_unrelated_hash() {
        let document = format!("{HASH_A}  some-other-file.tar.gz\n");
        assert_eq!(
            extract_expected_hash(&document, "node-v20.11.0-linux-x64.tar.xz"),
            None
        );
    }

    /// The old matcher used `line.contains(filename)`, so the digest of a
    /// signature or a longer sibling name could be picked for the archive.
    #[test]
    fn does_not_match_on_substring_of_a_longer_filename() {
        let document = format!(
            "{HASH_A}  node-v20.11.0-linux-x64.tar.xz.asc\n\
             {HASH_B}  node-v20.11.0-linux-x64.tar.xz\n"
        );
        assert_eq!(
            extract_expected_hash(&document, "node-v20.11.0-linux-x64.tar.xz").as_deref(),
            Some(HASH_B),
            "must match the archive line, not the .asc line"
        );
    }

    #[test]
    fn reads_bare_sidecar_digest() {
        assert_eq!(
            extract_expected_hash(&format!("{HASH_A}\n"), "cpython-3.11.7.tar.gz").as_deref(),
            Some(HASH_A)
        );
    }

    #[test]
    fn accepts_binary_mode_marker_and_path_prefixes() {
        let starred = format!("{HASH_A} *runx-linux-x64.tar.gz\n");
        assert_eq!(
            extract_expected_hash(&starred, "runx-linux-x64.tar.gz").as_deref(),
            Some(HASH_A)
        );

        let prefixed = format!("{HASH_A}  ./dist/runx-linux-x64.tar.gz\n");
        assert_eq!(
            extract_expected_hash(&prefixed, "runx-linux-x64.tar.gz").as_deref(),
            Some(HASH_A)
        );
    }

    #[test]
    fn normalises_uppercase_digests() {
        let document = format!("{}  file.tar.gz\n", HASH_A.to_uppercase());
        assert_eq!(
            extract_expected_hash(&document, "file.tar.gz").as_deref(),
            Some(HASH_A)
        );
    }

    #[test]
    fn ignores_comments_and_blank_lines() {
        let document = format!("# checksums\n\n{HASH_A}  file.tar.gz\n");
        assert_eq!(
            extract_expected_hash(&document, "file.tar.gz").as_deref(),
            Some(HASH_A)
        );
    }

    /// An HTML error page served instead of a manifest must not be mistaken for
    /// a digest.
    #[test]
    fn rejects_non_digest_documents() {
        assert_eq!(
            extract_expected_hash("<html>404</html>", "file.tar.gz"),
            None
        );
        assert_eq!(
            extract_expected_hash("not-a-hash  file.tar.gz", "file.tar.gz"),
            None
        );
        assert_eq!(extract_expected_hash("", "file.tar.gz"), None);
    }

    #[test]
    fn recognises_only_well_formed_digests() {
        assert!(is_sha256_hex(HASH_A));
        assert!(!is_sha256_hex("abc"));
        assert!(!is_sha256_hex(&format!("{HASH_A}a")));
        assert!(!is_sha256_hex(&"z".repeat(64)));
    }
}
