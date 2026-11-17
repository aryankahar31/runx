# Verify `--locked` on Windows with a non-ASCII path

**Labels:** test, windows

## Problem

Locked execution normalizes paths and compares extracted trees. Windows path
semantics (drive letters, separators, Unicode) are the most likely place for a
false mismatch, but the locked tests run on Unix-style temp paths.

## Proposal

Add a Windows-only integration test under a directory containing a space and a
non-ASCII character. Plant a runtime, write a matching lockfile, run
`runx run --locked` with a probe, and assert success. Add a mutation case that
must fail.

## Acceptance

- Passes on the Windows CI runner.
- Skips cleanly on Unix or runs there too with an equivalent path.
- No changes to production verification logic unless the test finds a real bug.
