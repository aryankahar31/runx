# Show HN: Runx – Run any project with exact runtime versions

Hi HN,

Every project I picked up assumed a different ritual: install nvm, source it,
install pyenv, hope the system Python is not the one that breaks, and never
quite trust that the versions I got are the ones the project asked for.

runx is a single binary that reads the version files a project already has
(`.nvmrc`, `package.json` engines, `pyproject.toml`, `go.mod`, `.java-version`)
or a small `runx.toml`, downloads those runtimes into `~/.runx`, verifies the
archives, and runs your command with only that runtime on the child's `PATH`.

Six runtimes: Node, Python, Bun, Go, Deno, Java (Temurin 9+). No shell
activation, no shims, no sudo, no telemetry. `runx lock` pins versions and
per-platform digests for CI, and `--locked` makes drift fail.

Limits: Java is Temurin 9+ only; Ruby, PHP, and .NET were skipped because their
portable archives do not meet the same checksum bar; the Dockerfile is a recipe,
not a published image.

Rust, MIT, Linux/macOS/Windows. Install with the curl or PowerShell script,
`cargo install runx-launcher`, `npm install -g runx-launcher`, or
`brew install aryankahar31/tap/runx-launcher`.

https://github.com/aryankahar31/runx

Feedback welcome, especially from locked-down work machines and people who can
break the "no shell integration" promise.

## First comment (post after submitting)

Benchmarks and method are in the repo, including the shell-overhead comparison
with mise (`benchmarks/shell-overhead.sh`). They are one-machine numbers with a
warm cache, not a universal guarantee; the structural claim is that runx pays
its cost once per invocation rather than on every prompt.
