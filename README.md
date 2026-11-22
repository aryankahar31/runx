<div align="center">

# 🚀 Runx

### Project Launcher with Portable Runtimes

Run projects with reproducible, isolated runtimes — no global installations, no shell integration.

[![CI](https://github.com/aryankahar31/runx/actions/workflows/ci.yml/badge.svg)](https://github.com/aryankahar31/runx/actions/workflows/ci.yml)
[![Latest Release](https://img.shields.io/github/v/release/aryankahar31/runx?label=Release)](https://github.com/aryankahar31/runx/releases)
[![License](https://img.shields.io/github/license/aryankahar31/runx?cacheSeconds=60)](LICENSE)
[![Rust](https://img.shields.io/badge/Built%20with-Rust-orange?logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20macOS%20%7C%20Linux-blue)](https://github.com/aryankahar31/runx)

**🌐 [runx-cli.vercel.app](https://runx-cli.vercel.app/)**

</div>

An unrelated crates.io project named `runx` exists; this tool's package is
`runx-launcher`, while its installed binary and Rust library crate are both
named `runx`.

<div align="center">

![runx demo: zero-config Node 20.11.0 detection from .nvmrc, SHA-256 verification, isolated execution, and warm-cache run with no global Node installed](demo/runx.gif)

</div>

```text
$ runx dev
No runx.toml found — detected from project files:
  node 20.11.0 (from .nvmrc)
Installing node 20.11.0
Downloading https://nodejs.org/dist/v20.11.0/node-v20.11.0-linux-x64.tar.xz
✓ Checksum verified
Running `npm run dev`
```

Second run — everything is already cached:

```text
$ runx dev
No runx.toml found — detected from project files:
  node 20.11.0 (from .nvmrc)
Using cached node 20.11.0 at /home/user/.runx/runtimes/node/20.11.0
Running `npm run dev`
```

---

## Contents

- [Why Runx?](#why-runx)
- [What Runx is not](#what-runx-is-not)
- [Quick Start](#quick-start)
- [Features](#features)
- [Supported Runtimes](#supported-runtimes)
- [Zero-Config Detection](#zero-config-detection)
- [Runtime Resolution](#runtime-resolution)
- [Lockfiles](#lockfiles)
- [Security](#security)
- [Comparison](#comparison)
- [Benchmarks](#benchmarks)
- [Architecture](#architecture)
- [CLI Reference](#cli-reference)
- [Roadmap](#roadmap)
- [Zero-admin machines](#zero-admin-machines)
- [CI and containers](#ci-and-containers)
- [Documentation](#documentation)

---

## Why Runx?

Modern projects pin runtime versions per project; your machine has one global `PATH`. Runx closes that gap:

- **No global runtime installations** — runtimes live in a local cache (`~/.runx`), not in your system
- **Project-driven detection** — reads `.nvmrc`, `package.json` `engines`, `pyproject.toml`, `go.mod`, Bun/Deno version files that your project *already* declares
- **Isolated execution** — each command runs with exactly the declared runtime versions on its child `PATH`
- **Multi-runtime projects** — Node + Python + Go in one project? All resolved and placed on one `PATH`
- **Reproducible environments** — exact pins resolve offline; ranges resolve to the newest matching release; `runx.lock` freezes both
- **Editor-independent** — works the same from any terminal, any editor, any CI runner

## What Runx is not

Runx is not an IDE, build system, or language-specific package manager. It provisions and isolates the runtimes your project's commands run with — nothing more.

- VS Code / any editor → terminal or task runner → `runx dev` → node/python/go/bun/deno/java → your app
- Android Studio → Gradle / Android SDK → (optionally) runx providing the underlying JVM/Node runtime

If your workflow needs full IDE tooling, build orchestration, or platform-specific SDKs, those tools still own that job. Runx's job is narrower: resolve, verify, cache, isolate, and run the exact runtime version a project declares.

## Quick Start

| Channel | v0.7.1 status | Command / destination |
| --- | --- | --- |
| Install script (macOS / Linux) | Available now | `curl -fsSL https://raw.githubusercontent.com/aryankahar31/runx/main/install.sh \| sh` |
| Install script (Windows) | Available now | `iwr https://raw.githubusercontent.com/aryankahar31/runx/main/install.ps1 \| iex` |
| GitHub release binaries | Available now | [Releases](https://github.com/aryankahar31/runx/releases) |
| Cargo | Available now | `cargo install runx-launcher` (installs `runx`) |
| cargo-binstall | Available now | `cargo binstall runx-launcher` (installs `runx`) |
| npm | Planned / pending submission | `npx --package runx-launcher -- runx dev` |
| Homebrew | Available now | `brew install aryankahar31/tap/runx-launcher` (installs `runx`) |
| Winget | Planned / pending submission | `winget install aryankahar31.runx-launcher` |
| Scoop | Available now | `scoop bucket add aryankahar31 https://github.com/aryankahar31/scoop-bucket; scoop install runx-launcher` |
| AUR | Planned / pending submission | `yay -S runx-launcher-bin` (Waiting for Arch AUR registration to reopen) |
| Nix | Planned / pending submission | `runx-launcher` flake package |

> Behind a network that blocks Fastly (`raw.githubusercontent.com`)? Use the jsDelivr mirror:
> `curl -fsSL https://cdn.jsdelivr.net/gh/aryankahar31/runx@main/install.sh | sh`

The v0.7.1 install scripts and release binaries are available, the
`runx-launcher` crate is published on crates.io, Homebrew is available via
[`aryankahar31/homebrew-tap`](https://github.com/aryankahar31/homebrew-tap) (`brew install aryankahar31/tap/runx-launcher`), and Scoop is available via
[`aryankahar31/scoop-bucket`](https://github.com/aryankahar31/scoop-bucket). npm,
Winget, AUR, and Nix remain planned / pending their separate
submissions; the AUR `runx-launcher-bin` package waits for Arch to reopen
registration. The release
workflow renders package versions and archive digests; downstream packages
still need review and publication. See [`LAUNCH_CHECKLIST.md`](LAUNCH_CHECKLIST.md).

For published releases, the curl and PowerShell installers verify the release
archive's SHA-256 checksum — see [Security](#security).

Initialize a project:

```bash
runx init
```

This creates a starter `runx.toml`:

```toml
[runtimes]
node = "20.11.0"
python = "3.11.7"

[run]
dev = "npm run dev"
build = "npm run build"
test = "npm test"
```

Run:

```bash
runx dev
```

Or skip `runx.toml` entirely — if your project already has a `.nvmrc`, `pyproject.toml`, or similar, just run `runx dev` ([detection rules](#zero-config-detection)).

## Features

### Runtime management

- Node.js, Python, Bun, Go, Deno, Java (Temurin 9+)
- Automatic downloads with retry, exponential backoff and **resume**
- Semver/range resolution to the newest matching release
- Shared runtime cache across all projects

### Reproducibility & security

- [`runx.lock`](docs/lockfile.md) pins resolved versions for CI and teams
- A recorded platform artifact's digest **becomes the install's integrity constraint**: locked installs verify against `runx.lock` itself, so upstream mutations of a published release fail instead of flowing through
- `--locked` fails on anything the lockfile doesn't pin
- SHA-256 verification before extraction (older Deno releases are TLS-only); Sigstore/cosign signatures for runx's own releases
- Staged installs; `--locked` also authenticates retained archives and checks cached runtime contents

### Developer experience

- Zero-config auto-detection (`package.json` scripts become run keys; names
  matching built-in commands use `runx run <key>`), or explicit `runx.toml`
- `runx install` / `--install` — install project dependencies (npm, Yarn, pnpm, Bun, pip, uv, Poetry, Go) using the managed runtime
- No shell integration required, ever — nothing happens until you type `runx`
- Cross-platform: Linux, macOS, Windows
- Scripting modes: `--json`, `--quiet`, `--offline`
- `runx doctor` — cache integrity, stale PATH shims, and project readiness (runtime, package manager, dependencies, lockfile) with a pass/warn/fail reason per check; `doctor --verify` checks cached runtime entry-point executables against their recorded digests where available
- Cache management (`list`/`size`/`clean`/`prune`), `self update`, shell completions

## Supported Runtimes

| Runtime | Status |
|----------|--------|
| Node.js | ✅ |
| Python | ✅ |
| Bun | ✅ |
| Go | ✅ |
| Deno | ✅ |
| Java (Eclipse Temurin, 9+) | ✅ |
| Ruby, PHP, .NET | 🚧 Planned — see [ROADMAP.md](ROADMAP.md) |

## Zero-Config Detection

Runx automatically detects runtimes from files your project already uses — `.nvmrc`, `.python-version`, `pyproject.toml`, `Pipfile`, `package.json` `engines`, `go.mod`, `.java-version`, `.sdkmanrc`, and Bun/Deno version files. Multiple runtimes are detected together and share one isolated environment. Every `package.json` script becomes a run key without configuration; names matching built-in commands are invoked with `runx run <key>`.

Detection never writes to disk and never merges with an existing `runx.toml`; explicit configuration always wins.

**[View full detection rules →](docs/zero-config.md)**

## Runtime Resolution

Ranges (`>=20`, `^20`, `~20.11`) resolve against each vendor's own release index to the newest matching release. Exact pins (`20.11.0`) avoid version-list lookups, but downloads and Python/Go asset metadata may still need the network. There is no third-party registry in the path; see the [offline limits](docs/security.md#locked-execution).

**[View resolution internals & registry freshness →](docs/runtime-resolution.md)**

## Lockfiles

```bash
runx lock
```

Installs the declared runtimes and writes `runx.lock`, pinning the exact resolved version (with per-platform checksums) so every machine and CI runner gets identical runtimes. Enforce it with `runx run <key> --locked`.

**[View lockfile format & CI usage →](docs/lockfile.md)**

## Security

Download and cache verification have distinct scopes:

- Published SHA-256 checksum verification before extraction, except for older Deno releases
- `--locked` requires a platform digest in `runx.lock`, verifies the retained archive against it, and compares cached runtime contents with a fresh extraction; missing metadata or mismatched bytes abort execution
- Sigstore/cosign keyless verification for runx's own release archives, not vendor runtimes (opt-in strict mode via `RUNX_REQUIRE_SIGNATURE=1`)
- Staged installs verified before publication; interrupted installs may leave staging files, and replacement is not crash-atomic
- `runx doctor --verify` checks entry-point bytes against available local receipts; it is not full-tree authentication and metadata failures may leave entries unverifiable
- Strict validation of every version string at a single chokepoint (path-traversal safe by construction)
- Installers fail closed when no checksum tool is available

**[Read the full security model →](docs/security.md)**

## Comparison

What runx combines in one editor-independent workflow:

| Capability | Runx |
| :--- | :---: |
| Project launcher | ✅ |
| Multiple runtimes per project | ✅ |
| Zero-config detection from existing files | ✅ |
| Isolated child `PATH` | ✅ |
| No shell integration required | ✅ |
| Lockfile with per-platform checksums | ✅ |

Traditional version managers (nvm, Volta, pyenv, asdf, mise) cover parts of
this list differently. Runx's focus is combining provisioning, detection,
isolation, verification, and execution in one workflow. The detailed
third-party comparison is best-effort and may change as those tools evolve.

**[View the detailed per-tool comparison →](docs/comparison.md)**

## Benchmarks

Headline numbers on warm cache (macOS, Apple silicon, zsh — full method in [docs/benchmarks.md](docs/benchmarks.md)):

| Measurement | Mean |
| :-- | --: |
| Complete `runx <key>` run, warm cache | 11.4 ms |
| Runx overhead before your command starts | ~0.25 ms, **once per invocation** |
| Interactive zsh startup with mise activation | +18.6 ms versus the no-hook baseline |

This benchmark measures shell startup with an empty config versus one that
loads `mise activate`; it does not measure repeated prompt renders. Runx has no
shell hook, so it adds no runx work during shell startup or prompt rendering.

`--locked` is the exception: it re-extracts each runtime's archive and compares
it against the cached tree on every run. A previous single-machine measurement
recorded in the v0.6.2 notes was ~2.1 s for one runtime versus 133 ms for the
same run without `--locked`; this is not part of the current shell benchmark.
The extra verification cost suits CI and release builds rather than an
edit-run loop.

**[View full benchmark methodology →](docs/benchmarks.md)**

## Architecture

```text
runx
 │
 ▼
Parse runx.toml / detect project files
 │
 ▼
Resolve runtime versions
 │
 ▼
Check local cache ─── hit ──┐
 │ miss                     │
 ▼                          │
Download + verify           │
(atoms: staging → rename)   │
 └──────────────────────────┘
 │
 ▼
Build isolated PATH
 │
 ▼
Execute project command
```

Runtimes are cached under `~/.runx/runtimes/<tool>/<version>/` and reused across all projects.

**[View isolation guarantees & implementation notes →](docs/architecture.md)**

## CLI Reference

```bash
runx dev              # run a key from [run] (any non-builtin word works)
runx build            # run a non-reserved package.json script
runx run test --locked # enforce the lockfile, including cached artifact integrity
runx --frozen test     # --locked plus --offline (CI default)
runx init             # create a starter runx.toml
runx lock             # write runx.lock
runx install          # install project dependencies (npm ci, pnpm install, pip, etc.)
runx doctor           # diagnose cache, PATH and detection issues
runx doctor --verify  # hash cached runtime entry points against recorded digests where available
runx list             # cached runtimes (alias: runx cache list)
runx which node       # executable this project's run would use
runx exec -- make     # run an arbitrary program with the project's runtime PATH
runx cache list       # also: size, dir, clean, prune (--older-than N, --dry-run)
runx self update      # verified atomic binary swap (alias: runx upgrade)
runx completions zsh  # bash, zsh, fish, powershell

runx --json doctor    # machine-readable output (also: cache list/size)
runx --quiet dev      # suppress banners and progress bars
runx --offline dev    # block runx network operations, not child networking
runx --install dev    # install project dependencies before running the command
```

### Dependency installation

`runx install` detects the project's package manager and installs dependencies using the runx-managed runtime. Supported managers:

| Lockfile | Manager | Install command |
|----------|---------|-----------------|
| `package-lock.json` | npm | `npm ci` |
| `yarn.lock` | Yarn | `yarn install`, or Corepack/`npx` at the `packageManager` pin, falling back when Yarn is not installed |
| `pnpm-lock.yaml` | pnpm | `pnpm install`, or Corepack/`npx` at the `packageManager` pin, falling back when pnpm is not installed |
| `bun.lock` / `bun.lockb` | Bun | `bun install` |
| `pyproject.toml` | pip | `pip install -e .` or individual deps |
| `requirements.txt` | pip | `pip install -r requirements.txt` |
| `uv.lock` | uv | `uv sync --frozen` (supersedes pip detection) |
| `poetry.lock` | Poetry | `poetry install` (supersedes pip detection) |
| `go.mod` | Go | `go mod download` |
| `deno.lock` | Deno | `deno install` (Deno 2.x), `deno cache <entrypoint>` (Deno 1.x) |

`runx --install <key>` does the same inline: install dependencies, then run the command. When dependencies are already up to date, the install is skipped.

#### What each command prepares

Runtime readiness and dependency readiness are separate steps, and only `--install` does both:

| Command | Runtimes | Project dependencies | Runs your command |
|---------|----------|----------------------|-------------------|
| `runx install` | installed if missing | installed | no |
| `runx --install dev` | installed if missing | installed | yes |
| `runx dev` | installed if missing | **no** — prints a hint if they are missing | yes |
| `runx lock` | installed (a digest can only be recorded for verified bytes) | no | no |

`runx doctor` reports both kinds of readiness without changing anything, and `runx --offline <key>` prints the same summary before it runs.

Dependency state is reported as one of four conditions, each with the command that resolves it:

- **already installed** — nothing to do
- **not installed** — `runx install`
- **lockfile out of sync** — the lockfile changed after the last install. For npm this is what `npm ci` refuses, so the fix is `npm install`
- **nothing to install** — no recognised manifest in the project

Global flags work before any subcommand (`runx --json <command>`).

Pass arguments through with `--`: `runx dev -- --port 3000`. Without `--`, extra arguments are rejected rather than silently dropped.

### Environment variables

| Variable | Effect |
|----------|--------|
| `RUNX_HOME` | Cache location (default `~/.runx`). Useful for CI caching and isolation. |
| `HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY` | Proxy for all outbound requests. Precedence follows curl; TLS and checksum verification are unchanged by proxying. |
| `NO_PROXY` | Comma-separated hosts that bypass the proxy (`*` disables it entirely). Loopback always bypasses. |
| `RUNX_RESOLUTION` | `latest` (default) or `minimum` — see [strict mode](docs/runtime-resolution.md#strict-mode). |
| `GITHUB_TOKEN` | Optional. Raises GitHub API rate limits for Bun/Deno/Python lookups (60 → 5000 req/hour). Sent only to `api.github.com`. |
| `RUNX_LOCKED` | `1` makes every run command behave as if `--locked` was passed. |
| `RUNX_REQUIRE_SIGNATURE` | `1` makes a missing cosign signature an error instead of a warning. |

There is no telemetry. Runx itself makes outbound requests for runtime release
metadata, archives and checksums, and for its own release/update metadata,
archives, checksums, and optional Sigstore bundles. Child processes such as
npm, uv, or Poetry manage their own network access.

## Roadmap

The current release scope and planned work are tracked in
[`ROADMAP.md`](ROADMAP.md). v0.7.0 added Java, lock enforcement, daily-use CLI
commands, dependency lockfile support, a GitHub Action, and packaging
templates. v0.7.1 names the Cargo and npm packages `runx-launcher`; downstream
package submissions remain manual launch steps.

## Zero-admin machines

runx needs no `sudo`, system package manager, or writable system directory. The
binary, runtimes, and downloads all live under `~/.runx` (or `RUNX_HOME`), and
runx never edits your shell files on its own. The installer may ask before
adding `~/.runx/bin` to `PATH`; `RUNX_INSTALL_NO_MODIFY_PATH=1` skips even that
prompt.

See [docs/zero-admin.md](docs/zero-admin.md) for cache locations, offline
recipes, and the honest limits (runx installs language runtimes, not system
libraries).

## CI and containers

The composite `setup-runx` action is available from this repository. Consumer
workflow verification remains pending. It installs a SHA-256-verified release
binary, caches `~/.runx/runtimes`, and can enforce the lockfile:

```yaml
- uses: actions/checkout@v4
- uses: aryankahar31/runx/setup-runx@main
  with:
    locked: true
- run: runx test
```

See [`setup-runx/`](setup-runx/) for inputs and a full example workflow. For
containers, [`docs/docker.md`](docs/docker.md) covers building the included
`Dockerfile` recipe and keeping the runtime cache across runs. The image is not
built in CI or published by this repository.

## Documentation

- [Website](https://runx-cli.vercel.app/) — overview, quick start, benchmarks ([source](website/))
- [Zero-Config Detection](docs/zero-config.md) — every detected file, priority order, inference rules
- [Zero-admin machines](docs/zero-admin.md) — no root, no system installs, `RUNX_HOME` on locked-down hosts
- [Runtime Resolution](docs/runtime-resolution.md) — range semantics, strict mode, registry freshness
- [Lockfiles](docs/lockfile.md) — format, platform artifacts, CI enforcement
- [Security](docs/security.md) — verification model, manual release verification
- [Architecture](docs/architecture.md) — isolation guarantees, atomic installs
- [Benchmarks](docs/benchmarks.md) — shell-overhead methodology, freshness checks
- [Comparison](docs/comparison.md) — per-tool matrix vs nvm/Volta/pyenv/asdf/mise
- [Containers](docs/docker.md) — build and use the image
- [Changelog](CHANGELOG.md) — every release from v0.1.0 onward

---

## Contributing

Contributions are welcome — see [CONTRIBUTING.md](CONTRIBUTING.md) for the
branch → PR → CI workflow and how releases are cut. Please ensure:

- Runtime installers remain portable
- Downloads are deterministic
- Existing tests continue to pass; new features include tests
- Documentation is updated

You need a stable Rust toolchain (1.88 or newer, via [rustup](https://rustup.rs)) with the
`rustfmt` and `clippy` components — CI enforces all four commands below:

```bash
git clone https://github.com/aryankahar31/runx.git && cd runx
cargo fmt -- --check                                      # formatting
cargo clippy --all-targets --all-features -- -D warnings  # lints, warnings are errors
cargo test            # full suite
cargo build --release # produce target/release/runx
```

## License

MIT — see the [LICENSE](LICENSE) file.

<div align="center">

⭐ **If Runx helped you, consider giving the repository a star!**

</div>
