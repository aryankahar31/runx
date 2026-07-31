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
#[derive(Debug)]
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

/// Download `url` to a temporary file and verify its SHA-256, then return the
/// verified archive.
///
/// The digest is verified either against the checksum document published at
/// `checksum_url`, or — when `expected_sha256` is given — directly against
/// that digest, for publishers who list it in the release metadata itself
/// (Go's `go.dev/dl` JSON). At most one source is consulted; when
/// `expected_sha256` is set, `checksum_url` is ignored.
///
/// The inner [`NamedTempFile`] auto-deletes on drop (including on panic or
/// early return), so the caller should extract from `path()` and then drop it.
/// The checksum is verified *before* returning — a file that fails verification
/// is never handed back for extraction.
pub fn download_to_temp(
    url: &str,
    checksum_url: &str,
    expected_sha256: Option<&str>,
) -> Result<Download> {
    println!("Downloading {url}");

    let mut temp = NamedTempFile::new().context("Failed to create temporary download file")?;
    let mut have: u64 = 0;
    let mut last_error: Option<anyhow::Error> = None;

    for attempt in 0..crate::http::MAX_ATTEMPTS {
        if attempt > 0 {
            let delay = crate::http::backoff_delay(attempt - 1);
            if have > 0 {
                eprintln!(
                    "  Download interrupted after {}; resuming in {:.1}s (attempt {}/{})",
                    crate::cache::format_size(have),
                    delay.as_secs_f64(),
                    attempt + 1,
                    crate::http::MAX_ATTEMPTS
                );
            } else {
                eprintln!(
                    "  Download failed; retrying in {:.1}s (attempt {}/{})",
                    delay.as_secs_f64(),
                    attempt + 1,
                    crate::http::MAX_ATTEMPTS
                );
            }
            std::thread::sleep(delay);
        }

        match fetch_into(url, &mut temp, have) {
            Ok(()) => {
                let sha256 = verify_checksum(url, checksum_url, expected_sha256, &temp)?;
                // Do NOT call `.keep()` — returning the NamedTempFile lets it
                // auto-delete on drop, so a killed process leaks nothing.
                return Ok(Download { temp, sha256 });
            }
            Err(Interrupted { written, error }) => {
                have = written;
                let retryable = match error.downcast_ref::<ureq::Error>() {
                    Some(err) => crate::http::is_retryable(err),
                    // An I/O failure mid-stream (connection reset) is worth
                    // another attempt.
                    None => true,
                };
                last_error = Some(error);
                if !retryable {
                    break;
                }
            }
        }
    }

    Err(last_error.unwrap_or_else(|| anyhow::anyhow!("Failed to download {url}")))
        .with_context(|| format!("Failed to download {url}"))
}

/// A download attempt that did not finish, and how many bytes survived it.
struct Interrupted {
    written: u64,
    error: anyhow::Error,
}

/// Perform one download attempt, appending to `temp` from byte `resume_from`.
///
/// Returns the number of bytes on disk when an attempt fails, so the next
/// attempt can resume rather than start over — the difference between losing
/// 5 seconds and losing a 150 MiB transfer.
fn fetch_into(url: &str, temp: &mut NamedTempFile, resume_from: u64) -> Result<(), Interrupted> {
    let mut request = crate::http::get(url);
    if resume_from > 0 {
        request = request.set("Range", &format!("bytes={resume_from}-"));
    }

    let response = match request.call() {
        Ok(response) => response,

        // 416 Range Not Satisfiable means our offset lies past the end of the
        // file the server now holds, so the bytes we kept are stale — the
        // release was re-published, or an earlier attempt wrote garbage.
        // Discarding them and starting over is wasteful but correct; failing
        // outright would leave the user permanently stuck on a bad partial file.
        Err(ureq::Error::Status(416, _)) if resume_from > 0 => {
            let _ = truncate_to_start(temp);
            return Err(Interrupted {
                written: 0,
                // Deliberately not a `ureq::Error`, so the caller treats this as
                // retryable and makes a clean attempt from byte zero.
                error: anyhow::anyhow!(
                    "server rejected the resume range; restarting from the start"
                ),
            });
        }

        Err(err) => {
            return Err(Interrupted {
                written: resume_from,
                error: err.into(),
            })
        }
    };

    // A server that does not support ranges ignores the header and replies 200
    // with the *whole* body. Appending that to what we already have would
    // silently produce a corrupt archive, so start over instead. (The checksum
    // would catch it, but only after another full transfer.)
    let resuming = resume_from > 0 && response.status() == 206;
    let start = if resuming { resume_from } else { 0 };

    if !resuming {
        if let Err(err) = truncate_to_start(temp) {
            return Err(Interrupted {
                written: 0,
                error: err,
            });
        }
    } else if let Err(err) = seek_to_end(temp) {
        return Err(Interrupted {
            written: resume_from,
            error: err,
        });
    }

    // Content-Length covers only the remaining bytes on a 206, so add what we
    // already hold to show a meaningful total.
    let remaining = response
        .header("Content-Length")
        .and_then(|value| value.parse::<u64>().ok());
    let progress = match remaining {
        Some(bytes) => ProgressBar::new(start + bytes),
        None => ProgressBar::new_spinner(),
    };
    progress.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} {bytes}/{total_bytes} [{bar:40.cyan/blue}] {bytes_per_sec}",
        )
        .unwrap_or_else(|_| ProgressStyle::default_bar()),
    );
    progress.set_position(start);

    let mut reader = response.into_reader();
    let result = copy_with_progress(&mut reader, temp.as_file_mut(), &progress);
    progress.finish_and_clear();

    // Survivors are counted from the file itself, not from a byte tally.
    result.map_err(|error| Interrupted {
        written: bytes_on_disk(temp),
        error,
    })
}

