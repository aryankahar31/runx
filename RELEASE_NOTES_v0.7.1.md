# runx v0.7.1

v0.7.1 makes the crates.io package name unambiguous. The package is
`runx-launcher`; the installed executable, CLI command, and Rust library crate
remain `runx`. GitHub release asset names remain `runx-*`.

## Install

The crates.io package is published and available:

```sh
cargo install runx-launcher
# or, with cargo-binstall:
cargo binstall runx-launcher
```

Both commands install the `runx` binary. The install scripts and GitHub release
binaries install the same command. npm, Homebrew, Winget, Scoop, AUR, and Nix
packages remain planned / pending their separate submissions.

## Verification

- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test`
- `cargo package --list` and `cargo publish --dry-run`
- `packaging/validate.sh` and `cd packaging/npm && npm test`
- Install check: `/tmp/runx-install-test/bin/runx --version` prints `0.7.1`

## Inherited limitations

- Java is Temurin 9+; a full Temurin download and install has not been exercised
  end-to-end.
- The Dockerfile is a reviewed recipe, not a CI-built or published image.
- Ruby, PHP, and .NET are not supported because portable verified artifacts
  have not met the project's requirements.
- Winget schema validation and Nix source/Cargo hashes still require manual
  work before submission.

[Full changelog](https://github.com/aryankahar31/runx/compare/v0.7.0...v0.7.1)
