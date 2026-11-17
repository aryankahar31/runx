# Security policy

## Reporting a vulnerability

Please do **not** open a public issue for a security problem.

Use GitHub's private vulnerability reporting:
<https://github.com/aryankahar31/runx/security/advisories/new>

Include the affected version (`runx --version`), platform, reproduction steps,
and the impact you believe it has. You can expect an initial reply within seven
days. We will credit reporters in the advisory unless you ask otherwise.

## Supported versions

Security fixes are released for the latest minor version. Older releases may be
asked to upgrade before a fix is backported.

## Scope

In scope:

- the `runx` binary, install scripts, and release artifacts;
- version resolution, download, checksum/signature verification, extraction,
  cache handling, and child-process `PATH` construction;
- lockfile enforcement (`--locked` / `--frozen`).

Out of scope:

- vulnerabilities in the runtimes or packages runx downloads (report them to
  the upstream project);
- a project's own command or dependencies;
- a lockfile the attacker can already modify, or another local process racing
  runx's cache (the documented [security model](docs/security.md) covers both).

## What runx verifies

- Runtime archives against published SHA-256 checksums where the vendor
  publishes them (Node, Python, Bun, Go, Deno 2.0.1+, Java via Adoptium).
- The installed tree and retained archive against `runx.lock` under `--locked`.
- runx's own release archives against Sigstore/cosign signatures when cosign is
  available.

Known limitations, including older Deno releases and the Python/Go metadata
requirement offline, are documented in [docs/security.md](docs/security.md)
rather than left implicit.
