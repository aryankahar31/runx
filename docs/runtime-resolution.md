# Runtime Resolution

How runx turns a version *requirement* into a concrete runtime version.

## Semver range resolution

When a version file contains a range rather than an exact version, runx resolves it to the **newest published release that satisfies the constraint**.

| Input | Resolves to |
|-------|-------------|
| `>=20` | newest release ≥ 20 |
| `^20` | newest `20.x` |
| `~20.11` | newest `20.11.x` |
| `<20` | newest release below 20 |
| `20` | newest `20.x` (a bare partial version is an X-range) |
| `18 \|\| >=20` | newest release matching either branch |
| `>=3.11` | newest release ≥ 3.11 |
| `~=3.11` | newest `3.x` ≥ 3.11 (PEP 440) |
| `20.11.0` | `20.11.0` (exact pin, never changed) |

Runx always prints which concrete version a range resolved to, so there are no silent surprises.

Resolving an exact pin does not need a release-list lookup. Installing a missing
runtime can still need an archive, checksum, or runtime-specific asset metadata.
Resolving a range normally fetches the vendor release list, cached for 6 hours;
if that lookup fails, runx falls back to the lowest satisfying version and
says so. Explicit `--offline` refuses a range that needs an uncached release
list rather than silently making that fallback.

### Strict mode

Set `RUNX_RESOLUTION=minimum` to resolve ranges to the *lowest* satisfying version instead. This is fully offline and time-independent, but note that `>=20` then means "the oldest Node 20 ever published", which carries known CVEs. Exact pins are unaffected by this setting.

For reproducibility across machines and CI, prefer [`runx lock`](lockfile.md) over strict mode: it pins the exact version that was resolved, rather than re-deriving one.

## Registry freshness: no third-party sync

runx resolves version ranges against vendor release sources: `nodejs.org/dist/index.json` for Node, the `astral-sh/python-build-standalone` GitHub releases for Python, GitHub releases for Bun and Deno, `go.dev/dl` for Go, and Adoptium metadata for Java. Registry results use a local 6-hour cache (`INDEX_TTL_SECS` in `src/registry.rs`). There is no sync service or intermediate registry in the path.

This differs from tools that use a third-party version registry: their freshness
depends on that registry's sync and rate-limit behavior. runx's own release-list
cache has a defined 6-hour lifetime.

**Falsifiable and continuously checked:** `benchmarks/registry-freshness.sh` fetches the latest version from each vendor's canonical source with plain `curl`, then has runx resolve a fresh `*` range in an isolated cache, and compares the two. A weekly CI run ([`registry-freshness` workflow](../.github/workflows/registry-freshness.yml)) keeps the claim verified over time:
![registry-freshness status](https://github.com/aryankahar31/runx/actions/workflows/registry-freshness.yml/badge.svg)

The freshness workflow currently probes Node, Go, Bun, and Python. It does not
cover Java or Deno. Last checked-in sample (`benchmarks/registry-results.json`)
is a recorded snapshot, not a live claim about today's latest releases:

| Runtime | Vendor latest | runx resolved | Status |
| :-- | :-- | :-- | :--: |
| node | 26.7.0 | 26.7.0 | ✅ |
| go | 1.26.5 | 1.26.5 | ✅ |
| bun | 1.3.14 | 1.3.14 | ✅ |
| python | 3.14.7 | 3.14.7 | ✅ |

Honest caveats: the python index is heavy (10 pages of python-build-standalone releases, ~15 MB each), so the weekly CI run exercises it on GitHub's own network, which is the environment runx users in CI hit — it passes there even when it is too slow for a laptop. The 6-hour cache means a release published within the last 6 hours may not yet appear in runx's resolution; rerun after the window for a clean check. The unauthenticated GitHub API (60 requests/hour/IP) is the practical ceiling for the python and bun lookups; a weekly run uses a fraction of it.
