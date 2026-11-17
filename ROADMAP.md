# Roadmap

runx's goal is narrow: **run any project with the runtimes it declares, without
admin rights or shell integration.** Features that do not serve that sentence
are usually declined.

Status is honest: shipped means tested and in a release; planned means wanted,
not promised; not planned includes requests we have deliberately rejected and
why.

## Shipped

- **v0.6.x** — Node, Python, Bun, Go, Deno; zero-config detection; checksum
  verification; atomic installs; lockfiles; `runx doctor`; cache management;
  self-update; Sigstore-signed runx releases.
- **v0.7.0**
  - Eclipse Temurin Java 9+ (Adoptium metadata + checksums)
  - `runx list`, `runx which`, `runx exec`, `runx upgrade`,
    `runx cache dir`, `runx cache prune --dry-run`
  - `--frozen` and `RUNX_LOCKED=1` for CI-grade lock enforcement
  - `packageManager` pins for pnpm/Yarn; uv and Poetry lockfile support
  - install channels: Cargo metadata, cargo-binstall, Homebrew, Winget, Scoop,
    AUR, Nix, npm wrapper
  - `setup-runx` GitHub Action and a container image recipe
  - [zero-admin guide](docs/zero-admin.md)

## Planned (ordered by demand, not dates)

- **More runtimes** where a vendor publishes portable, checksummed archives:
  .NET, Ruby, PHP, Zig. Each needs a trustworthy archive and digest source; a
  runtime without one will not ship.
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
- **Ruby, PHP, and .NET in v0.7.0.** Checked and skipped rather than shipped
  weakly: `ruby/ruby-builder` publishes no checksums for its portable tarballs
  and has no Windows build; PHP has no official portable checksummed binaries;
  .NET uses SHA-512 and four-part versions, which do not fit runx's exact
  three-part version and SHA-256 lockfile model. These can return when the
  archive/verification gap is solvable, not by relaxing validation.

## How to influence it

Open a feature request with the project, platform, and runtime you need. A
reproduction with a real repository is worth more than a +1. Security work and
bug fixes jump the queue.
