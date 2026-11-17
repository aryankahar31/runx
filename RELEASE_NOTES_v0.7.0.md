# runx v0.7.0

v0.7.0 adds Eclipse Temurin Java support, lock enforcement for CI, and a set of
daily-use commands for inspecting and running projects.

## What's new

- **Java 9+** via Eclipse Temurin, with Adoptium release metadata and SHA-256
  verification. `.java-version` and `.sdkmanrc` (`java=`) are detected.
- **Daily-use commands:** `runx list`, `runx which`, `runx exec`,
  `runx upgrade`, `runx cache dir`, and `runx cache prune --dry-run`.
- **CI lock enforcement:** `--frozen` combines `--locked --offline`, and
  `RUNX_LOCKED=1` applies `--locked` to run commands.
- **Dependency installs:** pnpm and Yarn honor `packageManager` pins through
  Corepack with an `npx` fallback; `uv.lock` and `poetry.lock` take precedence
  over pip detection.
- **CI and containers:** a composite `setup-runx` GitHub Action can cache
  runtimes and enforce `runx.lock`. A multi-stage Dockerfile recipe and usage
  guide are included.
- **Packaging groundwork:** Cargo/cargo-binstall metadata and templates for
  Homebrew, Winget, Scoop, AUR, Nix, and npm.
- **Documentation and community files:** a zero-admin guide, security and
  conduct policies, roadmap, issue templates, scoped issue drafts, launch
  drafts, and a VHS demo.

## Verification

- `cargo fmt -- --check` and `cargo clippy -- -D warnings` pass on macOS.
- `cargo test`: 540 passed, 0 failed; one real-release cosign test is ignored
  by default.
- The ignored cosign test passed against the existing v0.4.2 release archive.
- `packaging/validate.sh` and the npm wrapper's two unit tests pass. These do
  not validate downstream package-manager submissions.

## Known limitations

- Java's Adoptium response parsing and asset-selection logic have unit tests;
  a full Temurin download and install was not exercised end-to-end.
- The Dockerfile was not built in CI or locally, and no container image is
  published.
- The `setup-runx` action has not been exercised by a consumer workflow.
- Homebrew, Winget, Scoop, AUR, Nix, Cargo, and npm publication require separate
  manual steps. The Winget draft is only an installer manifest and needs
  companion manifests and schema validation; Nix still needs source and Cargo
  hashes and corrected platform metadata. Windows ARM64 has no native runx
  release asset.
- The npm wrapper's `RUNX_VERSION` override needs path validation before that
  package is published.
- The packaging renderer's checksum-error path needs to fail closed before it
  is used to generate downstream package manifests.
- Ruby, PHP, and .NET are not included in this release.

[Full changelog](https://github.com/aryankahar31/runx/compare/v0.6.2...v0.7.0)
