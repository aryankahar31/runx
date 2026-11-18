# runx (npm wrapper)

```bash
npx runx dev
```

This package downloads the prebuilt `runx` binary for your platform from the
matching GitHub release, verifies it against the release's `SHA256SUMS`, caches
it under `~/.runx/npm/<version>/`, and runs it. Unsupported platforms
(`freebsd`, Windows ARM64) get a clear error instead of a binary that does not
start.

Set `RUNX_VERSION=0.7.0` to pin a different release. It must use numeric
`MAJOR.MINOR.PATCH` form. Set `RUNX_HOME` to move the cache.

**Planned / pending submission:** this wrapper is not published yet. The
release process renders its version from the matching `runx` release before
the manual `npm publish` step.
