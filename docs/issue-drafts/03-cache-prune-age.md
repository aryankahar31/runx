# Add `runx cache prune --older-than`

**Labels:** enhancement, UX, good first issue

## Problem

`runx cache prune` only removes versions not referenced by the current project's
lockfile/config. A shared machine accumulates runtimes for old projects with no
way to age them out.

## Proposal

```sh
runx cache prune --older-than 90d
runx cache prune --older-than 90d --dry-run
```

Consider a runtime unused if its last access time is older than the threshold.
Keep the existing safety behavior: never delete the runtime the current project
resolves, and always support `--dry-run`.

## Open question

Should "unused" mean atime, or a runx-maintained last-used marker? atime is
unreliable on some filesystems and disabled on many mounts; a small marker file
updated on use may be the honest choice.

## Acceptance

- Dry run prints every candidate and deletes nothing (test).
- A runtime in use is never a candidate (test).
- Docs state the timestamp source and its limitations.
