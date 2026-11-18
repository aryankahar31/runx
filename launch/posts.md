# Launch posts

Facts current as of v0.7.1: six runtimes (Node.js, Python, Bun, Go, Deno, Java
Temurin 9+); installable via install scripts, cargo, npm, Homebrew, and Scoop;
Winget PR open, AUR blocked on Arch's registration freeze, Nix unfinished.

## Show HN

**Title:** Show HN: Runx – Run any project with exact runtime versions

**Body:**

Every repo says "use Node 20" in a README while the machine has Node 22.
runx closes that gap without installing anything globally.

`runx dev` reads what the project already declares — `.nvmrc`, `package.json`
engines, `pyproject.toml`, `go.mod`, `.java-version` — downloads those exact
portable runtimes into `~/.runx`, verifies their SHA-256 before extraction, and
runs your command with a child-only `PATH`. Your shell is untouched: no `eval`
line, no shims, no prompt hook.

- Six runtimes: Node, Python, Bun, Go, Deno, Java (Temurin).
- `runx lock` pins versions plus per-platform digests; `--locked` enforces them
  in CI.
- Release archives are Sigstore/cosign-signed; no telemetry.
- Written in Rust, MIT, Linux/macOS/Windows.

Install with the curl or PowerShell script, `cargo install runx-launcher`,
`npm install -g runx-launcher`, or
`brew install aryankahar31/tap/runx-launcher`.

I built it because I kept juggling nvm, pyenv, and Go toolchains on locked-down
machines. Feedback welcome — especially where runx resolves the wrong version,
or where giving up ambient switching does not fit how you work.

https://github.com/aryankahar31/runx

## X / Mastodon thread

1/ I shipped runx v0.7.1: run any project with the exact runtimes it declares,
without sudo, global installs, or shell hooks. One static Rust binary for Node,
Python, Bun, Go, Deno, and Java. MIT.

https://github.com/aryankahar31/runx

2/ One command does the whole chain:

    runx dev

Reads `.nvmrc` / `package.json` engines / `pyproject.toml` / `go.mod`,
downloads those exact portable runtimes, checks SHA-256, builds a child-only
PATH, then runs your command. No eval, no shims, no prompt cost.

3/ Security is visible, not implied: runtime archives are SHA-256 checked before
extraction, release archives and SHA256SUMS are Sigstore/cosign-signed, `self
update` verifies the same way, and `RUNX_REQUIRE_SIGNATURE=1` fails closed. No
telemetry.

4/ Install: curl/PowerShell script, `cargo install runx-launcher`,
`npm install -g runx-launcher`, `brew install aryankahar31/tap/runx-launcher`,
or the Scoop bucket. Feedback welcome, especially from locked-down work
machines.

https://github.com/aryankahar31/runx
