# Show HN: Runx – run any project with its own runtimes, no sudo or shell hooks

Hi HN,

I built runx because every machine I touched had a different ritual for running
someone else's project: install nvm, source it, install pyenv, hope the system
Python is not the one that breaks, and never quite trust that the versions I got
are the ones the project asked for.

runx is a single binary that reads the version files a project already has
(`.nvmrc`, `.python-version`, `package.json` engines, `pyproject.toml`, `go.mod`,
`deno.lock`, `.java-version`) or a small `runx.toml`, downloads those runtimes
into `~/.runx`, verifies the archives, and runs your command with only that
runtime on the child's `PATH`.

What it deliberately does not do:

- no shell activation, prompt hook, or shims;
- no global runtime versions;
- no sudo or system package manager;
- no telemetry.

`runx dev` is the whole interface on a detected project. `runx lock` pins exact
versions and archive digests for CI, and `runx doctor` explains what is missing
before you hit a confusing stack trace.

The honest limits: Java is Temurin 9+ only; older Deno releases have no vendor
checksum and say so; offline Python/Go provisioning may need cached metadata;
and the Dockerfile is a recipe, not a published image yet. Ruby, PHP, and .NET
were evaluated for v0.7.0 and skipped because their portable archive/checksum
story does not meet the same bar — the roadmap explains why.

It is Rust, MIT, and works on Linux, macOS, and Windows x64.

Repo: https://github.com/aryankahar31/runx
Docs: https://github.com/aryankahar31/runx#readme

I would especially value feedback from people who run projects on locked-down
work machines or shared CI runners, and from anyone who can break the "no shell
integration" promise.

## First comment (post after submitting)

Benchmarks and method are in the repo, including the shell-overhead comparison
with mise (`benchmarks/shell-overhead.sh`). They are one-machine numbers with a
warm cache, not a universal guarantee; the structural claim is that runx pays
its cost once per invocation rather than on every prompt.
