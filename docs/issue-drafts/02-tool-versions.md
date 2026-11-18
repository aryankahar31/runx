# Detect `.tool-versions` (asdf/mise compatibility)

**Labels:** enhancement, detection

## Problem

Teams migrating from asdf or mise often have a `.tool-versions` file and no
per-language version files. runx cannot use it, so the first run either asks for
a `runx.toml` or resolves a different version than the team expects.

## Proposal

Read `.tool-versions` as a lowest-priority detection source, after ecosystem
files and before falling back to a range. Support the subset runx can run:
`nodejs`, `python`, `golang`, `java`, `bun`, `deno`. Ignore comments and
unknown tools.

## Non-goals

- asdf plugins or `.asdfrc` behavior.
- Global/mise config files: project-local only.

## Acceptance

- One unit test per supported tool, plus a test that an ecosystem-specific file
  (`.nvmrc`) still wins.
- Documented in `docs/zero-config.md` with the precedence table updated.
