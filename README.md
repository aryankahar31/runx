# runx

`runx` is a universal project launcher. It reads `runx.toml`, downloads exact portable runtime versions into `~/.runx`, and runs project commands with an isolated `PATH`.

## Install

macOS/Linux:

```sh
curl -fsSL https://raw.githubusercontent.com/your-org/runx/main/install.sh | sh
```

Windows PowerShell:

```powershell
iwr https://raw.githubusercontent.com/your-org/runx/main/install.ps1 | iex
```

The installer expects release assets named `runx-linux-x64.tar.gz`, `runx-linux-arm64.tar.gz`, `runx-macos-x64.tar.gz`, `runx-macos-arm64.tar.gz`, `runx-windows-x64.zip`, and `runx-windows-arm64.zip`.

## Build From Source

```sh
cargo build --release
```

The binary is written to `target/release/runx` or `target\release\runx.exe`.

## Configuration

Create `runx.toml` in your project:

```toml
[runtimes]
node = "20.11.0"
python = "3.11.7"

[run]
dev = "npm run dev"
build = "npm run build"
```

Run any key under `[run]`:

```sh
runx dev
runx build
```

Initialize a starter config:

```sh
runx init
```

## Commands

```sh
runx --help
runx --version
runx init
runx <command>
```

## Runtime Cache

Runtimes are installed under:

```text
~/.runx/runtimes/<tool>/<version>/
```

If the expected executable already exists in that directory, `runx` treats it as a cache hit and skips the download.

Supported runtimes in this MVP:

- Node.js from official `nodejs.org/dist` archives.
- Python from `astral-sh/python-build-standalone` portable release archives.

## Isolation

`runx` spawns the configured command as a child process. It prepends only the cached runtime bin directories to a minimal safe system `PATH`, then inherits the rest of the environment. It does not modify shell startup files, global `PATH`, or system runtime installs.

## Contributing

Keep runtime installers portable, deterministic, and admin-free. Add tests for config parsing and cache/executor behavior when changing those surfaces.
