//! Shared HTTP client configuration.
//!
//! Every outbound request in runx goes through here so that timeouts and the
//! User-Agent are applied uniformly. Previously each module built its own bare
//! `ureq::get(...)` with no timeout at all, so a server that accepted the
//! connection and then stalled would hang runx indefinitely with no output and
//! no way to recover short of killing it.
//!
//! **Why not a single total timeout:** `ureq`'s per-request `.timeout()` sets a
//! deadline for the *entire* call. Runtime archives are hundreds of megabytes,
//! so any total deadline generous enough for a slow-but-working connection is
//! too long to catch a hang, and any deadline short enough to catch a hang
//! would abort legitimate large downloads. Instead this configures a connect
//! timeout plus an **idle** read timeout: a transfer may take as long as it
//! needs so long as bytes keep arriving, but a stalled socket fails promptly.

use std::{
    sync::OnceLock,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

/// Compile-time User-Agent derived from the crate version, so it never goes
/// stale as the crate is bumped.
pub const USER_AGENT: &str = concat!("runx/", env!("CARGO_PKG_VERSION"));

/// Maximum time to establish a TCP connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Maximum time to wait for *more* data once a transfer is under way. This is
/// an idle timeout, not a total one, so large archives are unaffected.
const READ_TIMEOUT: Duration = Duration::from_secs(60);

/// Process-wide agent. Built once so connections can be pooled across the
/// checksum fetch and the archive download.
static AGENT: OnceLock<ureq::Agent> = OnceLock::new();

/// Build an agent with explicit timeouts.
pub fn agent_with_timeouts(connect: Duration, read: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(connect)
        .timeout_read(read)
        .timeout_write(read)
        .user_agent(USER_AGENT)
        .build()
}

/// The shared agent used for all runx requests.
pub fn agent() -> &'static ureq::Agent {
    AGENT.get_or_init(|| agent_with_timeouts(CONNECT_TIMEOUT, READ_TIMEOUT))
}

/// Start a GET request with runx's standard timeouts and User-Agent.
///
/// Requests to the GitHub API (Bun, Deno and Python version lookups) get an
/// `Authorization: Bearer` header when `GITHUB_TOKEN` is set, raising the
/// unauthenticated 60 req/h cap to 5000. No other host ever receives the
/// token.
pub fn get(url: &str) -> ureq::Request {
    let mut request = agent().get(url);
    if let Some(header) = github_auth_header(url) {
        request = request.set("Authorization", &header);
    }
    request
}

/// `Authorization: Bearer <token>` for api.github.com requests, from the
/// optional `GITHUB_TOKEN` env var. `None` for every other host, so an
/// exported token cannot leak to nodejs.org, go.dev or a GitHub CDN.
fn github_auth_header(url: &str) -> Option<String> {
    let host = url.split("://").nth(1)?.split(['/', '?', '#']).next()?;
    if host != "api.github.com" {
        return None;
    }
    std::env::var("GITHUB_TOKEN")
        .ok()
        .map(|token| format!("Bearer {token}"))
}

/// Total attempts before a download is abandoned.
pub const MAX_ATTEMPTS: u32 = 4;

/// First backoff delay; each subsequent attempt doubles it.
const BASE_DELAY_MS: u64 = 500;

/// Ceiling on backoff, so a long outage does not stall for minutes.
const MAX_DELAY_MS: u64 = 8_000;

/// Whether a failed request is worth repeating.
///
/// Retrying a permanent failure only delays the error the user needs to see: a
/// 404 means the version does not exist, and no amount of waiting changes that.
/// Only transport failures and the status codes that specifically signal "try
/// again" are retried.
pub fn is_retryable(error: &ureq::Error) -> bool {
    match error {
        // 408 Request Timeout, 425 Too Early, 429 Too Many Requests, and the
        // 5xx family are all transient by definition.
        ureq::Error::Status(code, _) => {
            matches!(code, 408 | 425 | 429 | 500 | 502 | 503 | 504)
        }
        // Connection reset, DNS hiccup, TLS failure, idle timeout.
        ureq::Error::Transport(_) => true,
    }
}

