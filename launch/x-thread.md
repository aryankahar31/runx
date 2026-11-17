# X / Mastodon thread

1/ I shipped runx v0.7.0: run any project with the runtimes it declares, without
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

5/ v0.7.0 adds Temurin Java 9+, `runx list/which/exec/upgrade`, uv and Poetry
lockfiles, `packageManager` pins for pnpm/Yarn, install channels (Cargo,
Homebrew, Winget, Scoop, AUR, Nix, npm), a setup-runx Action, and a zero-admin
guide.

6/ Limits, stated up front: Java 9+ only. Ruby/PHP/.NET were checked and skipped
because their portable archive checksums do not meet the bar. No Docker image is
published yet. Offline Python/Go may need cached metadata.

7/ Rust, MIT, Linux/macOS/Windows x64. Feedback welcome, especially from
locked-down work machines and people who want to break the no-shell-hook claim.

https://github.com/aryankahar31/runx
