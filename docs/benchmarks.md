# Benchmarks

## Shell overhead

mise, asdf and nvm hook into your shell. `mise activate` runs a hook on **every prompt render** — every command you type pays its cost, forever, for the life of the session. asdf's shims pay on every command lookup instead. runx does none of that: there is no activation line, no shim directory, no directory hook. **Nothing happens until you type `runx <key>`, and the overhead is paid once per invocation — not per prompt.**

Measured on macOS 27.2 (Apple silicon, zsh 5.9, hyperfine 1.20, runx 0.6.2, mise 2026.7.18, `--warmup 3 --runs 20`; full method in [`benchmarks/README.md`](../benchmarks/README.md)):

| Measurement | Mean |
| :-- | --: |
| zsh startup, no hooks (baseline) | 30.3 ms |
| zsh startup + `mise activate` (per prompt) | 48.9 ms — **+18.6 ms per prompt** |
| `runx <key>` total, warm cache | 11.4 ms |
| — runx pre-child work (`RUNX_TIMINGS=1`): config | 0.07 ms |
| — cache lookup | 0.03 ms |
| — PATH build + spawn | 0.15 ms |

runx's entire contribution before your command starts is **~0.25 ms, once**. `mise activate` adds **18.6 ms to every prompt** on the same machine (mise's own discussion #6279 reports ~80–97 ms first-prompt lag in some configurations — heavier shells only make it worse). A complete `runx <key>` run — process spawn, config read, PATH construction, child start — is **4.3× faster than rendering one mise-activated prompt**, and runx pays nothing on the prompts before and after.

There is a second, related difference: shim-based tools (asdf, and mise's shim mode) have had real bugs in the Windows/WSL boundary, where shim files were misinterpreted as executable scripts under `/mnt/c/...` path semantics. That is a known class of shim-related fragility that runx's architecture avoids **by construction** — runx creates no shims of any kind.

These are directional numbers on one machine, not a universal guarantee; the structural claim — *no shell hook, so no per-prompt cost* — does not depend on any particular measurement.

## Registry freshness verification

The claim that runx resolves against vendor indexes with at most a 6-hour cache window is continuously checked by [`benchmarks/registry-freshness.sh`](../benchmarks/registry-freshness.sh) and a weekly CI workflow ([`registry-freshness.yml`](../.github/workflows/registry-freshness.yml)). Latest results and caveats live in [runtime-resolution.md](runtime-resolution.md#registry-freshness-no-third-party-sync).