/// Discard any partial content so an attempt can start from byte zero.
fn truncate_to_start(temp: &mut NamedTempFile) -> Result<()> {
    use std::io::Seek;

    let file = temp.as_file_mut();
    file.set_len(0)
        .context("Failed to reset the download file")?;
    file.rewind()
        .context("Failed to rewind the download file")?;
    Ok(())
}

/// Position the file at its end so resumed bytes append.
fn seek_to_end(temp: &mut NamedTempFile) -> Result<()> {
    use std::io::{Seek, SeekFrom};

    temp.as_file_mut()
        .seek(SeekFrom::End(0))
        .context("Failed to seek the download file")?;
    Ok(())
}

/// Verify the SHA-256 of `temp` against the expected digest, returning the
/// verified digest.
///
/// The expected digest comes either directly (`expected_sha256`) or from the
/// checksum document published at `checksum_url`. Returns an error on
/// mismatch without extracting anything.
fn verify_checksum(
    url: &str,
    checksum_url: &str,
    expected_sha256: Option<&str>,
    temp: &NamedTempFile,
) -> Result<String> {
    let filename = url.rsplit('/').next().unwrap_or(url);

    let expected = match expected_sha256 {
        Some(expected) => expected.to_ascii_lowercase(),
        None => {
            let document = fetch_checksum_document(checksum_url)?;
            extract_expected_hash(&document, filename).ok_or_else(|| {
                anyhow::anyhow!("Could not find a SHA-256 hash for {filename} in {checksum_url}")
            })?
        }
    };

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

/// Bytes currently on disk for a partial download.
///
/// Read from the filesystem rather than tracked in a counter: a `write_all` that
/// fails partway may still have written some bytes, so a counter can understate
/// the real length. Resuming from too low an offset while appending would
/// silently corrupt the archive. On any error this reports 0, which restarts the
/// download — wasteful but never wrong.
fn bytes_on_disk(temp: &NamedTempFile) -> u64 {
    temp.as_file()
        .metadata()
        .map(|meta| meta.len())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{BufRead, BufReader},
        net::{TcpListener, TcpStream},
        sync::{
            atomic::{AtomicUsize, Ordering},
            Arc, Mutex,
        },
        thread,
        time::{Duration, Instant},
    };

    const HASH_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const HASH_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    // ── Checksum document parsing ────────────────────────────────────────────

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

    // ── Retry classification ─────────────────────────────────────────────────

    /// Build a throwaway response so `ureq::Error::Status` can be constructed.
    fn status_error(code: u16) -> ureq::Error {
        let response = ureq::Response::new(code, "Status", "").expect("build test response");
        ureq::Error::Status(code, response)
    }

    /// Retrying a permanent failure only delays the error the user needs to see:
    /// a 404 means the version does not exist, and waiting will not change that.
    #[test]
    fn permanent_failures_are_not_retried() {
        for code in [400, 401, 403, 404, 410, 451] {
            assert!(
                !crate::http::is_retryable(&status_error(code)),
                "HTTP {code} should not be retried"
            );
        }
    }

    #[test]
    fn transient_failures_are_retried() {
        for code in [408, 425, 429, 500, 502, 503, 504] {
            assert!(
                crate::http::is_retryable(&status_error(code)),
                "HTTP {code} should be retried"
            );
        }
    }

    // ── Backoff ──────────────────────────────────────────────────────────────

    #[test]
    fn backoff_grows_and_is_capped() {
        let first = crate::http::backoff_delay(0).as_millis();
        let second = crate::http::backoff_delay(1).as_millis();

        assert!(first >= 500, "first delay should be at least the base");
        assert!(second > first, "delay should grow: {first} -> {second}");

        // Capped, allowing for the 25% jitter.
        for attempt in 0..20 {
            let delay = crate::http::backoff_delay(attempt).as_millis();
            assert!(
                delay <= 8_000 + 2_100,
                "attempt {attempt} delay {delay}ms exceeds the cap"
            );
        }
    }

    /// A large attempt number must not overflow the shift or the multiply.
    #[test]
    fn backoff_does_not_overflow() {
        for attempt in [30_u32, 64, 1000, u32::MAX] {
            let _ = crate::http::backoff_delay(attempt);
        }
    }

    // ── Resume against a scripted server ─────────────────────────────────────

    /// How the test server answers one request for the archive.
    #[derive(Debug, Clone, Copy)]
    enum Behavior {
        /// Announce the full length, send `0..n` bytes, then hang up — exactly
        /// what a dropped connection looks like to the client.
        TruncateThenClose(usize),
        /// Serve correctly, honouring `Range` with a 206.
        Serve,
        /// Ignore `Range` and always reply 200 with the whole body.
        IgnoreRange,
        /// Fail with a status code.
        Status(u16),
    }

    /// `Range` start offsets observed by the test server, one per archive
    /// request, shared so assertions can inspect what the client actually sent.
    type SeenRanges = Arc<Mutex<Vec<Option<u64>>>>;

    /// Serve `payload` at `/archive.tar.gz` and its digest at `/SHASUMS256.txt`.
    ///
    /// The listener is non-blocking with a deadline and stops after
    /// `expected_requests`, so a finished client can never leave the thread
    /// blocked in `accept()` and hang `join()`.
    fn scripted_server(
        payload: Vec<u8>,
        script: Vec<Behavior>,
        expected_requests: usize,
    ) -> (String, SeenRanges, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let addr = listener.local_addr().expect("listener address");

        let digest = {
            let mut hasher = Sha256::new();
            hasher.update(&payload);
            hex_encode(&hasher.finalize())
        };
        let document = format!("{digest}  archive.tar.gz\n");

        // Range offsets seen on each archive request, for assertions.
        let ranges = Arc::new(Mutex::new(Vec::new()));
        let ranges_for_server = Arc::clone(&ranges);
        let archive_count = Arc::new(AtomicUsize::new(0));

        let handle = thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(30);
            let mut served = 0usize;

            while served < expected_requests && Instant::now() < deadline {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        // An accepted socket can inherit non-blocking mode on
                        // some platforms; reads below assume blocking.
                        let _ = stream.set_nonblocking(false);
                        served += 1;

                        let (path, range) = read_request(&mut stream);
                        if path.contains("SHASUMS") {
                            respond(&mut stream, 200, None, document.as_bytes());
                            continue;
                        }

                        let index = archive_count.fetch_add(1, Ordering::SeqCst);
                        ranges_for_server.lock().unwrap().push(range);

                        match script.get(index).copied().unwrap_or(Behavior::Serve) {
                            Behavior::Status(code) => {
                                respond(&mut stream, code, None, b"error");
                            }
                            Behavior::TruncateThenClose(sent) => {
                                let body = &payload[..sent.min(payload.len())];
                                // Full Content-Length, short body.
                                write_headers(&mut stream, 200, payload.len(), None);
                                let _ = stream.write_all(body);
                                let _ = stream.flush();
                            }
                            Behavior::IgnoreRange => {
                                respond(&mut stream, 200, None, &payload);
                            }
                            Behavior::Serve => match range {
                                Some(start) if (start as usize) < payload.len() => {
                                    let rest = &payload[start as usize..];
                                    respond(
                                        &mut stream,
                                        206,
                                        Some((start, payload.len() as u64)),
                                        rest,
                                    );
                                }
                                Some(_) => respond(&mut stream, 416, None, b""),
                                None => respond(&mut stream, 200, None, &payload),
                            },
                        }
                    }
                    Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(5));
                    }
                    Err(_) => return,
                }
            }
        });

        (format!("http://{addr}"), ranges, handle)
    }

    /// Read a request, returning its path and any `Range` start offset.
    fn read_request(stream: &mut TcpStream) -> (String, Option<u64>) {
        let mut reader = BufReader::new(stream.try_clone().expect("clone stream"));
        let mut request_line = String::new();
        let _ = reader.read_line(&mut request_line);

        let path = request_line
            .split_whitespace()
            .nth(1)
            .unwrap_or("/")
            .to_string();

        let mut range = None;
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" || line == "\n" {
                break;
            }
            let lower = line.to_ascii_lowercase();
            if let Some(value) = lower.strip_prefix("range:") {
                if let Some(spec) = value.trim().strip_prefix("bytes=") {
                    range = spec
                        .split('-')
                        .next()
                        .and_then(|start| start.trim().parse().ok());
                }
            }
        }
        (path, range)
    }

    fn write_headers(
        stream: &mut TcpStream,
        code: u16,
        content_length: usize,
        content_range: Option<(u64, u64)>,
    ) {
        let mut headers = format!(
            "HTTP/1.1 {code} S\r\nContent-Length: {content_length}\r\nConnection: close\r\n"
        );
        if let Some((start, total)) = content_range {
            headers.push_str(&format!(
                "Content-Range: bytes {start}-{}/{total}\r\n",
                total - 1
            ));
        }
        headers.push_str("\r\n");
        let _ = stream.write_all(headers.as_bytes());
    }

    fn respond(stream: &mut TcpStream, code: u16, content_range: Option<(u64, u64)>, body: &[u8]) {
        write_headers(stream, code, body.len(), content_range);
        let _ = stream.write_all(body);
        let _ = stream.flush();
    }

    /// Varied bytes, so a wrongly assembled file cannot coincidentally match.
    fn payload_of(size: usize) -> Vec<u8> {
        (0..size).map(|i| (i % 251) as u8).collect()
    }

    fn digest_of(payload: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(payload);
        hex_encode(&hasher.finalize())
    }

    fn fetch(base: &str) -> Result<Download> {
        download_to_temp(
            &format!("{base}/archive.tar.gz"),
            &format!("{base}/SHASUMS256.txt"),
            None,
        )
    }

    /// A publisher that carries the digest in the release metadata rather than
    /// a checksum document must be verified the same way.
    #[test]
    fn verifies_against_a_directly_carried_digest() {
        let payload = payload_of(40_000);
        let (base, _ranges, server) = scripted_server(payload.clone(), vec![Behavior::Serve], 1);

        let download = download_to_temp(
            &format!("{base}/archive.tar.gz"),
            "",
            Some(&digest_of(&payload)),
        )
        .expect("download should verify against the expected digest");

        assert_eq!(
            std::fs::read(download.path()).expect("read file"),
            payload,
            "the verified file must be intact"
        );

        server.join().ok();
    }

    /// A wrong directly-carried digest must fail exactly like a document
    /// mismatch, without consulting any checksum URL.
    #[test]
    fn rejects_a_mismatched_expected_digest() {
        let payload = payload_of(10_000);
        let (base, _ranges, server) = scripted_server(payload.clone(), vec![Behavior::Serve], 1);

        let err = download_to_temp(&format!("{base}/archive.tar.gz"), "", Some(HASH_A))
            .expect_err("a mismatched expected digest must fail");

        assert!(
            format!("{err:#}").contains("SHA-256 mismatch"),
            "expected a checksum mismatch, got: {err:#}"
        );

        server.join().ok();
    }

    /// The point of the feature: a connection dropped partway must resume from
    /// where it stopped rather than restart, and still verify.
    #[test]
    fn resumes_after_an_interrupted_transfer() {
        let payload = payload_of(200_000);
        let (base, ranges, server) = scripted_server(
            payload.clone(),
            vec![Behavior::TruncateThenClose(100_000), Behavior::Serve],
            3, // two archive attempts plus the checksum
        );

        let download = fetch(&base).expect("download should resume and verify");

        assert_eq!(
            std::fs::read(download.path()).expect("read downloaded file"),
            payload,
            "resumed file must match the original byte for byte"
        );

        let seen = ranges.lock().unwrap().clone();
        assert_eq!(seen.len(), 2, "expected one failure and one resume");
        assert_eq!(seen[0], None, "first attempt should not send Range");
        assert_eq!(
            seen[1],
            Some(100_000),
            "resume must continue from the bytes already on disk"
        );

        server.join().ok();
    }

    /// A server that ignores `Range` replies 200 with the whole body. Appending
    /// that to what is already held would silently corrupt the archive.
    #[test]
    fn restarts_when_the_server_ignores_range() {
        let payload = payload_of(150_000);
        let (base, _ranges, server) = scripted_server(
            payload.clone(),
            vec![Behavior::TruncateThenClose(70_000), Behavior::IgnoreRange],
            3,
        );

        let download = fetch(&base).expect("download should restart cleanly and verify");

        assert_eq!(
            std::fs::read(download.path()).expect("read file"),
            payload,
            "a non-206 reply must replace the partial file, not append to it"
        );

        server.join().ok();
    }

    /// 416 means the offset lies past the end of what the server now holds, so
    /// the kept bytes are stale and must be discarded rather than retried
    /// forever.
    #[test]
    fn recovers_when_the_server_rejects_the_resume_range() {
        let payload = payload_of(120_000);
        let (base, _ranges, server) = scripted_server(
            payload.clone(),
            vec![
                Behavior::TruncateThenClose(60_000),
                Behavior::Status(416),
                Behavior::Serve,
            ],
            4,
        );

        let download = fetch(&base).expect("should recover from a rejected range");

        assert_eq!(
            std::fs::read(download.path()).expect("read file"),
            payload,
            "should end up with the complete archive"
        );

        server.join().ok();
    }

    #[test]
    fn retries_a_transient_server_error() {
        let payload = payload_of(50_000);
        let (base, ranges, server) = scripted_server(
            payload.clone(),
            vec![Behavior::Status(503), Behavior::Serve],
            3,
        );

        let download = fetch(&base).expect("a 503 should be retried");

        assert_eq!(std::fs::read(download.path()).expect("read file"), payload);
        assert_eq!(
            ranges.lock().unwrap().len(),
            2,
            "expected exactly one retry"
        );

        server.join().ok();
    }

    /// A 404 must fail immediately: the release genuinely does not exist, and
    /// retrying only makes the user wait to be told so.
    #[test]
    fn does_not_retry_a_missing_release() {
        let (base, ranges, server) = scripted_server(
            payload_of(1_000),
            vec![Behavior::Status(404), Behavior::Serve, Behavior::Serve],
            1,
        );

        assert!(fetch(&base).is_err(), "a 404 should fail the download");
        assert_eq!(
            ranges.lock().unwrap().len(),
            1,
            "a permanent failure must not be retried"
        );

        server.join().ok();
    }

    #[test]
    fn gives_up_after_the_attempt_limit() {
        let attempts = crate::http::MAX_ATTEMPTS as usize;
        let (base, ranges, server) = scripted_server(
            payload_of(80_000),
            // Every attempt truncates, so it can never complete.
            vec![Behavior::TruncateThenClose(10_000); attempts],
            attempts,
        );

        assert!(
            fetch(&base).is_err(),
            "should fail after exhausting its attempts"
        );
        assert_eq!(
            ranges.lock().unwrap().len(),
            attempts,
            "should make exactly MAX_ATTEMPTS attempts, no more"
        );

        server.join().ok();
    }

    /// Verification still runs after a resume, so a transfer that assembles into
    /// the wrong bytes is caught instead of being extracted.
    #[test]
    fn rejects_a_download_whose_digest_does_not_match() {
        let payload = payload_of(40_000);
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");

        let server = thread::spawn(move || {
            for _ in 0..2 {
                let Ok((mut stream, _)) = listener.accept() else {
                    return;
                };
                let (path, _) = read_request(&mut stream);
                if path.contains("SHASUMS") {
                    // A valid-looking digest for different content.
                    respond(
                        &mut stream,
                        200,
                        None,
                        format!("{HASH_A}  archive.tar.gz\n").as_bytes(),
                    );
                } else {
                    respond(&mut stream, 200, None, &payload);
                }
            }
        });

        let err = fetch(&format!("http://{addr}")).expect_err("mismatched digest must fail");
        assert!(
            format!("{err:#}").contains("SHA-256 mismatch"),
            "expected a checksum mismatch, got: {err:#}"
        );

        server.join().ok();
    }
}
