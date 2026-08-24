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

/// Process-wide direct agent. Built once so connections can be pooled across
/// requests.
static DIRECT_AGENT: OnceLock<ureq::Agent> = OnceLock::new();

/// Process-wide proxied agent, built on first use from the environment.
///
/// `None` means no proxy is configured (or its URL was unparseable, which
/// degrades to direct with a warning rather than failing every request).
static PROXIED_AGENT: OnceLock<Option<ureq::Agent>> = OnceLock::new();

/// Build an agent with explicit timeouts.
pub fn agent_with_timeouts(connect: Duration, read: Duration) -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(connect)
        .timeout_read(read)
        .timeout_write(read)
        .user_agent(USER_AGENT)
        .build()
}

fn direct_agent() -> &'static ureq::Agent {
    DIRECT_AGENT.get_or_init(|| agent_with_timeouts(CONNECT_TIMEOUT, READ_TIMEOUT))
}

fn proxied_agent(proxy_url: &str) -> Option<&'static ureq::Agent> {
    PROXIED_AGENT
        .get_or_init(|| match ureq::Proxy::new(proxy_url) {
            Ok(proxy) => Some(
                ureq::AgentBuilder::new()
                    .timeout_connect(CONNECT_TIMEOUT)
                    .timeout_read(READ_TIMEOUT)
                    .timeout_write(READ_TIMEOUT)
                    .user_agent(USER_AGENT)
                    .proxy(proxy)
                    .build(),
            ),
            Err(err) => {
                eprintln!(
                    "Warning: ignoring unusable proxy setting {proxy_url:?} ({err}); \
                     connecting directly"
                );
                None
            }
        })
        .as_ref()
}

/// The agent for `url`: the proxied agent when the environment configures a
/// proxy that applies to this host, the direct agent otherwise. TLS, checksum
/// and signature verification are unchanged either way — a proxy only relays
/// the same verified bytes.
fn agent_for_url(url: &str) -> &'static ureq::Agent {
    let host = host_of(url);
    let configured = pick_proxy(|name| std::env::var(name).ok());
    match (configured, host) {
        (Some(proxy_url), Some(host)) if !host_is_no_proxy(&host) => {
            proxied_agent(&proxy_url).unwrap_or_else(direct_agent)
        }
        _ => direct_agent(),
    }
}

/// Host portion of `url`, lowercased, port and credentials stripped.
fn host_of(url: &str) -> Option<String> {
    let rest = url.split("://").nth(1)?;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority.rsplit('@').next().unwrap_or(authority);
    let host = authority.split(':').next().unwrap_or(authority);
    if host.is_empty() {
        return None;
    }
    Some(host.to_ascii_lowercase())
}

/// Pick the proxy URL from the environment using curl's precedence: an
/// explicit HTTPS/ALL setting wins over the plain-HTTP ones, and uppercase
/// variants win over lowercase. Empty values are treated as unset so a stray
/// `HTTP_PROXY=""` does not route traffic to an invalid proxy.
///
/// Takes the lookup as a parameter so tests can run without touching the
/// process environment.
fn pick_proxy(read: impl Fn(&str) -> Option<String>) -> Option<String> {
    for name in [
        "ALL_PROXY",
        "all_proxy",
        "HTTPS_PROXY",
        "https_proxy",
        "HTTP_PROXY",
        "http_proxy",
    ] {
        if let Some(value) = read(name)
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
        {
            return Some(value);
        }
    }
    None
}

/// True when `host` must bypass the configured proxy.
///
/// Two rules, checked before [`host_matches_no_proxy_list`]:
///
/// * **Loopback always bypasses** (`localhost`, `127.*`, `::1`): a developer's
///   global `HTTP_PROXY` must never hijack traffic to a runtime mirror served
///   on their own machine (or runx's own hermetic test servers).
/// * `NO_PROXY`/`no_proxy`, standard comma-separated semantics: `*` disables
///   all proxying; a leading dot matches subdomains only; otherwise entries
///   match themselves and their subdomains. Ports are ignored.
pub fn host_is_no_proxy(host: &str) -> bool {
    if host == "localhost"
        || host == "::1"
        || host.starts_with("127.")
        || host.ends_with(".localhost")
    {
        return true;
    }

    let Ok(no_proxy) = std::env::var("NO_PROXY").or_else(|_| std::env::var("no_proxy")) else {
        return false;
    };
    host_matches_no_proxy_list(host, &no_proxy)
}

/// The `NO_PROXY` matcher proper, split out so tests exercise it without
/// touching process environment state.
fn host_matches_no_proxy_list(host: &str, no_proxy: &str) -> bool {
    let no_proxy = no_proxy.trim();
    if no_proxy.is_empty() {
        return false;
    }
    if no_proxy == "*" {
        return true;
    }

    for entry in no_proxy.split(',') {
        let entry = entry.trim();
        // A leading dot means "subdomains only" — the bare domain does not
        // match. Everything else matches itself and its subdomains.
        let subdomains_only = entry.starts_with('.');
        let entry = entry.trim_start_matches('.').to_ascii_lowercase();
        // Strip a :port suffix from the entry, if present.
        let entry = entry.split(':').next().unwrap_or(&entry);
        if entry.is_empty() {
            continue;
        }
        let is_subdomain = host.ends_with(&format!(".{entry}"));
        if (!subdomains_only && host == entry) || is_subdomain {
            return true;
        }
    }
    false
}

