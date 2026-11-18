# Zero-admin projects

runx is built for machines where you are not root: shared CI runners, locked-down
work laptops, coding interviews, conference Wi-Fi, and containers that are not
yours to rebuild.

Nothing runx does requires `sudo`, an administrator token, or a system package
manager. A project can go from clone to running with only a user-writable
directory.

## What lands where

| Thing | Location | Needs admin? |
|-------|----------|--------------|
| `runx` binary | `~/.runx/bin/runx` (override: `RUNX_INSTALL_DIR`) | No |
| Runtimes | `~/.runx/runtimes/<tool>/<version>` (override: `RUNX_HOME`) | No |
| Lockfile | the project's `runx.lock` (committed by you) | No |
| Downloads | streamed to the cache, never `/usr/local` or `C:\Program Files` | No |

`RUNX_HOME` moves the whole cache, which is useful when the home directory is
small, read-only, or wiped between CI jobs:

```sh
export RUNX_HOME="$PWD/.runx-cache"
runx dev
```

`runx doctor` prints the active cache path and size. `runx cache size` reports
it without a full diagnostic.

## What runx deliberately never does

- No system-wide install: no `apt`, `brew`, `choco`, or registry writes.
- No silent shell edits. The installer may *offer* to add `~/.runx/bin` to your
  `PATH`; decline it with `RUNX_INSTALL_NO_MODIFY_PATH=1` and export the path
  yourself. runx itself never edits shell files.
- No global runtime versions: each project resolves its own, and child processes
  get that project's `PATH`, not yours.
- No telemetry.

## Working without a global runtime

You never install Node, Python, or Go system-wide first. runx downloads the
project's runtime into its own cache and puts only that directory on the child
process's `PATH`. There is no implicit fallback to a system runtime, so a
machine with a stale `/usr/bin/python3` cannot silently change what your project
runs. Version precedence is explicit:

1. `runx.toml` (always wins),
2. `runx.lock`,
3. project files (`.nvmrc`, `.python-version`, `package.json`, ...).

To keep a project fully offline, pin exact versions and populate the cache once:

```sh
runx lock
runx install
runx --offline --frozen dev
```

`--offline` blocks runx's own network operations; it does not sandbox the
project's own networking.

## Limits worth knowing

- runx installs language runtimes, not system libraries. A project that needs a
  C toolchain, `libssl`, or a GPU driver still needs the normal machine setup.
- Corporate TLS interception can break downloads unless your proxy CA is in the
  system trust store. runx uses the platform's TLS roots.
- Some ISPs throttle Fastly-hosted GitHub assets. The [README](../README.md)
  documents the jsDelivr mirror for the install script only; runtime downloads
  come from nodejs.org, go.dev, adoptium.net, and GitHub release assets for
  Python and Bun.

## Verifying it on a clean machine

The published crates.io package is `runx-launcher`; `cargo install
runx-launcher` installs the `runx` command.

```sh
RUNX_HOME=/tmp/runx-clean RUNX_INSTALL_DIR=/tmp/runx-clean/bin \
  sh install.sh
/tmp/runx-clean/bin/runx doctor
```

No privileged command appears anywhere in that transcript.
