# Changelog

All releases from v0.1.0 to v0.6.0, reconstructed from the repository's git
tags and each tag's GitHub release notes (`gh release view`). Entries marked
*(from git)* were reconstructed from the commit range between tags because
that release has no notes; every line is verifiable against the linked
compare view or commit.

## Unreleased

### Fixed

- **A dependency declared in `package.json` but absent from the lockfile is now
  detected.** This is the state `npm ci` rejects with `EUSAGE`, but
  `runx install` reported "Dependencies already installed", `runx doctor`
  reported "installed and up to date", and both exited 0 — because the
  installed tree was a faithful mirror of a lockfile that was itself stale.
  `dep_state` now checks the two manifests against each other before comparing
  the tree, and names the missing package.
- **`runx doctor` no longer prints "everything looks healthy" above its own
  warnings.** A project whose dependencies were not installed got a `~` line
  and a healthy verdict in the same output, while `runx dev` exited 1. Warnings
  now withhold the verdict, worded "unresolved checks" rather than "cannot run"
  since some (an absent `runx.lock`) pin nothing but stop nothing. The exit code
  still reflects cache health only, so doctor's contract as a gate is unchanged.
- **A `package.json` with no dependencies is no longer reported as having
  uninstalled ones.** A scripts-only manifest was told to run `npm install` on a
  directory where `runx dev` already exited 0.
- **A runx flag after the command key is named as such.** `runx dev --install`
  advised `runx dev -- --install`, which forwards the flag to the child where it
  does nothing. It now gives `runx --install dev`, and `--locked` gets
  `runx run dev --locked`, its only working form.
- **The missing-dependency warning prints before the command, not after it.** A
  Node `MODULE_NOT_FOUND` dump is ~20 lines, which pushed the one actionable
  line off the visible screen.
- **A 404 on a release archive says the version does not exist.** `runx dev` on
  `node 99.99.99` reported a raw `status code 404` against a URL.
- **`runx install` without a lockfile says how to create one.** The hint listed
  the lockfiles runx accepts but not the command that produces one.

### Documentation

- README records that `--locked` costs seconds rather than milliseconds (~2.1 s
  versus 133 ms for the same run), since it re-extracts and compares every
  runtime archive on each invocation. The headline benchmark covers unlocked
  runs only.

## v0.6.1 — 2026-11-02

