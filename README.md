<div align="center">

# 🚀 Runx

### Universal Project Launcher with Portable Runtimes

Run projects with reproducible, isolated runtimes — no global installations, no shell integration.

[![CI](https://github.com/aryankahar31/runx/actions/workflows/ci.yml/badge.svg)](https://github.com/aryankahar31/runx/actions/workflows/ci.yml)
[![Latest Release](https://img.shields.io/github/v/release/aryankahar31/runx?label=Release)](https://github.com/aryankahar31/runx/releases)
[![License](https://img.shields.io/github/license/aryankahar31/runx?cacheSeconds=60)](LICENSE)
[![Rust](https://img.shields.io/badge/Built%20with-Rust-orange?logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20macOS%20%7C%20Linux-blue)](https://github.com/aryankahar31/runx)

**🌐 [runx-cli.vercel.app](https://runx-cli.vercel.app/)**

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

- VS Code / any editor → terminal or task runner → `runx dev` → node/python/go/bun/deno → your app
- Android Studio → Gradle / Android SDK → (optionally) runx providing the underlying JVM/Node runtime

If your workflow needs full IDE tooling, build orchestration, or platform-specific SDKs, those tools still own that job. Runx's job is narrower: resolve, verify, cache, isolate, and run the exact runtime version a project declares.

## Quick Start

Install (macOS / Linux):

```bash
curl -fsSL https://raw.githubusercontent.com/aryankahar31/runx/main/install.sh | sh
```

> Behind a network that blocks Fastly (`raw.githubusercontent.com`)? Use the jsDelivr mirror:
> `curl -fsSL https://cdn.jsdelivr.net/gh/aryankahar31/runx@main/install.sh | sh`

Windows PowerShell:

```powershell
iwr https://raw.githubusercontent.com/aryankahar31/runx/main/install.ps1 | iex
```

Both scripts verify the downloaded binary against its published SHA-256 checksum before installing — see [Security](#security).

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

- Node.js, Python, Bun, Go, Deno
- Automatic downloads with retry, exponential backoff and **resume**
- Semver/range resolution to the newest matching release
- Shared runtime cache across all projects

### Reproducibility & security

- [`runx.lock`](docs/lockfile.md) pins resolved versions for CI and teams
- `--locked` fails on anything the lockfile doesn't pin
- SHA-256 verification of every download, plus Sigstore/cosign signatures since v0.4.2
- Atomic installs — an interrupted download can never corrupt the cache

### Developer experience

- Zero-config auto-detection, or explicit `runx.toml`
- No shell integration required, ever — nothing happens until you type `runx`
- Cross-platform: Linux, macOS, Windows
- `runx doctor`, cache management (`list`/`size`/`clean`/`prune`), `self update`, shell completions

## Supported Runtimes

| Runtime | Status |
|----------|--------|
| Node.js | ✅ |
| Python | ✅ |
| Bun | ✅ |
| Go | ✅ |
| Deno | ✅ |
| Java | 🚧 Planned |
| .NET | 🚧 Planned |

## Zero-Config Detection

Runx automatically detects runtimes from files your project already uses — `.nvmrc`, `.python-version`, `package.json` `engines`, `pyproject.toml`, `go.mod`, and Bun/Deno version files. Multiple runtimes are detected together and share one isolated environment.

Detection never writes to disk and never merges with an existing `runx.toml`; explicit configuration always wins.

**[View full detection rules →](docs/zero-config.md)**

## Runtime Resolution

Ranges (`>=20`, `^20`, `~20.11`) resolve against each vendor's own release index to the newest matching release; exact pins (`20.11.0`) never touch the network and never change. Resolution results are cached for at most 6 hours — there is no third-party registry in the path.

**[View resolution internals & registry freshness →](docs/runtime-resolution.md)**

## Lockfiles

```bash
runx lock
```

Installs the declared runtimes and writes `runx.lock`, pinning the exact resolved version (with per-platform checksums) so every machine and CI runner gets identical runtimes. Enforce it with `runx run <key> --locked`.

**[View lockfile format & CI usage →](docs/lockfile.md)**

## Security

Every install is verified before it touches your disk:

- SHA-256 checksum verification of every download, before extraction
- Sigstore/cosign keyless signature verification on release archives (opt-in strict mode via `RUNX_REQUIRE_SIGNATURE=1`)
- Atomic installs into staging directories — a failed or interrupted install leaves nothing behind
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

Traditional version managers (nvm, Volta, pyenv, asdf, mise) each cover parts of this list differently — several require shell hooks or shims, and none read `package.json` `engines` / `pyproject.toml` directly today. Runx does not claim to reinvent runtime management; its focus is combining provisioning, detection, isolation, verification, and execution into one workflow.

**[View the detailed per-tool comparison →](docs/comparison.md)**

## Benchmarks

Headline numbers on warm cache (macOS, Apple silicon, zsh — full method in [docs/benchmarks.md](docs/benchmarks.md)):

| Measurement | Mean |
| :-- | --: |
| Complete `runx <key>` run, warm cache | 5.9 ms |
| Runx overhead before your command starts | ~0.2 ms, **once per invocation** |
| Shell-hook alternative (`mise activate`) | +15.2 ms **per prompt render** |

Because runx adds no shell hook, prompts without `runx` pay nothing at all.

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
runx build            # same
runx test --locked    # enforce the lockfile
runx init             # create a starter runx.toml
runx lock             # write runx.lock
runx doctor           # diagnose cache, PATH and detection issues
runx cache list       # also: size, clean, prune (--older-than N)
runx self update      # verified atomic binary swap
runx completions zsh  # bash, zsh, fish, powershell
```

Pass arguments through with `--`: `runx dev -- --port 3000`. Without `--`, extra arguments are rejected rather than silently dropped.

### Environment variables

| Variable | Effect |
|----------|--------|
| `RUNX_HOME` | Cache location (default `~/.runx`). Useful for CI caching and isolation. |
| `RUNX_RESOLUTION` | `latest` (default) or `minimum` — see [strict mode](docs/runtime-resolution.md#strict-mode). |
| `GITHUB_TOKEN` | Optional. Raises GitHub API rate limits for Bun/Deno/Python lookups (60 → 5000 req/hour). Sent only to `api.github.com`. |
| `RUNX_REQUIRE_SIGNATURE` | `1` makes a missing cosign signature an error instead of a warning. |

There is no telemetry, and runx makes no network requests beyond fetching runtime release metadata, archives, and their checksums.

## Roadmap

**Current** (v0.5)

- Node.js, Python, Bun, Go, Deno
- `runx.lock` + `--locked`, Sigstore/cosign signing
- Cache management, `doctor`, `self update`, completions
- Multi-runtime detection and isolated multi-runtime `PATH`

**Next**

- Java, .NET
- Monorepo / workspace support
- Pre/post run hooks

**Future**

- Plugin system and runtime registry
- VS Code extension
- Homebrew, Scoop, Winget, Chocolatey
- Stable API

## Documentation

- [Website](https://runx-cli.vercel.app/) — overview, quick start, benchmarks
- [Zero-Config Detection](docs/zero-config.md) — every detected file, priority order, inference rules
- [Runtime Resolution](docs/runtime-resolution.md) — range semantics, strict mode, registry freshness
- [Lockfiles](docs/lockfile.md) — format, platform artifacts, CI enforcement
- [Security](docs/security.md) — verification model, manual release verification
- [Architecture](docs/architecture.md) — isolation guarantees, atomic installs
- [Benchmarks](docs/benchmarks.md) — shell-overhead methodology, freshness checks
- [Comparison](docs/comparison.md) — per-tool matrix vs nvm/Volta/pyenv/asdf/mise

---

## Contributing

Contributions are welcome. Please ensure:

- Runtime installers remain portable
- Downloads are deterministic
- Existing tests continue to pass; new features include tests
- Documentation is updated

```bash
git clone https://github.com/aryankahar31/runx.git && cd runx
cargo test            # full suite
cargo build --release # produce target/release/runx
```

## License

MIT — see the [LICENSE](LICENSE) file.

<div align="center">

⭐ **If Runx helped you, consider giving the repository a star!**

</div>
