# AGENTS.md

Single-crate Rust project: one binary (`runx`) plus `src/` library modules. Target platforms: Linux, macOS, Windows.

## Commands

- Verify (matches CI): `cargo fmt -- --check` && `cargo clippy --all-targets --all-features -- -D warnings` && `cargo test`
- `cargo build` does **not** compile test code — only `clippy --all-targets` / `cargo test` do. After editing tests, always run those.
- One test file: `cargo test --test cli_integration <name-filter>`. The suite takes ~50s (spawns the real binary per test via `CARGO_BIN_EXE_runx`).

## Environment quirks

- Cache location: `~/.runx`, overridable with `RUNX_HOME` (read in `src/cache.rs::runx_home`). **Never isolate tests via `HOME`**: `dirs::home_dir` reads it on macOS, which redirects `CARGO_HOME` and breaks the cargo registry. Use `RUNX_HOME=<dir>`.
- Host is macOS: no `timeout`/`pwsh` commands. For hang-prone probes use `run_probe & pid=$!; sleep loop; kill $pid`.
- Integration tests must not touch the network: `tests/cli_integration.rs` uses only echo-style commands and no `[runtimes]` section. Manual E2E verification (real downloads from nodejs.org) goes through `RUNX_HOME`-isolated projects.

## Architecture (non-obvious)

- **`src/runtime.rs::resolve_runtime` is the single security chokepoint.** Version strings become filesystem paths (`~/.runx/runtimes/<tool>/<version>`, `remove_dir_all`'d on reinstall) and download URLs. All validation must live here / in `version::validate_concrete` — never validate in callers. This was a P0 path-traversal fix; bypassing it is a security regression.
- **CLI routing**: `[run]` keys dispatch through clap's `external_subcommand`, so adding a built-in subcommand silently steals any `[run]` key with the same name. New subcommands must be added to `RESERVED_COMMANDS` in `src/main.rs`; `runx run <key>` is the escape hatch. `tests/cli_integration.rs` pins this behavior.
- `src/version.rs`: `PartialEq` is manual (derived from `Ord` — do not re-derive). Bare partial versions (`"20"`) are X-ranges; only exact 3-part pins resolve offline.
- Installs are atomic: extract into `.staging-*` dirs, then rename + write `.runx-complete.json` receipt (see `src/cache.rs`). Runtimes without a receipt but with a working executable are legacy installs, adopted on use — don't "fix" that.
- `install.sh` / `install.ps1`: exact-match SHA-256, fail closed when no checksum tool is available (`RUNX_SKIP_CHECKSUM=1` to override). Keep both in sync.

## Conventions

- Work happens on the `hardening` branch; CI triggers on `main` and PRs.
- Commits in this session use `git -c user.name="runx maintainer" -c user.email="maintainer@runx.local" commit` (no repo-level git identity configured). Match existing commit-message style (`feat(x):`, `fix(x):` prefixes).
- No telemetry, no global PATH/shell-file modification — preserve these guarantees; `runx.toml` always overrides auto-detection.