- [fix: address the software testing & UX review (RX-UX-01..08) (#53)](https://github.com/aryankahar31/runx/pull/53)

Addresses the software testing & UX review (RX-UX-01..08). Behaviour that
already worked is unchanged: a plain `runx dev` still only hints and never
installs, `--locked` still never creates or rewrites `runx.lock`, and
`runx init` still refuses to overwrite an existing config.

### Added

- Project readiness checks shared by `runx install`, `--offline` runs and
  `runx doctor`: runtime, dependency and lockfile state, each with a
  pass/warn/fail reason and the command that resolves it (RX-UX-02).
- `--offline` prints runtime cache status, dependency state and the network
  policy before doing any work, and names the missing prerequisite instead of
  surfacing a refused download URL (RX-UX-03).
- `runx doctor` reports a project-readiness section covering runtimes,
  dependencies and the lockfile, plus total cache size, and no longer claims
  "everything looks healthy" while a prerequisite is missing (RX-UX-05/06).
- `runx init` summarises what was detected (runtimes with their source files,
  package manager, inferred commands) and recommends the next command
  (RX-UX-08).
- 18 integration tests automating cases T-03..T-11 from the review, including
  a cross-platform PATH isolation regression test that reads PATH from a probe
  process before and after a run and asserts equality (RX-UX-07). It runs on
  the Ubuntu, macOS and Windows CI runners.
- README: a table of what `runx install`, `--install`, `runx dev` and
  `runx lock` each prepare, and the four dependency states (RX-UX-02).

### Changed

- `runx install` distinguishes already-installed, lockfile-out-of-sync,
  install-failed and nothing-to-install. A failure now states the detected
  condition, whether any files changed, and the repair command — for an
  out-of-sync npm lockfile that is `npm install`, since `npm ci` (which runx
  runs) refuses that state by design (RX-UX-01).
- When `npm ci` fails because `package.json` and `package-lock.json` disagree,
  the reported condition says so and names the packages missing from the
  lockfile. Detected by comparing the two manifests, so it is accurate
  regardless of npm's wording; npm's own output is still shown in full.
- Dependency staleness is decided from evidence, not file timestamps. Previously
  a lockfile with a newer mtime than `node_modules` was reported as "changed
  since the last install", which fired on any freshly copied or checked-out
  project: `cp -R` writes `node_modules` a few hundred microseconds before the
  lockfile. The check now compares the lockfile's packages against
  `node_modules/.package-lock.json`, npm's record of what it installed, and
  warns only when a required package is genuinely absent. Lockfile entries
  marked `"optional": true` are excluded, because npm records every platform's
  binary but installs only the host's — a healthy `esbuild` tree is missing 22
  of its 25 lockfile entries. Managers that record no equivalent manifest
  (Bun, Deno, Go, pip) are never reported stale.
- A `--locked` run against a runtime cached before archive retention explains
  that the release archive was never kept, that ordinary runs are unaffected,
  and how to replace the entry. It previously failed with a raw
  `No such file or directory (os error 2)`. runx does not delete or reinstall
  the cached runtime itself: `--locked` exists to verify, and removing a
  multi-hundred-megabyte runtime (or re-downloading it) as a side effect of a
  verification flag would be a surprise, and would break `--offline --locked`.
- `runx doctor` warns when a cached runtime has no retained archive, naming the
  `--locked` impact and the fix.
- `--locked` verification no longer prints `Extracting to <tempdir>`. It
  re-extracts the archive to compare trees, which is internal bookkeeping about
  a path the user cannot act on.
- `runx doctor` prints cache-wide findings under the `Cache:` heading instead of
  under `Project readiness`, which had put unrelated runtimes (a cached Python
  with no recorded digest, say) inside the readiness section of a project that
  does not use them. Project readiness lists only the runtimes the project
  declares.
- A malformed `runx.lock` reports a runx-level error naming the file and the
  recovery, with the parser's first line quoted, instead of a bare TOML parse
  error and caret diagram (RX-UX-04).
- The four `runx cache` actions are named in top-level help, and `runx doctor`'s
  help states what it checks (RX-UX-05/06).

### Exit codes

Unchanged. Every `runx install` outcome exits as it did in v0.6.0; only the
messages gained detail.

| Outcome | Exit code |
|---------|-----------|
| Dependencies installed | 0 |
| Already installed | 0 |
| Lockfile out of sync (install attempted, manager failed) | 1 |
| Install failed for any other reason | 1 |
| Nothing to install — no recognised manifest in the project | 1 |

"Nothing to install" exiting non-zero is pre-existing behaviour, not a change:
`runx install` in a project with no lockfile or manifest has always been an
error, on the grounds that the command was asked to do something it cannot do.
Scripts that treat it as success must continue to special-case it.

### Not reproduced

Investigated with clean fixtures; no code change was warranted.

- RX-UX-04's core concern: `--locked` never creates or rewrites the lockfile.
  Missing, stale and platform-incomplete lockfiles already produced distinct
  errors naming the fix, and the file was byte-identical afterwards in every
  case. Only the malformed-file message needed work.
- RX-UX-07's leak: the parent PATH was already unmodified — nothing outside
  test code mutates the process environment. The gap was the missing
  regression test, which is now in place.

[Full changelog](https://github.com/aryankahar31/runx/compare/v0.6.0...v0.6.1)

## v0.6.0 — 2026-10-01

- [fix: locked execution hardening + missing-deps hint gate (#20)](https://github.com/aryankahar31/runx/pull/20)
- [feat: runx install — npm dependency bootstrap (Phase 1) (#21)](https://github.com/aryankahar31/runx/pull/21)
- [feat: --install flag for runx dev and run keys (Phase 2) (#22)](https://github.com/aryankahar31/runx/pull/22)
- [fix: reject --install on non-run commands (#23)](https://github.com/aryankahar31/runx/pull/23)
- [feat(dep): Python dependency support with venv isolation (Phase 3) (#24)](https://github.com/aryankahar31/runx/pull/24)
- [feat(deps): add Bun and Go dependency bootstrap (Phase 4) (#25)](https://github.com/aryankahar31/runx/pull/25)
- [feat(deps): add Pnpm dependency bootstrap (#26)](https://github.com/aryankahar31/runx/pull/26)
- [feat(deps): complete Yarn dependency bootstrap (#27)](https://github.com/aryankahar31/runx/pull/27)
- [test: add standalone runx install tests for pnpm, Bun, Go (#28)](https://github.com/aryankahar31/runx/pull/28)
- [test: add standalone runx install test for Python (#29)](https://github.com/aryankahar31/runx/pull/29)
- [feat(deps): add Deno dependency management (#30)](https://github.com/aryankahar31/runx/pull/30)
- [feat(deps): support multi-manager dependency installation (#31)](https://github.com/aryankahar31/runx/pull/31)
- [release: v0.6.0 (#33)](https://github.com/aryankahar31/runx/pull/33)

[Full changelog](https://github.com/aryankahar31/runx/compare/v0.5.3...v0.6.0)

## v0.5.3 — 2026-09-04

- [docs: improve README structure and clarity (TOC, scope, opening rewrite) (#10)](https://github.com/aryankahar31/runx/pull/10)
- [fix: proper multi-line formatting for terminal example (#11)](https://github.com/aryankahar31/runx/pull/11)
- [docs: README landing-page redesign + docs/ split (#12)](https://github.com/aryankahar31/runx/pull/12)
- [docs: link project website in hero and documentation index (#13)](https://github.com/aryankahar31/runx/pull/13)
- [docs: link project website in hero and documentation index (#14)](https://github.com/aryankahar31/runx/pull/14)
- [feat: harden runtime integrity and add CI-oriented CLI modes (#15)](https://github.com/aryankahar31/runx/pull/15)
- [Feat/production hardening (#16)](https://github.com/aryankahar31/runx/pull/16)
- [fix(release): drop windows-11-arm from matrix (cosign unsupported) (#17)](https://github.com/aryankahar31/runx/pull/17)
- [chore: bump version to 0.5.3 (#19)](https://github.com/aryankahar31/runx/pull/19)
- [chore: bump version to 0.5.3 (#18)](https://github.com/aryankahar31/runx/pull/18)

[Full changelog](https://github.com/aryankahar31/runx/compare/v0.5.2...v0.5.3)

## v0.5.2 — 2026-08-16

- [fix: preserve npm script passthrough arguments (#8)](https://github.com/aryankahar31/runx/pull/8)
- [chore: bump version to 0.5.2 (#9)](https://github.com/aryankahar31/runx/pull/9)

[Full changelog](https://github.com/aryankahar31/runx/compare/v0.5.1...v0.5.2)

## v0.5.1 — 2026-08-16

- [docs: document jsDelivr mirror for Fastly-blocked networks (#4)](https://github.com/aryankahar31/runx/pull/4)
- [docs: fix Supported Runtimes table broken by mid-table caveat (#5)](https://github.com/aryankahar31/runx/pull/5)
- [fix(run): execute project commands in the invocation directory (#6)](https://github.com/aryankahar31/runx/pull/6)
- [chore: bump version to 0.5.1 (#7)](https://github.com/aryankahar31/runx/pull/7)

[Full changelog](https://github.com/aryankahar31/runx/compare/v0.5.0...v0.5.1)

## v0.5.0 — 2026-08-16

- [feat(detect): multi-runtime detection with lockfile-based Bun support (#3)](https://github.com/aryankahar31/runx/pull/3)

[Full changelog](https://github.com/aryankahar31/runx/compare/v0.4.3...v0.5.0)

## v0.4.3 — 2026-08-14

*(from git)*

- `6b1fede` fix(self-update): probe cosign via the version subcommand
- `37fc3a4` chore: bump version to 0.4.3

[Full changelog](https://github.com/aryankahar31/runx/compare/v0.4.2...v0.4.3)

## v0.4.2 — 2026-08-14

*(from git)*

- `fd7d36d` feat(http): optional GITHUB_TOKEN for authenticated api.github.com requests
- `786f9da` feat(sigstore): keyless release signing and cosign verification
- `3fd50a5` chore: bump version to 0.4.2
- `78a7b7b` fix(release): duplicate jobs key and unused anchor in release.yml

[Full changelog](https://github.com/aryankahar31/runx/compare/v0.4.1...v0.4.2)

## v0.4.1 — 2026-08-14

*(from git)*

- `1faaf02` fix(deno): TLS-only install for Deno < 2.0.1 (no published archive checksum)
- `92b45f6` chore: bump version to 0.4.1

[Full changelog](https://github.com/aryankahar31/runx/compare/v0.4.0...v0.4.1)

## v0.4.0 — 2026-08-14

*(from git)*

- `082bf07` chore: ignore local opencode config
- `3b54ac2` docs: sync README registry-freshness table with green results; drop stale version example
- `ac4b1bf` feat(run): argument passthrough via `--` (behavior change: now valid, was an error)
- `3edb9af` fix(test): platform-aware passthrough quote expectation
- `185b7af` feat(deno): support Deno runtimes
- `59f4031` chore: bump version to 0.4.0

[Full changelog](https://github.com/aryankahar31/runx/compare/v0.3.2...v0.4.0)

## v0.3.2 — 2026-08-12

CI fixes that unblock releases:

- **fix(ci): doctor resolved-PATH test no longer assumes a `bin/` subdir on Windows.** node's Windows layout keeps `node.exe` at the install root, so the expectation is now derived from `runx::runtime::resolve_runtime` — the same source the binary walks. The doctor code itself was correct; the test had hardcoded the Unix layout.
- **fix(bench): registry-freshness.sh now resolves `RUNX_BIN` to an absolute path before `cd`-ing into `benchmarks/`, fixing the relative `target/release/runx` path used by CI.** Includes the genuinely green `benchmarks/registry-results.json` (python now passes).
- **chore: remove orphaned `tests/test_checksum.sh`** (never wired into CI and drifted from the real installer).

Also in this release:

- **fix(python): lazy paged index fetch with retries, and SHA256SUMS checksum fallback** (`bcd0da4`)
- **fix(install): mawk-safe checksum regex, SHELL default, root-level archives, linux-arm64** (`96db868`)
- **feat(doctor): show resolved runtime PATHs; add RUNX_TIMINGS breakdown**
- **docs: prove registry freshness with a weekly verification check**

[Full changelog](https://github.com/aryankahar31/runx/compare/v0.3.1...v0.3.2)

## v0.3.1 — 2026-07-31

*(from git)*

- `1330720` chore: ignore .DS_Store
- `96db868` fix(install): mawk-safe checksum regex, SHELL default, root-level archives, linux-arm64
- `54a6014` chore: bump version to 0.3.1

[Full changelog](https://github.com/aryankahar31/runx/compare/v0.3.0...v0.3.1)

## v0.3.0 — 2026-07-31

- [fix: resolve 8 bugs in v0.2 (#1)](https://github.com/aryankahar31/runx/pull/1) by @yv3000
- [Hardening: P0 fixes, lockfile, cache commands, Bun/Go runtimes, completions, self update (#2)](https://github.com/aryankahar31/runx/pull/2) by @aryankahar31

New contributors: @yv3000 (first contribution in #1), @aryankahar31 (first contribution in #2).

[Full changelog](https://github.com/aryankahar31/runx/compare/v0.2.0...v0.3.0)

## v0.2.0 — 2026-07-05

*(from git)*

- `d26cd64` feat: zero-config auto-detection with UTF-16 file support (v0.2.0)
- `32e521f` style: apply cargo fmt

[Full changelog](https://github.com/aryankahar31/runx/compare/v0.1.1...v0.2.0)

## v0.1.1 — 2026-07-05

*(from git)*

- `c6b5699` Revise README for clarity and additional details
- `38522bf` docs: rewrite README with improved structure and documentation
- `ae00e8b` Retry Python release JSON decode failures
- `33d2ce6` docs: add MIT license
- `44942b5` Update badge links and labels in README.md
- `743ff74` Update license badge in README.md
- `55fa8a8` feat: add SHA-256 checksum verification (v0.1.1)

[Full changelog](https://github.com/aryankahar31/runx/compare/v0.1.0...v0.1.1)

## v0.1.0 — 2026-07-01

*(from git)*

- `c6d4672` Initial runx MVP
- `0c4aace` Update installer repository defaults

[Full changelog](https://github.com/aryankahar31/runx/commits/v0.1.0)
