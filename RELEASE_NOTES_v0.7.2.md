# runx v0.7.2

v0.7.2 fixes a bug in `runx init` when generating `runx.toml` from `package.json` scripts whose names contain special characters ([#73](https://github.com/aryankahar31/runx/issues/73)).

## Fixed

- **`runx init` now quotes TOML keys containing special characters (`:`, `.`, `/`, `@`, spaces, quotes, backslashes, unicode) and validates the generated `runx.toml` before and after writing** ([#73](https://github.com/aryankahar31/runx/issues/73), [#74](https://github.com/aryankahar31/runx/pull/74), [#77](https://github.com/aryankahar31/runx/pull/77)). Previously, `package.json` scripts such as `test:rules` or `worker:typecheck` were written as bare keys in `[run]`, causing subsequent commands (`runx install`, `runx doctor`, `runx test:rules`) to fail with a TOML parse error (or an unquoted `.` to silently create a nested table).
- **Hand-written `runx.toml` files with unquoted `:` or `.` keys now include an actionable quoting hint in the parse error** (for example, `Hint: keys containing ':' or '.' must be quoted, e.g. "test:rules" = "npm run test:rules"`).

## Upgrade

```bash
runx self update
```

- **Upgrading from v0.6.x:** run `runx self update` (the `runx upgrade` alias was introduced in v0.7.0 and does not exist in v0.6.x).
- If `runx --version` still shows an older version after installing via a second channel, run `which -a runx` and remove the duplicate binary.

## Install

The package name on crates.io, Homebrew, and npm is `runx-launcher`; the installed CLI binary and Rust library crate are named `runx`.

- **Install script (macOS / Linux):**
  ```bash
  curl -fsSL https://raw.githubusercontent.com/aryankahar31/runx/main/install.sh | sh
  ```
- **Install script (Windows PowerShell):**
  ```powershell
  iwr https://raw.githubusercontent.com/aryankahar31/runx/main/install.ps1 | iex
  ```
- **Cargo / crates.io:**
  ```bash
  cargo install runx-launcher
  # or, with cargo-binstall:
  cargo binstall runx-launcher
  ```
- **Homebrew (macOS / Linux):**
  ```bash
  brew install aryankahar31/tap/runx-launcher
  ```
- **npm (Node 18+):**
  ```bash
  npx runx-launcher dev
  # or install globally:
  npm install -g runx-launcher
  ```

## Inherited limitations

- The `Dockerfile` is a reviewed recipe, not a CI-built or published container image.
- Java is Eclipse Temurin 9+; a full Temurin download and install has not been exercised end to end in CI.
- Ruby, PHP, and .NET are not supported because portable verified artifacts have not met the project's requirements.
- Winget, Scoop, AUR, and Nix packages remain pending their separate downstream submissions.

**Full Changelog**: https://github.com/aryankahar31/runx/compare/v0.7.1...v0.7.2