/// Start a GET request with runx's standard timeouts and User-Agent.
///
/// Requests to the GitHub API (Bun, Deno and Python version lookups) get an
/// `Authorization: Bearer` header when `GITHUB_TOKEN` is set, raising the
/// unauthenticated 60 req/h cap to 5000. No other host ever receives the
/// token.
pub fn get(url: &str) -> ureq::Request {
    let mut request = agent_for_url(url).get(url);
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
    fn shared_direct_agent_is_reused() {
        let first = direct_agent() as *const ureq::Agent;
        let second = direct_agent() as *const ureq::Agent;
        assert_eq!(first, second, "agent should be built once");
    }

    #[test]
    fn user_agent_tracks_the_crate_version() {
        assert_eq!(USER_AGENT, format!("runx/{}", env!("CARGO_PKG_VERSION")));
    }

    // ── Proxy configuration ──────────────────────────────────────────────────

    use std::collections::HashMap;

    fn env_map(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        move |name| map.get(name).cloned()
    }

    #[test]
    fn proxy_precedence_follows_curl_order() {
        let none = pick_proxy(|_| None);
        assert_eq!(none, None, "no env means no proxy");

        let all = pick_proxy(env_map(&[("ALL_PROXY", "http://all:1")]));
        assert_eq!(all.as_deref(), Some("http://all:1"));

        // ALL_PROXY beats the protocol-specific variables.
        let order = pick_proxy(env_map(&[
            ("ALL_PROXY", "http://all:1"),
            ("HTTPS_PROXY", "http://https:2"),
            ("HTTP_PROXY", "http://http:3"),
        ]));
        assert_eq!(order.as_deref(), Some("http://all:1"));

        let https = pick_proxy(env_map(&[
            ("HTTPS_PROXY", "http://https:2"),
            ("HTTP_PROXY", "http://http:3"),
        ]));
        assert_eq!(https.as_deref(), Some("http://https:2"));

        let lower = pick_proxy(env_map(&[("http_proxy", "http://lower:4")]));
        assert_eq!(lower.as_deref(), Some("http://lower:4"));

        // An empty value is treated as unset, not as a broken proxy.
        let empty = pick_proxy(env_map(&[
            ("HTTPS_PROXY", ""),
            ("HTTP_PROXY", "http://x:5"),
        ]));
        assert_eq!(empty.as_deref(), Some("http://x:5"));
    }

    #[test]
    fn no_proxy_matching_follows_standard_semantics() {
        // Exact host.
        assert!(host_matches_no_proxy_list("nodejs.org", "nodejs.org"));
        // Subdomains match; siblings and suffix-lookalikes do not.
        assert!(host_matches_no_proxy_list("dist.nodejs.org", "nodejs.org"));
        assert!(!host_matches_no_proxy_list("notnodejs.org", "nodejs.org"));
        assert!(!host_matches_no_proxy_list("example.org", "nodejs.org"));
        // Leading dot: subdomains only, not the bare domain.
        assert!(host_matches_no_proxy_list("a.example.com", ".example.com"));
        assert!(!host_matches_no_proxy_list("example.com", ".example.com"));
        // Wildcard disables everything; ports in entries are ignored.
        assert!(host_matches_no_proxy_list("anything.invalid", "*"));
        assert!(host_matches_no_proxy_list(
            "host.example",
            "host.example:8080"
        ));
        // Comma lists match any entry; empty bypasses nothing.
        assert!(host_matches_no_proxy_list(
            "go.dev",
            "nodejs.org, go.dev ,api.github.com"
        ));
        assert!(!host_matches_no_proxy_list("go.dev", ""));
        assert!(!host_matches_no_proxy_list("go.dev", "   "));
    }

    /// Loopback hosts always bypass, whatever the environment says — a
    /// developer's global proxy must not hijack traffic to a local runtime
    /// mirror (or runx's own test servers).
    #[test]
    fn loopback_always_bypasses_the_proxy() {
        for host in [
            "localhost",
            "127.0.0.1",
            "127.8.8.8",
            "::1",
            "mirror.localhost",
        ] {
            assert!(host_is_no_proxy(host), "{host} must bypass");
        }
        assert!(!host_is_no_proxy("localhost.example.com"),);
    }

    /// A configured proxy must actually carry the request — proven with a
    /// local fake proxy that records the request line it receives, so no
    /// external network is touched. A proxied request to a non-loopback host
    /// arrives in absolute form; a direct one would never reach this server.
    ///
    /// This is the only test that touches proxy environment variables, per
    /// this file's convention for env-mutating tests.
    #[test]
    fn requests_route_through_the_configured_proxy() {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = listener.local_addr().expect("addr");

        let server = thread::spawn(move || {
            let mut first_line = String::new();
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 2048];
                if let Ok(read) = stream.read(&mut buf) {
                    first_line = String::from_utf8_lossy(&buf[..read])
                        .lines()
                        .next()
                        .unwrap_or_default()
                        .to_string();
                }
                let body = "ok";
                let _ = stream.write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    )
                    .as_bytes(),
                );
            }
            first_line
        });

        std::env::set_var("HTTP_PROXY", format!("http://{addr}"));

        let response = get("http://198.51.100.7/hello").call();
        std::env::remove_var("HTTP_PROXY");

        assert!(
            response.is_ok(),
            "request through the local proxy should succeed"
        );
        let seen = server.join().expect("server thread");
        assert!(
            seen.starts_with("GET http://198.51.100.7/hello"),
            "the proxy must receive an absolute-form request, got: {seen:?}"
        );
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
