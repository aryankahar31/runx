# X / Mastodon thread

1/ I shipped runx v0.7.1: run any project with the runtimes it declares, without
sudo, global installs, or shell hooks.

2/ You clone a repo. It has `.nvmrc`, `pyproject.toml`, or `runx.toml`. You run:

    runx dev

runx downloads the exact runtimes into `~/.runx`, verifies the archives, and runs
your command with only those runtimes on the child's PATH. Your shell is
untouched.

3/ No `eval "$(tool activate)"`. No shims. No prompt cost. The parent shell is
unchanged after the run — there is a test that reads PATH from a probe before
and after.

4/ For CI: `runx lock` pins versions and per-platform digests. `--locked` /
`--frozen` and `RUNX_LOCKED=1` make a mismatch fail instead of silently
resolving something new.

5/ The crates.io package is `runx-launcher`; the installed command and Rust
library crate remain `runx`. After publication, install with
`cargo install runx-launcher`. v0.7.1 carries forward Temurin Java 9+,
daily-use commands, uv and
Poetry lockfiles, package-manager pins, and the setup-runx Action.

6/ Limits, stated up front: Java 9+ only. Ruby/PHP/.NET were checked and skipped
because their portable archive checksums do not meet the bar. No Docker image is
published yet. Offline Python/Go may need cached metadata. Homebrew, Winget,
Scoop, AUR, Nix, and npm submissions remain pending.

7/ Rust, MIT, Linux/macOS/Windows x64. Feedback welcome, especially from
locked-down work machines and people who want to break the no-shell-hook claim.

https://github.com/aryankahar31/runx
