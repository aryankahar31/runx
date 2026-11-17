# Extend `runx doctor --json` with project readiness details

**Labels:** enhancement, CI

## Problem

`runx doctor --json` already emits a cache-health object with `healthy`,
`cache_dir`, `cache_bytes`, `runtimes`, `notes`, and `broken`. It does not yet
include the project readiness checks shown by human-readable `runx doctor`,
such as resolved project runtimes, dependency checks, and lockfile status. CI
and editor integrations otherwise have to combine JSON from separate commands
or parse human output.

## Proposal

Extend the existing JSON object with project-readiness fields while keeping
the existing cache-health fields and human output. Document which status values
are informational, warnings, or failures. Preserve the current exit-code
contract unless a `--strict` flag is explicitly added.

## Acceptance

- Document the complete schema and field/status meanings in `docs/`.
- Snapshot-style tests for a healthy project and a missing runtime/dependency.
- Keep path disclosure limited to paths already printed by human doctor output.
