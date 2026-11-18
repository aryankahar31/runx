# setup-runx

A composite GitHub Action that installs `runx` (SHA-256-verified release binary),
restores the runtime cache, and can enforce `runx.lock`.

```yaml
- uses: actions/checkout@v4

- uses: aryankahar31/runx/setup-runx@main
  with:
    version: 0.7.0   # or "latest" (default)
    locked: true     # sets RUNX_LOCKED=1
    cache: true      # caches ~/.runx/runtimes

- run: runx dev
```

The cache key includes `hashFiles('**/runx.lock')`, so a lockfile change gets a
fresh cache while unrelated commits reuse it. The runtime cache is a global
`~/.runx/runtimes`; runx reads it in place and never needs a shell hook.

`locked: true` does not create or repair a lockfile. It makes a missing or stale
lock a hard error before your command runs, which is what CI wants. Generate the
lock locally with `runx lock` and commit it.

Example workflow: [`examples/ci.yml`](examples/ci.yml).

## Inputs

| Input | Default | Effect |
| --- | --- | --- |
| `version` | `latest` | Exact runx release (`0.7.0` or `v0.7.0`) or `latest`. |
| `locked` | `false` | Exports `RUNX_LOCKED=1`. |
| `cache` | `true` | Caches `~/.runx/runtimes` with `actions/cache`. |
