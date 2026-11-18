# Why runx has no shell integration (and what it does instead)

Most version managers work by changing your shell. nvm and pyenv ask you to
`eval` an activation script; asdf and mise put shims or hooks in the prompt
path; Volta installs shims. That design is convenient for ambient switching,
and it means every interactive command pays the manager's cost forever.

runx takes the other branch: **run one command, with one project's runtimes, and
leave the shell alone.**

The crates.io package is named `runx-launcher`, while its command remains
`runx`; after publication, install with `cargo install runx-launcher`.

## The mechanism

1. Resolve the project's requirements from `runx.toml`, a lockfile, or the
   version files the project already has.
2. Ensure each exact runtime exists in `~/.runx/runtimes/<tool>/<version>`,
   verifying the archive checksum before extraction.
3. Build a `PATH` containing those runtime `bin` directories and the project's
   venv, then append the system `PATH` so `git`, `make`, and `curl` still work.
4. Spawn the command with that environment.

The parent process never changes. There is no activation line to forget, no
"which version am I in" ambiguity between directories, and no per-prompt cost.

The trade-off is real: `runx dev` is not an ambient `node` that follows you
between directories. If you want that, mise or asdf is the right tool. runx is
for projects that should be runnable from a fresh clone by someone who has only
the runx binary.

## Why the distinction matters

Ambient switching is a shell feature. Project execution is a process feature.
Conflating them is why "works on my machine" survives: the shell state is part
of the setup, and the shell state is invisible to CI, editors, and teammates.

runx makes the environment explicit at the process boundary. That also makes it
testable: an integration test reads the parent's `PATH` before and after a run
and asserts it is byte-identical.

## Verification, not vibes

`runx lock` records the resolved version and a per-platform SHA-256 for the
archive. With `--locked`, runx authenticates the retained archive, re-extracts,
and compares the installed tree before running. Losing the archive or the digest
is a failure, not a fallback.

None of this sandboxes the child or defends against a malicious lockfile — a
lockfile you control is a trust input. The [security model](https://github.com/aryankahar31/runx/blob/main/docs/security.md)
says so explicitly.

## What is still missing

Java is Temurin 9+ only. Ruby, PHP, and .NET are not in v0.7.0 because their
portable archives and checksum sources do not meet the same bar; shipping them
with weaker verification would undermine the point. The Dockerfile is a recipe,
not a published image.

If "the runtime is part of the command, not the shell" sounds like your kind of
trade-off, the repo is <https://github.com/aryankahar31/runx>.
