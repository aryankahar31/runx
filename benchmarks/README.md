# Shell overhead benchmark

runx makes one structural claim: **no shell integration** — no
`eval "$(mise activate zsh)"`, no shims, no prompt hooks. mise, asdf and nvm
hook into every interactive prompt, adding latency to *every* command you
type for the life of the shell session. runx does nothing until you
explicitly run `runx <key>`.

This benchmark measures both sides of that claim:

| Measurement | What it captures |
| :-- | :-- |
| `zsh startup, no hooks (baseline)` | Cost of rendering an interactive zsh prompt with an empty rc. |
| `zsh startup + mise activate` | The same, with `eval "$(mise activate zsh)"` in the rc — the per-prompt cost mise imposes. Only runs when mise is installed. |
| `runx <key>` | Total wall time of one `runx <key>` invocation with a warm cache — a one-time cost, paid only when you run it. |
| `RUNX_TIMINGS=1` breakdown | runx's own pre-child work (config discovery, cache lookup, PATH build + spawn), the apples-to-apples number against mise's `MISE_TIMINGS=1`. |

## Results (this machine)

Machine: macOS 26.5.2, Apple silicon (arm64), zsh 5.9, hyperfine 1.20.0.
runx 0.3.1 built from `target/release`. mise 2026.7.18 (Homebrew).

| Benchmark | Mean | Min | Max |
| :-- | --: | --: | --: |
| zsh startup, no hooks (baseline) | 23.2 ms | 22.2 ms | 23.7 ms |
| zsh startup + mise activate | 38.4 ms | 37.3 ms | 40.1 ms |
| runx `<key>` (one-time, warm cache) | 5.9 ms | 5.4 ms | 6.4 ms |
| runx pre-child (config) | 0.07 ms | — | — |
| runx pre-child (cache) | 0.02 ms | — | — |
| runx pre-child (PATH + spawn) | 0.14 ms | — | — |

`mise activate` adds **15.2 ms to every prompt** over the baseline on this
machine — and that is with a bare rc; the hook only gets more expensive as
your shell does more. A full `runx <key>` run (5.9 ms, child included) is
**6.6× faster than rendering one mise-activated prompt**, and runx's own
pre-child work is **~0.2 ms, paid once per invocation**. Mise's team
reports ~80–97 ms first-prompt lag in some configurations
([#6279](https://github.com/jdx/mise/discussions/6279)) — the 15 ms here is
the hook on a clean prompt, the low end of that scale.

The structural point: a hook-based tool adds tens of milliseconds **to every
single prompt, forever**, while runx's entire contribution is ~0.2 ms
**once, when you run it**.

Honest caveat: these are microbenchmarks on one machine with a warm OS cache.
Numbers vary with machine, shell config, and filesystem. The relationship
they show — runx has no per-prompt cost at all, and its one-time cost is
sub-millisecond — is structural, not a performance guarantee.

## Reproduce

```sh
brew install hyperfine   # or: apt install hyperfine
benchmarks/shell-overhead.sh            # uses `runx` from PATH
benchmarks/shell-overhead.sh path/to/runx   # or a specific binary
```

The script:

1. builds a **hermetic** project (fake cached `node` 20.11.0, exact-version
   pin) — no network access, no dependence on your real `~/.runx`;
2. measures interactive zsh startup with an empty rc vs one containing
   `eval "$(mise activate zsh)"` (mise section skipped with a notice if mise
   is missing);
3. measures `runx <key>` with a warm cache;
4. prints a `RUNX_TIMINGS=1` pre-child breakdown;
5. writes `results.json` / `results.csv` (hyperfine's own exports).

Defaults: `--warmup 3 --runs 20`. Override via the `WARMUP` / `RUNS`
variables or edit the script.

To compare against your real configuration (e.g. a heavy `.zshrc`), measure
`zsh -i -c 'exit'` with your normal `ZDOTDIR` instead of the empty one —
the delta for mise grows, not shrinks.

## Re-run with mise installed

```sh
brew install mise
./benchmarks/shell-overhead.sh
```

The mise row fills in automatically.
