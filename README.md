<div align="center">

# 🚀 Runx

### Universal Project Launcher with Portable Runtimes

Run projects with the exact runtime versions they require — **without installing Node.js, Python, or other runtimes globally.**

[![CI](https://github.com/aryankahar31/runx/actions/workflows/ci.yml/badge.svg)](https://github.com/aryankahar31/runx/actions/workflows/ci.yml)
[![Latest Release](https://img.shields.io/github/v/release/aryankahar31/runx?label=Release)](https://github.com/aryankahar31/runx/releases)
[![License](https://img.shields.io/github/license/aryankahar31/runx?cacheSeconds=60)](LICENSE)
[![Rust](https://img.shields.io/badge/Built%20with-Rust-orange?logo=rust)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20macOS%20%7C%20Linux-blue)](https://github.com/aryankahar31/runx)


**One command. Any runtime. Any project.**

⭐ Star the repository if you find it useful.

</div>

---

# Why Runx?

Modern development often requires multiple runtime versions.

One project needs:

- Node.js 20
- Python 3.11

Another needs:

- Node.js 18
- Python 3.10

Installing and managing these globally quickly becomes difficult.

**Runx solves this problem.**

Runx automatically downloads the exact runtime versions required by a project, stores them in a local cache, and runs commands inside an isolated environment.

No global installations.

No PATH pollution.

No version managers.

---

# 🔍 Zero-Config Mode

Runx works **without a `runx.toml`** if standard version files are already
present in your project.

## How it works

When you run `runx dev` and no `runx.toml` is found, runx automatically
scans the project directory for well-known ecosystem files and infers the
runtime versions from them.

If a `runx.toml` *does* exist it is **always used exclusively** — explicit
configuration always wins over auto-detection, with no merging.

## Detected files and priority order

### Node.js (first match wins)

| Priority | File | Notes |
|----------|------|-------|
| 1 | `.nvmrc` | Plain text, leading `v` stripped |
| 2 | `.node-version` | Plain text, leading `v` stripped |
| 3 | `package.json` → `engines.node` | JSON, range resolved (see below) |

### Python (first match wins)

| Priority | File | Notes |
|----------|------|-------|
| 1 | `.python-version` | Plain text, leading `v` stripped |
| 2 | `pyproject.toml` → `[project].requires-python` | TOML, range resolved (see below) |

### Bun (first match wins)

| Priority | File | Notes |
|----------|------|-------|
| 1 | `package.json` → `engines.bun` | JSON, range resolved |
| 2 | `package.json` → `packageManager` | `bun@1.1.0` form; the `+sha512.…` digest is ignored |

### Go (first match wins)

| Priority | File | Notes |
|----------|------|-------|
| 1 | `go.mod` → `go` directive | Plain text; the `toolchain` directive is ignored |

A `go.mod` also marks a project root for auto-detection, alongside
`package.json`, `.nvmrc`, `.node-version`, `.python-version`, `pyproject.toml`,
`.dvmrc`, `.deno-version`, `.git` and `runx.toml`.

## Semver range resolution

When a version file contains a range rather than an exact version, runx
resolves it to the **newest published release that satisfies the
constraint** — the same behaviour as nvm, Volta and mise.

| Input | Resolves to |
|-------|-------------|
| `>=20` | newest release ≥ 20 |
| `^20` | newest `20.x` |
| `~20.11` | newest `20.11.x` |
| `<20` | newest release below 20 |
| `20` | newest `20.x` (a bare partial version is an X-range) |
| `18 \|\| >=20` | newest release matching either branch |
| `>=3.11` | newest release ≥ 3.11 |
| `~=3.11` | newest `3.x` ≥ 3.11 (PEP 440) |
| `20.11.0` | `20.11.0` (exact pin, never changed) |

Runx always prints which concrete version a range resolved to, so there
are no silent surprises.

**Exact pins never touch the network.** Resolving a range needs the
published release list, which is cached for 6 hours; if it cannot be
fetched, runx falls back to the lowest satisfying version and says so, so
offline machines keep working.

### Strict mode

Set `RUNX_RESOLUTION=minimum` to resolve ranges to the *lowest* satisfying
version instead. This is fully offline and time-independent, but note that
`>=20` then means "the oldest Node 20 ever published", which carries known
CVEs. Exact pins are unaffected by this setting.

For reproducibility across machines and CI, prefer `runx lock` (below) over
strict mode: it pins the exact version that was resolved, rather than
re-deriving one.

---

# 🔒 Reproducible installs with `runx.lock`

Ranges resolve against the current release list, so the same project can
pick up a newer runtime next month. When that is not wanted — CI, a team,
a release branch — generate a lockfile:

```bash
runx lock
```

This installs the runtimes, then writes `runx.lock` pinning exactly what
was resolved. Commit it.

```toml
version = 1

[runtimes.node]
version = "20.11.0"
requirement = ">=20"

[runtimes.node.artifacts.macos-aarch64]
url = "https://nodejs.org/dist/v20.11.0/node-v20.11.0-darwin-arm64.tar.gz"
sha256 = "94e443d007e2882f8e5aecc85d978f7591520dc3b642adc7583b3cb0b3fc37d7"
```

Artifacts are keyed by platform because runtime archives *are*
platform-specific: Node 20.11.0 on macOS/arm64 is a different file with a
different digest than on Linux/x64. The **version** pin is shared across
platforms; the digest is a per-platform integrity check on top. Running
`runx lock` on macOS does not discard a teammate's Linux entry.

## Enforcing the lockfile in CI

```bash
runx run test --locked
```

`--locked` fails rather than resolving anything the lockfile does not
already pin, mirroring `cargo build --locked`. It fails when a runtime is
missing from the lockfile, or when `runx.toml` asks for a requirement the
lockfile does not record.

A missing entry for *your platform* is deliberately not fatal, even under
`--locked`: the version is still pinned, and the download is still verified
against the publisher's own checksums. A mixed-OS team is not blocked by a
lockfile generated elsewhere.

## Precedence

`runx.toml` always wins. If someone bumps a version there without
re-running `runx lock`, runx uses the config and warns that the lockfile is
stale — a lockfile that silently overrode an explicit version bump would be
baffling to debug.

## Run-command inference

For the inferred `dev` command runx checks whether `package.json` contains
a `"dev"` script and runs `npm run dev` if so.  No other commands are
guessed.  If a dev command cannot be inferred, runx prints a clear error
and suggests running `runx init`.

## Example output

With only a `.nvmrc` pinning `v20.11.0` and a `package.json` that has a
`dev` script:

```
No runx.toml found — detected from project files:
  node 20.11.0 (from .nvmrc)
Installing node 20.11.0
Downloading https://nodejs.org/dist/v20.11.0/node-v20.11.0-linux-x64.tar.xz
✓ Checksum verified
Extracting to /home/user/.runx/runtimes/node/.staging-20.11.0-4127-...
Running `npm run dev`
```

On subsequent runs the cached runtime is reused:

```
No runx.toml found — detected from project files:
  node 20.11.0 (from .nvmrc)
Using cached node 20.11.0 at /home/user/.runx/runtimes/node/20.11.0
Running `npm run dev`
```

When the project declares a **range** — say `"engines": { "node": ">=20" }` —
runx reports which concrete release it picked:

```
No runx.toml found — detected from project files:
  node >=20 (from package.json (engines.node))
Resolved node `>=20` to 22.11.0
Installing node 22.11.0
```

Pin the result with `runx lock` if you need that choice to stay fixed.

## Opt-in-by-absence guarantee

- If `runx.toml` exists → it is the sole source of truth. Auto-detection
  is never consulted, and the file is never modified.
- Auto-detection is the fallback *only* when no `runx.toml` is present.
- Auto-detection **never writes to disk**. To persist a detected
  configuration, run `runx init` which creates a starter `runx.toml`.

---

# ✨ Features

- 🚀 Zero global runtime installation
- 📦 Automatic downloads with retry, exponential backoff and **resume**
- 💾 Cache management — `list`, `size`, `clean`, `prune`
- 📌 `runx.lock` for reproducible installs across machines and CI
- 🎯 Ranges resolve to the **newest** matching release (nvm/Volta/mise semantics)
- 🛡 SHA-256 verification of every download, before extraction (Deno < 2.0.1
  excepted — those releases publish no archive checksum, see *Supported
  Runtimes*)
- ⚛️ Atomic installs — an interrupted download can never corrupt the cache
- 🔒 Isolated execution — global `PATH` and shell files are never touched
- 🐚 **No shell integration required**, ever
- ⚡ Exact version pins resolve offline and instantly
- 🖥 Cross-platform (Linux, macOS, Windows)
- ⚙ Zero-config auto-detection, or an explicit `runx.toml`
- 🦀 Built with Rust, no telemetry

---

# Installation

## macOS / Linux

```bash
curl -fsSL https://raw.githubusercontent.com/aryankahar31/runx/main/install.sh | sh
```

---

## Windows PowerShell

```powershell
iwr https://raw.githubusercontent.com/aryankahar31/runx/main/install.ps1 | iex
```

---

> **🛡 Security:** Both install scripts verify the downloaded binary against
> the SHA-256 checksum published with each release, and abort without
> extracting or installing anything if it does not match. They also **fail
> closed**: if no SHA-256 tool is available (`sha256sum`, `shasum`, or
> `openssl`), the install stops rather than silently proceeding unverified.
> Set `RUNX_SKIP_CHECKSUM=1` to override that deliberately.
>
> Checksums confirm the download is intact and matches what the publisher
> listed. They are fetched from the same origin as the artifact, so they are
> not by themselves protection against a compromised release host.
> `runx self update` verifies the same checksum before swapping the binary;
> cryptographic signature verification is on the roadmap (Sigstore/cosign).

Verify installation

```bash
runx --version
```

Expected output

```
runx <version>
```

It prints the exact version you installed.

---

# Verifying a Release Manually

If you download a binary directly from
[GitHub Releases](https://github.com/aryankahar31/runx/releases) instead of
using the install script, you can verify it manually.

## Linux / macOS

```bash
# Download the archive and the SHA256SUMS file
curl -fsSLO https://github.com/aryankahar31/runx/releases/latest/download/runx-linux-x64.tar.gz
curl -fsSLO https://github.com/aryankahar31/runx/releases/latest/download/SHA256SUMS

# Verify (prints OK if the checksum matches)
sha256sum -c SHA256SUMS --ignore-missing
# or on macOS:
shasum -a 256 -c SHA256SUMS --ignore-missing
```

## Windows PowerShell

```powershell
# Download the archive and the per-file checksum
Invoke-WebRequest -Uri https://github.com/aryankahar31/runx/releases/latest/download/runx-windows-x64.zip -OutFile runx-windows-x64.zip
Invoke-WebRequest -Uri https://github.com/aryankahar31/runx/releases/latest/download/runx-windows-x64.zip.sha256 -OutFile runx-windows-x64.zip.sha256

# Compare
$expected = (Get-Content .\runx-windows-x64.zip.sha256).Split(' ')[0]
$computed = (Get-FileHash .\runx-windows-x64.zip -Algorithm SHA256).Hash
if ($expected -ieq $computed) { Write-Host "OK" } else { Write-Error "MISMATCH" }
```

---

# Quick Start

Initialize a project

```bash
runx init
```

This creates

```text
runx.toml
```

Configure your project

```toml
[runtimes]
node = "20.11.0"
python = "3.11.7"

[run]
dev = "npm run dev"
build = "npm run build"
test = "npm test"
```

Run your application

```bash
runx dev
```

---

# Example

Project

```
my-project/
│
├── package.json
├── runx.toml
└── src/
```

package.json

```json
{
  "scripts": {
    "dev": "node index.js"
  }
}
```

index.js

```javascript
console.log("Hello from Runx!");
```

Run

```bash
runx dev
```

Output

```
Installing node 20.11.0
Downloading...
Extracting...

Running npm run dev

Hello from Runx!
```

Second run

```
Using cached node 20.11.0

Running npm run dev

Hello from Runx!
```

---

# Runtime Cache

Downloaded runtimes are stored in

```
~/.runx/runtimes/
```

Example

```
~/.runx/runtimes/

node/
└──20.11.0/

python/
└──3.11.7/
```

Runx automatically reuses cached runtimes.

No repeated downloads.

---

# Supported Runtimes

| Runtime | Status |
|----------|--------|
| Node.js | ✅ |
| Python | ✅ |
| Bun | ✅ |
| Go | ✅ |
| Deno | ✅ |

Deno releases from v2.0.1 publish a per-asset `.sha256sum` sidecar that runx
verifies exactly like Node, Bun and Go. Older Deno releases (the 1.x line and
v2.0.0) publish **no archive checksum**, so runx installs them with TLS-only
verification and prints a warning at install time.
| Java | 🚧 Planned |
| .NET | 🚧 Planned |

---

# CLI Commands

## Running project commands

Any word that is not a built-in subcommand is treated as a key from `[run]`:

```bash
runx dev            # runs the `dev` key
runx build          # runs the `build` key
runx test           # runs the `test` key
```

Use the explicit form when a key collides with a built-in name:

```bash
runx run dev
runx run test --locked   # fail if runx.lock does not pin everything
```

Pass arguments through to the run command with `--`:

```bash
runx dev -- --port 3000
runx run test -- --watch
```

Everything after `--` is appended to the underlying command, shell-quoted so
spaces and special characters arrive intact. Without `--`, extra arguments are
rejected rather than silently dropped.

## Project setup

```bash
runx init           # create a starter runx.toml
runx lock           # install runtimes and write runx.lock
```

## Cache management

```bash
runx cache list     # every cached runtime, with size and last use
runx cache size     # total disk usage
runx cache clean    # remove all runtimes      (dry run without --yes)
runx cache prune    # remove runtimes unused for 30+ days
runx cache prune --older-than 7 --yes
```

Both `clean` and `prune` print what they *would* delete and change nothing
unless you pass `--yes`.

## Information

```bash
runx --version
runx --help
runx doctor           # diagnose problems with the cache and PATH
```

## Shell completions

```bash
runx completions bash         # or: zsh, fish, powershell
runx completions zsh > "$ZDOTDIR/.zfunc/_runx"
```

## Self update

```bash
runx self update
```

Checks the latest GitHub release, verifies it against the release `SHA256SUMS`,
and atomically swaps the current binary (the previous one is kept as
`runx.old` until the new one runs). The binary needs write access to its own
directory — update from a local `cargo install` location manually instead.
Releases must publish a platform archive (`runx-{linux|macos|windows}-{x64|arm64}.tar.gz` or `.zip`)
plus `SHA256SUMS` for `self update` to work.

## Environment variables

| Variable | Effect |
|----------|--------|
| `RUNX_HOME` | Cache location (default `~/.runx`). Useful for CI caching and for isolating a cache without touching `HOME`. |
| `RUNX_RESOLUTION` | `latest` (default) or `minimum` — see [Strict mode](#strict-mode). |
| `GITHUB_TOKEN` | Optional. Authenticates the GitHub API version lookups (Bun, Deno, Python), raising the rate limit from 60 to 5000 requests/hour — useful for CI on a shared IP or heavy development. A classic PAT or fine-grained token with **no scopes** is enough for public release/tag data; runx sends it only to `api.github.com`, never to other hosts. |

There is no telemetry, and runx makes no network requests beyond fetching
runtime release metadata, archives, and their checksums.

---

# Build From Source

Clone

```bash
git clone https://github.com/aryankahar31/runx.git

cd runx
```

Build

```bash
cargo build --release
```

Binary

Linux/macOS

```
target/release/runx
```

Windows

```
target\release\runx.exe
```

---

# Architecture

```
                    runx
                      │
          ┌───────────┴───────────┐
          │                       │
          ▼                       ▼
    Parse runx.toml        Resolve runtimes
          │
          ▼
     Check local cache
          │
     ┌────┴────┐
     │         │
 Cache Hit   Cache Miss
     │         │
     │     Download Runtime
     │         │
     │     Extract Archive
     │         │
     └────┬────┘
          │
          ▼
  Build isolated PATH
          │
          ▼
 Execute project command
```

---

# How It Works

1. Read `runx.toml`
2. Resolve runtime versions
3. Check local cache
4. Download missing runtime
5. Extract portable runtime
6. Build isolated PATH
7. Execute command

---

# Isolation

Runx never modifies:

- Your global `PATH`
- Shell startup files (`.bashrc`, `.zshrc`, profiles)
- System-installed runtimes
- Anything outside `~/.runx` and your project directory

Every command runs with the cached runtime's `bin` directories **prepended**
to `PATH`, so the project's versions take priority over anything installed
system-wide. The existing `PATH` is then **appended**, so ordinary tools
(`git`, `make`, `curl`, Homebrew) keep working — runx isolates *runtime
versions*, not the entire environment.

Because the change is scoped to the child process, **no shell integration is
required**: no `eval "$(… init)"` line, no shim directory, no directory
hooks. Nothing about your shell changes until you type `runx`.

---

# Comparison

| Feature | Runx | nvm | Volta | pyenv | asdf | mise |
| :--- | :---: | :---: | :---: | :---: | :---: | :---: |
| Node.js | ✅ | ✅ | ✅ | ❌ | ✅ | ✅ |
| Python | ✅ | ❌ | ❌ | ✅ | ✅ | ✅ |
| Multiple runtimes | ✅ | ❌ | ❌ | ❌ | ✅ | ✅ |
| Runtime cache | ✅ | ✅ | ✅ | ✅ | ✅ | ✅ |
| Project launcher | ✅ | ❌ | ❌ | ❌ | ❌ | ✅ |
| Cross-platform | ✅ | ⚠️ | ✅ | ⚠️ | ✅ | ✅ |
| **No shell integration required** | ✅ | ❌ | ✅ (shims) | ❌ | ❌ | ⚠️ (optional; needed for ambient switching) |
| Reads `package.json` `engines` | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Reads `pyproject.toml` `requires-python` | ✅ | ❌ | ❌ | ❌ | ❌ | ❌ |
| Zero-config from existing files | ✅ | ⚠️ (`.nvmrc`) | ⚠️ (`volta` field) | ⚠️ (`.python-version`) | ❌ (needs `.tool-versions`) | ⚠️ (`.nvmrc`, `.python-version`) |
| Ranges resolve to newest match | ✅ | ✅ | ✅ | ❌ (exact only) | ❌ (exact only) | ✅ |
| Checksum verification | ✅ | ⚠️ | ✅ | ⚠️ | ⚠️ (plugin-dependent) | ✅ |
| Resumable downloads | ✅ | ❌ | ❌ | ❌ | ❌ | ⚠️ |
| Atomic installs | ✅ | ⚠️ | ✅ | ⚠️ | ⚠️ | ✅ |
| Cache size / prune commands | ✅ | ❌ | ❌ | ❌ | ❌ | ⚠️ |
| No telemetry | ✅ | ✅ | ⚠️ | ✅ | ✅ | ✅ |

Where runx genuinely differs: it reads the version constraints your project
*already* declares (`package.json` `engines`, `pyproject.toml`
`requires-python`) instead of requiring its own file, and it needs **no shell
integration at all** — no `eval` in your profile, no shims on `PATH`, no
directory hooks.

> Comparisons reflect each tool's documented default behaviour and are
> best-effort; these projects move quickly, so check their current docs before
> relying on a row. Corrections via PR are welcome.

---

# Why runx has near-zero shell overhead

mise, asdf and nvm hook into your shell. `mise activate` runs a hook on
**every prompt render** — every command you type pays its cost, forever, for
the life of the session. asdf's shims pay on every command lookup instead.
runx does none of that: there is no activation line, no shim directory, no
directory hook. **Nothing happens until you type `runx <key>`, and the
overhead is paid once per invocation — not per prompt.**

Measured on macOS 26.5 (Apple silicon, zsh 5.9, hyperfine 1.20, mise
2026.7.18, `--warmup 3 --runs 20`; full method in
[`benchmarks/README.md`](benchmarks/README.md)):

| Measurement | Mean |
| :-- | --: |
| zsh startup, no hooks (baseline) | 23.2 ms |
| zsh startup + `mise activate` (per prompt) | 38.4 ms — **+15.2 ms per prompt** |
| `runx <key>` total, warm cache | 5.9 ms |
| — runx pre-child work (`RUNX_TIMINGS=1`): config | 0.07 ms |
| — cache lookup | 0.02 ms |
| — PATH build + spawn | 0.14 ms |

runx's entire contribution before your command starts is **~0.2 ms, once**.
`mise activate` adds **15.2 ms to every prompt** on the same machine (mise's
own discussion #6279 reports ~80–97 ms first-prompt lag in some
configurations — heavier shells only make it worse). A complete `runx <key>`
run — process spawn, config read, PATH construction, child start — is
**6.6× faster than rendering one mise-activated prompt**, and runx pays
nothing on the prompts before and after.

There is a second, related difference: shim-based tools (asdf, and mise's
shim mode) have had real bugs in the Windows/WSL boundary, where shim files
were misinterpreted as executable scripts under `/mnt/c/...` path semantics.
That is a known class of shim-related fragility that runx's architecture
avoids **by construction** — runx creates no shims of any kind.

These are directional numbers on one machine, not a universal guarantee; the
structural claim — *no shell hook, so no per-prompt cost* — does not depend
on any particular measurement.

---

# Registry freshness: no third-party sync

runx resolves version ranges **directly against each vendor's own release
index** — `nodejs.org/dist/index.json` for Node, the
`astral-sh/python-build-standalone` GitHub releases for Python, GitHub
releases for Bun, and `go.dev/dl` for Go. Results are cached locally for at
most 6 hours (`INDEX_TTL_SECS` in `src/registry.rs`); after that, the next
range resolution refetches from the vendor. There is no sync service, mirror,
or intermediate registry in the path.

That is the structural difference from mise's version registry, which
depends on a third-party sync pipeline: per
[mise's own discussion #7468](https://github.com/jdx/mise/discussions/7468),
its index refreshes roughly every 15 minutes but is rate-limited, so
`mise latest <tool>` can report a version that is **days behind** the actual
latest upstream release. runx's worst case is its own 6-hour cache window —
bounded and self-healing, not unbounded by someone else's rate limit.

**Falsifiable and continuously checked:** `benchmarks/registry-freshness.sh`
fetches the latest version from each vendor's canonical source with plain
`curl`, then has runx resolve a fresh `*` range in an isolated cache, and
compares the two. A weekly CI run
([`registry-freshness` workflow](.github/workflows/registry-freshness.yml))
keeps the claim verified over time;
![registry-freshness status](https://github.com/aryankahar31/runx/actions/workflows/registry-freshness.yml/badge.svg)

Latest verified run (`benchmarks/registry-results.json`):

| Runtime | Vendor latest | runx resolved | Status |
| :-- | :-- | :-- | :--: |
| node | 26.7.0 | 26.7.0 | ✅ |
| go | 1.26.5 | 1.26.5 | ✅ |
| bun | 1.3.14 | 1.3.14 | ✅ |
| python | 3.14.7 | 3.14.7 | ✅ |

Honest caveats: the python index is heavy (10 pages of
python-build-standalone releases, ~15 MB each), so the weekly CI run exercises
it on GitHub's own network, which is the environment runx users in CI hit —
it passes there even when it is too slow for a laptop. The
6-hour cache means a release published within the last 6 hours may not yet
appear in runx's resolution; rerun after the window for a clean check. The
unauthenticated GitHub API (60 requests/hour/IP) is the practical ceiling for
the python and bun lookups; a weekly run uses a fraction of it.

---

# Roadmap

## v0.1

- ✅ Node.js
- ✅ Python
- ✅ Runtime cache
- ✅ GitHub Releases
- ✅ Cross-platform installers
- ✅ GitHub Actions CI/CD
- ✅ SHA-256 checksum verification (v0.1.1)

---

## v0.2

- ✅ Zero-config auto-detection (Node.js + Python from `.nvmrc`, `.node-version`, `package.json`, `.python-version`, `pyproject.toml`)
- ✅ Bun (from `engines.bun` / `packageManager`, or `runx.toml`)
- ✅ Go (from the `go.mod` `go` directive, or `runx.toml`)

---

## v0.3

**Correctness and safety**

- ✅ Strict version validation (closes a path-traversal → cache-deletion / `PATH`-hijack chain)
- ✅ Archive extraction hardening (symlink escape, exec bits preserved)
- ✅ Exact checksum matching (no substring or fall-open matches)
- ✅ Atomic installs — an interrupted download cannot corrupt the cache
- ✅ Connect and idle-read timeouts on every request
- ✅ `cargo clippy -D warnings` enforced in CI

**Features**

- ✅ Correct semver resolution — ranges resolve to the newest matching release
- ✅ `runx.lock` + `--locked` for reproducible installs
- ✅ Cache management (`list`, `size`, `clean`, `prune`)
- ✅ Retry with exponential backoff and resumable downloads
- ✅ `RUNX_HOME` for cache relocation
- ✅ `runx doctor` — diagnose broken cache, corrupt runtimes, `PATH` conflicts
- ✅ Shell completions (bash, zsh, fish, PowerShell)
- ✅ Bun
- ✅ Go
- ✅ Deno (from `.dvmrc` / `.deno-version`, or `runx.toml`)
- ✅ `runx self update` — checks the latest GitHub release, verifies the SHA-256
  checksum, and atomically swaps the binary
- ✅ Argument passthrough (`runx dev -- --port 3000`)
- 🚧 Signature verification (Sigstore/cosign path)

---

## v0.4 and later

- 🚧 Java, .NET
- 🚧 Monorepo / workspace support
- 🚧 Pre/post run hooks
- 🚧 Plugin system and runtime registry

---

## v1.0

- 🚧 Stable API
- 🚧 VS Code Extension
- 🚧 Homebrew
- 🚧 Scoop
- 🚧 Winget
- 🚧 Chocolatey

---

# Contributing

Contributions are welcome.

Please ensure:

- Runtime installers remain portable
- Downloads are deterministic
- Existing tests continue to pass
- New features include tests
- Documentation is updated

Clone the project

```bash
git clone https://github.com/aryankahar31/runx.git

cd runx
```

Run tests

```bash
cargo test
```

Build

```bash
cargo build --release
```

---

# License

This project is licensed under the MIT License.

See the `LICENSE` file for details.

---

<div align="center">

## 🦀 Built with Rust

Portable runtimes.

Deterministic environments.

Zero global installations.

---

⭐ **If Runx helped you, consider giving the repository a star!**

**GitHub**

https://github.com/aryankahar31/runx

</div>
