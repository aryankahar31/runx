# I made a runtime launcher that never touches your shell

**Subreddits:** r/programming, r/rust, r/devops

Every project I pick up assumes I have already installed its exact Node, Python,
or Java, globally, and that I enjoy debugging which one won. runx replaces that
with one command:

```sh
runx dev
```

It reads the version files the project already has (`.nvmrc`,
`.python-version`, `pyproject.toml`, `package.json` engines, `go.mod`,
`.java-version`, ...) or a `runx.toml`, puts the resolved runtimes in
`~/.runx`, checks the download against the vendor's checksum, and runs the
command with only that runtime on the child's `PATH`.

No shell activation. No shims. No sudo. No global versions. The parent shell is
unchanged; a probe test asserts that.

For teams, `runx lock` writes exact versions plus per-platform SHA-256 digests,
and `--locked` / `--frozen` make CI fail rather than drift.

Caveats, because a launch post without them is an ad: Java is Temurin 9+; Ruby,
PHP, and .NET were intentionally not shipped in this release because their
portable archives and checksums do not meet the same verification bar; there is
no Docker image published yet, only a documented recipe.

MIT, Rust, Linux/macOS/Windows x64:
https://github.com/aryankahar31/runx

Happy to answer questions about the isolation model, lockfile verification, or
why not just use mise/asdf.
