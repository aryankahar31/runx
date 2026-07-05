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

## Semver range resolution

When a version file contains a range (e.g. `>=3.11`, `^20`, `~20.11`)
rather than an exact version, runx resolves it to the **minimum version
that satisfies the constraint**:

| Input | Resolved to |
|-------|-------------|
| `>=3.11` | `3.11.0` |
| `^20` | `20.0.0` |
| `~20.11` | `20.11.0` |
| `>=20.11.0` | `20.11.0` |
| `20.11.0` | `20.11.0` (exact, no change) |

This is a documented simplification. Runx always prints which version was
chosen and from which file so there are no silent surprises.

## Run-command inference

For the inferred `dev` command runx checks whether `package.json` contains
a `"dev"` script and runs `npm run dev` if so.  No other commands are
guessed.  If a dev command cannot be inferred, runx prints a clear error
and suggests running `runx init`.

## Example output

With only a `.nvmrc` and a `package.json` that has a `dev` script:

```
No runx.toml found — detected from project files:
  node 20.11.0 (from .nvmrc)
Installing node 20.11.0
...
Running `npm run dev`
```

On subsequent runs (runtime already cached):

```
No runx.toml found — detected from project files:
  node 20.11.0 (from .nvmrc)
Using cached node 20.11.0 at /home/user/.runx/runtimes/node/20.11.0
Running `npm run dev`
```

## Opt-in-by-absence guarantee

- If `runx.toml` exists → it is the sole source of truth. Auto-detection
  is never consulted, and the file is never modified.
- Auto-detection is the fallback *only* when no `runx.toml` is present.
- Auto-detection **never writes to disk**. To persist a detected
  configuration, run `runx init` which creates a starter `runx.toml`.

---

# ✨ Features

- 🚀 Zero global runtime installation
- 📦 Automatic runtime downloads
- 💾 Intelligent runtime cache
- 🔒 Isolated execution environment
- 🛡 SHA-256 checksum verification on install
- ⚡ Fast startup after first download
- 🖥 Cross-platform (Linux, macOS, Windows)
- ⚙ Configuration using `runx.toml`
- 🦀 Built with Rust
- 🔄 Deterministic project environments
- 🔧 GitHub Releases & CI/CD

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

> **🛡 Security:** Both install scripts automatically verify the downloaded
> binary against the SHA-256 checksum published with each release. If the
> checksum doesn't match, the installer will abort without extracting or
> installing anything.

Verify installation

```bash
runx --version
```

Expected output

```
runx 0.2.0
```

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
| Bun | 🚧 Planned |
| Deno | 🚧 Planned |
| Go | 🚧 Planned |
| Java | 🚧 Planned |
| .NET | 🚧 Planned |

---

# CLI Commands

Initialize configuration

```bash
runx init
```

Run project command

```bash
runx dev
```

Build project

```bash
runx build
```

Show version

```bash
runx --version
```

Display help

```bash
runx --help
```

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

Runx never modifies

- Global PATH
- Shell startup files
- System-installed runtimes
- User environment

Instead, every command runs inside an isolated environment using only the configured runtimes.

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
| Zero-config detection from existing files | ✅ | ⚠️ (only `.nvmrc`) | ⚠️ (only `volta` field) | ⚠️ (only `.python-version`) | ❌ (requires `.tool-versions`) | ⚠️ (supports `.nvmrc`/`.python-version`, not `package.json` engines or `pyproject.toml`) |
| Shell integration required | ❌ (none) | ✅ (required) | ❌ (none) | ✅ (required) | ✅ (required) | ⚠️ (optional; needed for ambient switching) |

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
- 🚧 Bun
- 🚧 Deno
- 🚧 Go
- 🚧 Java

---

## v0.3

- 🚧 Runtime registry
- 🚧 Plugin system
- 🚧 Cache management
- 🚧 Self update

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
