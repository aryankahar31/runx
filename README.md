<div align="center">

# 🚀 Runx

### Universal Project Launcher with Portable Runtimes

Run projects with the exact runtime versions they require — **without installing Node.js, Python, or other runtimes globally.**

[![CI](https://github.com/aryankahar31/runx/actions/workflows/ci.yml/badge.svg)](https://github.com/aryankahar31/runx/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/aryankahar31/runx)](https://github.com/aryankahar31/runx/releases)
[![License](https://img.shields.io/github/license/aryankahar31/runx)](LICENSE)
[![Rust](https://img.shields.io/badge/Built%20With-Rust-orange)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Linux%20%7C%20macOS%20%7C%20Windows-blue)](https://github.com/aryankahar31/runx)

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

# ✨ Features

- 🚀 Zero global runtime installation
- 📦 Automatic runtime downloads
- 💾 Intelligent runtime cache
- 🔒 Isolated execution environment
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

Verify installation

```bash
runx --version
```

Expected output

```
runx 0.1.0
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

| Feature | Runx | nvm | Volta | pyenv | asdf |
|----------|------|------|--------|--------|------|
| Node.js | ✅ | ✅ | ✅ | ❌ | ✅ |
| Python | ✅ | ❌ | ❌ | ✅ | ✅ |
| Multiple runtimes | ✅ | ❌ | ❌ | ❌ | ✅ |
| Runtime cache | ✅ | ✅ | ✅ | ✅ | ✅ |
| Project launcher | ✅ | ❌ | ❌ | ❌ | ❌ |
| Cross-platform | ✅ | ⚠️ | ✅ | ⚠️ | ✅ |

---

# Roadmap

## v0.1

- ✅ Node.js
- ✅ Python
- ✅ Runtime cache
- ✅ GitHub Releases
- ✅ Cross-platform installers
- ✅ GitHub Actions CI/CD

---

## v0.2

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
