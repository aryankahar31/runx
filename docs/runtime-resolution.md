# Runtime Resolution

How runx turns a version *requirement* into a concrete runtime version.

## Semver range resolution

When a version file contains a range rather than an exact version, runx resolves it to the **newest published release that satisfies the constraint** — the same behavior as nvm, Volta and mise.

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

**Exact pins never touch the network.** Resolving a range needs the published release list, which is cached for 6 hours; if it cannot be fetched, runx falls back to the lowest satisfying version and says so, so offline machines keep working.

### Strict mode

Set `RUNX_RESOLUTION=minimum` to resolve ranges to the *lowest* satisfying version instead. This is fully offline and time-independent, but note that `>=20` then means "the oldest Node 20 ever published", which carries known CVEs. Exact pins are unaffected by this setting.

For reproducibility across machines and CI, prefer [`runx lock`](lockfile.md) over strict mode: it pins the exact version that was resolved, rather than re-deriving one.

## Registry freshness: no third-party sync

runx resolves version ranges **directly against each vendor's own release index** — `nodejs.org/dist/index.json` for Node, the `astral-sh/python-build-standalone` GitHub releases for Python, GitHub releases for Bun, and `go.dev/dl` for Go. Results are cached locally for at most 6 hours (`INDEX_TTL_SECS` in `src/registry.rs`); after that, the next range resolution refetches from the vendor. There is no sync service, mirror, or intermediate registry in the path.

That is the structural difference from mise's version registry, which depends on a third-party sync pipeline: per [mise's own discussion #7468](https://github.com/jdx/mise/discussions/7468), its index refreshes roughly every 15 minutes but is rate-limited, so `mise latest <tool>` can report a version that is **days behind** the actual latest upstream release. runx's worst case is its own 6-hour cache window — bounded and self-healing, not unbounded by someone else's rate limit.

**Falsifiable and continuously checked:** `benchmarks/registry-freshness.sh` fetches the latest version from each vendor's canonical source with plain `curl`, then has runx resolve a fresh `*` range in an isolated cache, and compares the two. A weekly CI run ([`registry-freshness` workflow](../.github/workflows/registry-freshness.yml)) keeps the claim verified over time:
![registry-freshness status](https://github.com/aryankahar31/runx/actions/workflows/registry-freshness.yml/badge.svg)

Latest verified run (`benchmarks/registry-results.json`):

| Runtime | Vendor latest | runx resolved | Status |
| :-- | :-- | :-- | :--: |
| node | 26.7.0 | 26.7.0 | ✅ |
| go | 1.26.5 | 1.26.5 | ✅ |
| bun | 1.3.14 | 1.3.14 | ✅ |
| python | 3.14.7 | 3.14.7 | ✅ |

Honest caveats: the python index is heavy (10 pages of python-build-standalone releases, ~15 MB each), so the weekly CI run exercises it on GitHub's own network, which is the environment runx users in CI hit — it passes there even when it is too slow for a laptop. The 6-hour cache means a release published within the last 6 hours may not yet appear in runx's resolution; rerun after the window for a clean check. The unauthenticated GitHub API (60 requests/hour/IP) is the practical ceiling for the python and bun lookups; a weekly run uses a fraction of it.