/// Delay before attempt `attempt` (zero-based), with jitter.
///
/// Jitter matters because runx installs runtimes in parallel threads: without
/// it, several downloads throttled by the same server would retry in lockstep
/// and be throttled again together. It is derived from the system clock rather
/// than by adding a random-number dependency to a deliberately small CLI.
pub fn backoff_delay(attempt: u32) -> Duration {
    // `1 << attempt` with the shift clamped, so a large attempt count cannot
    // overflow the multiply.
    let doublings = attempt.min(5);
    let exponential = BASE_DELAY_MS.saturating_mul(1_u64 << doublings);
    let capped = exponential.min(MAX_DELAY_MS);

    let spread = capped / 4 + 1;
    let jitter = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| u64::from(elapsed.subsec_nanos()) % spread)
        .unwrap_or(0);

    Duration::from_millis(capped + jitter)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Read, net::TcpListener, thread};

    /// A server that accepts a connection and then never replies must not hang
    /// the client. Without a read timeout this test would block forever, which
    /// is exactly the production failure being fixed.
    #[test]
    fn stalled_server_times_out_instead_of_hanging() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("addr");

        // Hold the connection open, read the request, send nothing back.
        let server = thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buffer = [0u8; 1024];
                let _ = stream.read(&mut buffer);
                thread::sleep(Duration::from_secs(10));
            }
        });

        let agent = agent_with_timeouts(Duration::from_millis(500), Duration::from_millis(500));
        let started = std::time::Instant::now();
        let result = agent.get(&format!("http://{addr}/hang")).call();
        let elapsed = started.elapsed();

        assert!(result.is_err(), "a stalled response must fail, not hang");
        assert!(
            elapsed < Duration::from_secs(5),
            "should give up promptly, took {elapsed:?}"
        );

        drop(server);
    }

    /// A refused connection must surface quickly as an error.
    #[test]
    fn unreachable_host_fails_fast() {
        // Bind then drop, so the port is almost certainly closed.
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("addr");
        drop(listener);

        let agent = agent_with_timeouts(Duration::from_millis(500), Duration::from_millis(500));
        let started = std::time::Instant::now();
        let result = agent.get(&format!("http://{addr}/nope")).call();

        assert!(result.is_err(), "connection to a closed port should fail");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn shared_agent_is_reused() {
        let first = agent() as *const ureq::Agent;
        let second = agent() as *const ureq::Agent;
        assert_eq!(first, second, "agent should be built once");
    }

    #[test]
    fn user_agent_tracks_the_crate_version() {
        assert_eq!(USER_AGENT, format!("runx/{}", env!("CARGO_PKG_VERSION")));
    }

    /// `GITHUB_TOKEN` is optional and must reach only api.github.com; no other
    /// host may see it, and without the variable nothing changes.
    ///
    /// One test touches the variable so parallel tests cannot interleave
    /// mutations; no other test reads it.
    #[test]
    fn github_token_is_attached_only_to_the_github_api() {
        std::env::set_var("GITHUB_TOKEN", "secret-token");
        assert_eq!(
            github_auth_header("https://api.github.com/repos/oven-sh/bun/releases?per_page=100"),
            Some("Bearer secret-token".to_string())
        );
        assert_eq!(
            github_auth_header("https://nodejs.org/dist/index.json"),
            None
        );
        assert_eq!(github_auth_header("https://go.dev/dl/?mode=json"), None);
        // Release downloads are served by the GitHub CDN, not the API.
        assert_eq!(
            github_auth_header(
                "https://github.com/oven-sh/bun/releases/download/bun-v1.3.14/bun-darwin.zip"
            ),
            None
        );
        assert_eq!(
            github_auth_header("https://api.github.com/"),
            Some("Bearer secret-token".to_string())
        );

        std::env::remove_var("GITHUB_TOKEN");
        assert_eq!(
            github_auth_header("https://api.github.com/repos/denoland/deno/releases"),
            None,
            "without GITHUB_TOKEN the request must stay unauthenticated"
        );
    }

    /// The agent must actually send our User-Agent, since python-build-standalone
    /// metadata comes from the GitHub API, which rejects requests without one.
    #[test]
    fn sends_the_user_agent_header() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("addr");

        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut buffer = [0u8; 2048];
            let read = stream.read(&mut buffer).unwrap_or(0);
            let request = String::from_utf8_lossy(&buffer[..read]).to_string();

            use std::io::Write;
            let body = "ok";
            let _ = stream.write_all(
                format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                )
                .as_bytes(),
            );
            request
        });

        // Nudge the connection so the server thread proceeds.
        let agent = agent_with_timeouts(Duration::from_secs(2), Duration::from_secs(2));
        let response = agent.get(&format!("http://{addr}/ua")).call();
        assert!(response.is_ok(), "request should succeed");

        let request = server.join().expect("server thread");
        assert!(
            request.to_ascii_lowercase().contains("user-agent: runx/"),
            "request should carry the runx User-Agent, got:\n{request}"
        );
    }
}
