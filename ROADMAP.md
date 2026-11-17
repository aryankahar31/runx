# Roadmap

runx's goal is narrow: **run any project with the runtimes it declares, without
admin rights or shell integration.** Features that do not serve that sentence
are usually declined.

Status is honest: released means tested and included in a published release;
prepared means implemented on the v0.7.0 release branch but not released yet;
planned means wanted, not promised; not planned includes requests we have
deliberately rejected and why.

## Released

- **v0.6.x** — Node, Python, Bun, Go, Deno; zero-config detection; checksum
  verification; atomic installs; lockfiles; `runx doctor`; cache management;
  self-update; Sigstore-signed runx releases.

## Prepared for v0.7.0

- Eclipse Temurin Java 9+ (Adoptium metadata and SHA-256 checksums; full archive
  download/install has not been exercised end-to-end)
- `runx list`, `runx which`, `runx exec`, `runx upgrade`, `runx cache dir`, and
  `runx cache prune --dry-run`
- `--frozen` and `RUNX_LOCKED=1` lock enforcement
- `packageManager` pins for pnpm/Yarn; uv and Poetry lockfile support
- Cargo/cargo-binstall metadata and packaging templates for Homebrew, Winget,
  Scoop, AUR, Nix, and npm. Downstream package submissions are not published;
  Winget and Nix templates need additional release-specific work.
- `setup-runx` GitHub Action and an unbuilt container recipe
- [zero-admin guide](docs/zero-admin.md)

## Planned (ordered by demand, not dates)

- **More runtimes** where a vendor publishes portable archives that meet runx's
  version and verification requirements: .NET, Ruby, PHP, Zig. A runtime that
  cannot meet those requirements will not ship.
- **`runx env`** — print the resolved environment (`PATH`, versions) for shell
  debugging without activating anything.
- **Per-runtime mirrors** — a documented config knob for teams behind a proxy,
  with digest pinning preserved.
- **Stronger cache verification** — optional full-tree hashing on unlocked runs
  for people who want it, without making the default slower.

## Not planned

- **Global runtime installs or ambient version switching.** That is nvm/asdf/
  mise's job. runx deliberately only changes the child process's `PATH`.
- **Shell activation / prompt hooks / shims.** The no-shell-integration property
  is the product, not an implementation detail.
- **Telemetry.** None now, none planned.
- **A package registry or dependency resolver.** runx installs runtimes and
  delegates dependencies to npm/pnpm/pip/uv/poetry/go/etc.
- **Ruby, PHP, and .NET in v0.7.0.** They were left out because the
  distribution and version models evaluated for this release did not meet the
  project's portability and integrity requirements. Support can be
  reconsidered when suitable artifacts are available, without relaxing
  validation.

## How to influence it

Open a feature request with the project, platform, and runtime you need. A
reproduction with a real repository is worth more than a +1. Security work and
bug fixes jump the queue.
