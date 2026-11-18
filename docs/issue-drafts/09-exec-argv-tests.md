# Broaden `runx exec` argument-forwarding tests

**Labels:** test, good first issue

## Problem

`runx exec` deliberately skips a shell and forwards argv. Its existing
integration test checks that a command from the runtime cache receives a
regular argument and a literal `$(whoami)` without shell expansion. Empty
arguments, spaces within one argument, leading-dash arguments, and child exit
code propagation are not covered by that test.

## Proposal

Add probe-based tests (the existing `plant_executable` pattern) for:

- an argument containing spaces;
- an empty argument;
- a leading-dash argument after the command;
- exit-code propagation from the child.

## Acceptance

- No test touches the network.
- Failures show the actual argv the child received, not just an exit code.
