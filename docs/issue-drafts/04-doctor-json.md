# Emit machine-readable `runx doctor --json`

**Labels:** enhancement, CI

## Problem

CI and editor extensions currently parse doctor's human output. It is stable in
practice but not specified, and the exit code only reflects cache health.

## Proposal

`runx doctor --json` returns one object: overall status, active `RUNX_HOME`,
cache size, each runtime with resolved/missing state, dependency checks, and
lockfile status. Keep human output unchanged and keep the existing exit-code
contract unless a `--strict` flag is explicitly added.

## Acceptance

- Documented schema (field names and enum values) in `docs/`.
- Snapshot-style test for a healthy project and one missing runtime.
- No secrets or absolute home paths beyond what doctor already prints.
