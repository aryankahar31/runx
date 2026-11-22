# Warn in `runx doctor` when multiple `runx` binaries are on `PATH`

**Labels:** enhancement, UX, good first issue

## Problem

When a user installs `runx` through more than one channel (for example, the
install script at `~/.runx/bin/runx`, `cargo install` at `~/.cargo/bin/runx`, or
Homebrew at `/opt/homebrew/bin/runx`), an older binary earlier on `PATH` can
shadow the newly installed one. Running `runx --version` or `runx self update`
then operates on whichever binary comes first on `PATH`, without making the
shadowing obvious.

## Proposal

In `runx doctor`, scan `PATH` entries for `runx` executables (deduplicating
identical or canonicalized paths that resolve to the same file or duplicate
`PATH` segments):

- When more than one distinct `runx` executable is found on `PATH`, emit a
  non-fatal warning.
- List all discovered `runx` binary paths in `PATH` order, clearly indicating
  which one wins (the first match on `PATH`) and which ones are shadowed.
- Suggest removing the duplicate binary so upgrades and CLI invocations stay
  consistent.

## Acceptance

- `runx doctor` warns when two or more distinct `runx` binaries appear on
  `PATH`, shows every discovered path, and marks which one wins.
- Duplicate entries of the same directory on `PATH` (or symlinks resolving to
  the same binary where appropriate) do not trigger a false warning.
- Unit / integration tests cover both the single-binary (no warning) and
  duplicate-binary (warning with winning + shadowed paths) cases without
  touching the network.
