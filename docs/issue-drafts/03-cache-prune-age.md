# Accept duration suffixes in `runx cache prune --older-than`

**Labels:** enhancement, UX, good first issue

## Problem

`runx cache prune --older-than N` already prunes cached runtimes whose best
available activity timestamp is at least `N` days old. It prefers the
runx-maintained last-used marker and falls back to the install timestamp; an
install with neither timestamp is left alone. The threshold currently accepts
whole days only, so values such as `90d` are rejected.

## Proposal

```sh
runx cache prune --older-than 90
runx cache prune --older-than 90 --dry-run
runx cache prune --older-than 90d
runx cache prune --older-than 90d --dry-run
```

Accept readable duration suffixes such as `d` (days), while preserving the
existing integer form. Pruning currently applies to every cached runtime that
meets the threshold, including one selected by the current project; it defaults
to a dry run and requires `--yes` to delete.

## Open question

Should the parser support only days or also weeks/months? Keep the existing
runx-maintained last-used marker and install-date fallback as the age source.

## Acceptance

- Dry run prints every candidate and deletes nothing (test).
- The integer form remains accepted, and supported suffixes map to the intended
  number of days (test).
- Docs state the timestamp source and its limitations.
