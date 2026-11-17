# Containers

There is no published container image yet. Build one from the repository:

```bash
docker build -t runx .
```

Run a project with its declared runtimes:

```bash
docker run --rm \
  -v "$PWD:/work" -w /work \
  -v runx-cache:/runx \
  runx dev
```

The named volume keeps `~/.runx` (here `/runx`) across containers, so runtimes
are downloaded once. Without the volume the cache is discarded with the
container.

## Notes

- The container needs outbound network on first use to download a runtime.
  After the cache is populated, `--offline` works if the runtime's asset
  metadata is cached too; see [security.md](security.md#locked-execution).
- runx does not run as root here by default. If your image sets `USER`, make
  sure `/runx` is writable by that user.
- `--frozen` inside a container is usually a mistake unless `/runx` was
  pre-populated on the host and mounted read-only: it refuses network access.
- The image is `debian:bookworm-slim` plus CA certificates. If your project
  needs a C toolchain (native npm modules, Python extensions), extend the
  final stage with `build-essential` — runx deliberately does not install
  compilers into its cache.
