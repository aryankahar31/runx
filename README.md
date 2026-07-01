# 🚀 runx

> Universal project launcher with portable runtimes.

**runx** lets you run projects without installing language runtimes globally.

It automatically downloads the exact runtime versions your project requires, caches them locally, and executes commands in an isolated environment.

No global Node.js. No global Python. No version managers.

---

## ✨ Features

- 📦 Zero global runtime installation
- ⚡ Automatic runtime downloads
- 🔒 Isolated project execution
- 🚀 Fast runtime cache
- 🖥️ Cross-platform (Linux, macOS, Windows)
- 🔧 Configuration via `runx.toml`
- 🦀 Written in Rust
- 🎯 Reproducible development environments

---

# Why runx?

Managing runtimes across projects is painful.

Different projects require different versions of:

- Node.js
- Python
- (More runtimes coming soon)

Instead of installing version managers or changing your global environment, **runx** downloads exactly what the project needs and runs everything in an isolated environment.

Your system remains clean.

---

# Installation

## macOS / Linux

```bash
curl -fsSL https://raw.githubusercontent.com/aryankahar31/runx/main/install.sh | sh
```

## Windows (PowerShell)

```powershell
iwr https://raw.githubusercontent.com/aryankahar31/runx/main/install.ps1 | iex
```

After installation, ensure the following directory is available in your PATH.

Linux/macOS

```text
~/.runx/bin
```

Windows

```text
%USERPROFILE%\.runx\bin
```

---

# Quick Start

Create a project configuration.

```toml
[runtimes]
node = "20.11.0"
python = "3.11.7"

[run]
dev = "npm run dev"
build = "npm run build"
test = "npm test"
```

Initialize a new project:

```bash
runx init
```

Run a command:

```bash
runx dev
```

First execution:

```
Installing node 20.11.0
Downloading...
Extracting...
Running `node --version`
v20.11.0
```

Second execution:

```
Using cached node 20.11.0
Running `node --version`
v20.11.0
```

No additional downloads.

---

# Example

Project structure

```
my-project/
│
├── runx.toml
├── package.json
└── src/
```

Run:

```bash
runx dev
```

runx automatically:

1. Reads `runx.toml`
2. Resolves required runtimes
3. Downloads missing runtimes
4. Uses cached versions when available
5. Launches the command with an isolated PATH

---

# Runtime Cache

Downloaded runtimes are stored in

```
~/.runx/runtimes/<runtime>/<version>/
```

Example

```
~/.runx/runtimes/node/20.11.0/
~/.runx/runtimes/python/3.11.7/
```

Cached runtimes are automatically reused.

---

# Supported Runtimes (MVP)

| Runtime | Status |
|----------|--------|
| Node.js | ✅ |
| Python | ✅ |

---

# Commands

```
runx init
```

Create a starter `runx.toml`.

```
runx dev
```

Execute the `dev` command.

```
runx build
```

Execute the `build` command.

```
runx --version
```

Show installed version.

```
runx --help
```

Display CLI help.

---

# Build From Source

Clone the repository.

```bash
git clone https://github.com/aryankahar31/runx.git

cd runx
```

Build

```bash
cargo build --release
```

Binary location

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
 ├── Parse runx.toml
 │
 ├── Resolve runtime versions
 │
 ├── Check local cache
 │
 ├── Download runtime (if missing)
 │
 ├── Extract portable runtime
 │
 ├── Cache runtime
 │
 └── Launch command with isolated PATH
```

---

# Isolation

runx **never** modifies:

- Global PATH
- System runtimes
- Shell startup files
- User environment

Instead, it creates an isolated execution environment for each command.

---

# Roadmap

### Version 0.x

- ✅ Node.js
- ✅ Python
- ✅ Runtime cache
- ✅ Cross-platform installers
- ✅ GitHub Releases
- ✅ GitHub Actions CI

### Planned

- Bun
- Deno
- Java
- Go
- .NET
- Plugin system
- Runtime registry
- Self update
- VS Code extension

---

# Contributing

Contributions are welcome.

Please ensure:

- Runtime installers remain portable
- Downloads are deterministic
- Existing tests continue to pass
- New runtime behavior includes tests

---

# License

MIT License

---

<div align="center">

**Built with ❤️ in Rust**

⭐ Star the repository if you find runx useful.

https://github.com/aryankahar31/runx

</div>
