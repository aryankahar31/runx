# Contributing to runx

## Prerequisites

A stable Rust toolchain via [rustup](https://rustup.rs), including the
`rustfmt` and `clippy` components — CI runs all three.

## Build and test

```bash
git clone https://github.com/aryankahar31/runx.git && cd runx

cargo fmt -- --check                                      # formatting
cargo clippy --all-targets --all-features -- -D warnings  # lints, warnings are errors
cargo test                                                # full suite
cargo build --release                                     # produce target/release/runx
```

All four must pass locally before opening a PR — CI runs exactly these on
every PR (see `.github/workflows/ci.yml`). For a fast iteration loop on the
integration suite alone:

```bash
cargo test --test cli_integration <name-filter>
```

## Workflow: branch → PR → CI → merge

1. Branch from `main` using a scoped name (`feat/…`, `fix/…`, `docs/…`,
   `chore/…`).
2. Open a PR against `main`. Never push directly to `main`.
3. Wait for CI to be green on **all three** platforms. The required checks are
   `Test (ubuntu-latest)`, `Test (macos-latest)` and `Test (windows-latest)`,
   enforced strictly (the branch must be up to date with `main`) by the
   `main-protection` ruleset.
4. Merge only after all three checks are green.

Commit messages follow the existing style: a `feat(scope):`, `fix(scope):`,
`docs:`, `chore:` or `test:` prefix.

## How releases are cut

1. Bump the version in `Cargo.toml` through a normal PR
   (`chore: bump version to X.Y.Z`) and merge it.
2. Wait for the CI workflow run **on `main` for that exact merge SHA** to be
   green.
3. Tag that SHA `vX.Y.Z` and push the tag.
4. The Release workflow (`.github/workflows/release.yml`) refuses to build
   unless a successful `ci.yml` **push** run exists for the tagged SHA (its
   `verify-ci` job). It then builds all platform archives, signs them with
   cosign keyless, and publishes the GitHub release with checksums.

Never tag a commit whose CI push run has not finished green — the release
workflow will fail the tag rather than publish unverified artifacts.
