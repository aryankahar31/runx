# Broaden `runx exec` argument-forwarding tests

**Labels:** test, good first issue

## Problem

`runx exec` deliberately skips a shell and forwards argv. One integration test
covers a simple argument list; quoting, empty arguments, and a leading dash are
exactly where shell-free execution wins, and they are untested.

## Proposal

Add probe-based tests (the existing `plant_executable` pattern) for:

- an argument containing spaces;
- an empty argument;
- `--` before the command;
- a command name that exists only in the runtime cache;
- exit-code propagation from the child.

## Acceptance

- No test touches the network.
- Failures show the actual argv the child received, not just an exit code.
