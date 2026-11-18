# Roadmap

runx's goal is narrow: **run any project with the runtimes it declares, without
admin rights or shell integration.** Features that do not serve that sentence
are usually declined.

Status is honest: released means tested and included in a published release;
prepared means implemented but not yet submitted to its downstream channel;
planned means wanted, not promised; not planned includes requests we have
deliberately rejected and why.

## Released

- **v0.6.x** — Node, Python, Bun, Go, Deno; zero-config detection; checksum
  verification; atomic installs; lockfiles; `runx doctor`; cache management;
  self-update; Sigstore-signed runx releases.
- **v0.7.0** — Eclipse Temurin Java 9+, lock enforcement, daily-use commands,
  dependency lockfile support, packaging templates, and the `setup-runx` action.
- **v0.7.1** — renamed the crates.io package to `runx-launcher`; published the
  crate and five signed platform archives while keeping the command and Rust
  library crate named `runx`.

## Prepared for downstream submission

- npm, Homebrew, Winget, Scoop, AUR, and Nix templates use `runx-launcher`;
  their downstream submissions remain pending. Winget schema validation and
  Nix source/Cargo hashes still need manual work.

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
