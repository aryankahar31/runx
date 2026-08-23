# Lockfiles (`runx.lock`)

Ranges resolve against the current release list, so the same project can pick up a newer runtime next month. When that is not wanted — CI, a team, a release branch — generate a lockfile:

```bash
runx lock
```

This installs the runtimes, then writes `runx.lock` pinning exactly what was resolved. Commit it.

```toml
version = 1

[runtimes.node]
version = "20.11.0"
requirement = ">=20"

[runtimes.node.artifacts.macos-aarch64]
url = "https://nodejs.org/dist/v20.11.0/node-v20.11.0-darwin-arm64.tar.gz"
sha256 = "94e443d007e2882f8e5aecc85d978f7591520dc3b642adc7583b3cb0b3fc37d7"
```

Artifacts are keyed by platform because runtime archives *are* platform-specific: Node 20.11.0 on macOS/arm64 is a different file with a different digest than on Linux/x64. The **version** pin is shared across platforms; the digest is a per-platform integrity check on top. Running `runx lock` on macOS does not discard a teammate's Linux entry.

## Enforcing the lockfile in CI

```bash
runx run test --locked
```

`--locked` fails rather than resolving anything the lockfile does not already pin, mirroring `cargo build --locked`. It fails when a runtime is missing from the lockfile, or when `runx.toml` asks for a requirement the lockfile does not record.

A missing entry for *your platform* is deliberately not fatal, even under `--locked`: the version is still pinned, and the download is still verified against the publisher's own checksums. A mixed-OS team is not blocked by a lockfile generated elsewhere.

## Precedence

`runx.toml` always wins. If someone bumps a version there without re-running `runx lock`, runx uses the config and warns that the lockfile is stale — a lockfile that silently overrode an explicit version bump would be baffling to debug.
