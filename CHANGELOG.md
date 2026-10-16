# Changelog

All releases from v0.1.0 to v0.6.0, reconstructed from the repository's git
tags and each tag's GitHub release notes (`gh release view`). Entries marked
*(from git)* were reconstructed from the commit range between tags because
that release has no notes; every line is verifiable against the linked
compare view or commit.

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
