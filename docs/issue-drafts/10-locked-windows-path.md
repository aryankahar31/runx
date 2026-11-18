# Verify `--locked` on Windows with a non-ASCII path

**Labels:** test, windows

## Problem

Locked execution normalizes paths and compares extracted trees. The integration
suite runs on Windows CI, but its locked tests do not deliberately place the
project/cache beneath a path containing spaces and non-ASCII characters. That
leaves a useful Windows path case uncovered.

## Proposal

Add a Windows-only integration test under a directory containing a space and a
non-ASCII character. Plant a runtime, write a matching lockfile, run
`runx run check --locked` with a probe, and assert success. Add a mutation case
that must fail.

## Acceptance

- Passes on the Windows CI runner.
- Skips cleanly on Unix or runs there too with an equivalent path.
- No changes to production verification logic unless the test finds a real bug.
