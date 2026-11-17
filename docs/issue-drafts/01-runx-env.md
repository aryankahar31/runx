# Add `runx env` to print the resolved environment

**Labels:** enhancement, good first issue

## Problem

When a child process sees the wrong Node, or a shell tool cannot find the
runtime runx uses, there is no single command that prints the full environment
runx would pass to a child. `runx which <tool>` prints the selected executable
for one runtime, and `runx doctor` reports resolved project paths and readiness.
`RUNX_TIMINGS=1` reports phase timings, not paths or environment variables.
Inspecting the complete environment runx would inject is useful for debugging
and CI.

## Proposal

```sh
runx env            # shell-style exports? no — plain listing
runx env --json
```

Print the detected runtimes, their cache paths, the `PATH` entries runx would
prepend (runtime first, then project venv), and the active `RUNX_HOME`. Do not
start the command and do not install anything.

## Acceptance

- Works with no `runx.toml` (detection path) and with one.
- `--json` is stable and documented.
- An integration test plants a fake cached runtime and asserts the paths.
- No shell syntax is emitted; users can compose it themselves.
