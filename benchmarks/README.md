# Shell overhead benchmark

runx makes one structural claim: **no shell integration** — no
`eval "$(mise activate zsh)"`, no shims, no prompt hooks. runx does nothing
until you explicitly run `runx <key>`. This benchmark measures interactive
shell startup with and without a mise activation line; it does not measure
repeated prompt renders.

This benchmark measures both sides of that claim:

| Measurement | What it captures |
| :-- | :-- |
| `zsh startup, no hooks (baseline)` | Startup time for interactive zsh with an empty rc; the script exits without rendering a prompt. |
| `zsh startup + mise activate` | The same, with `eval "$(mise activate zsh)"` in the rc. This captures startup overhead, not repeated prompt-hook cost. Only runs when mise is installed. |
| `runx <key>` | Total wall time of one `runx <key>` invocation with a warm cache — a one-time cost, paid only when you run it. |
| `RUNX_TIMINGS=1` breakdown | runx's own pre-child work (config discovery, cache lookup, PATH build + spawn). It is not directly comparable to the shell-startup measurement. |

## Results (this machine)

Machine: macOS 27.2.1, Apple silicon (arm64), zsh 5.9, hyperfine 1.20.0.
runx 0.6.2 built from `target/release`. mise 2026.7.18 (Homebrew).

| Benchmark | Mean | Min | Max |
| :-- | --: | --: | --: |
| zsh startup, no hooks (baseline) | 30.3 ms | 29.3 ms | 32.0 ms |
| zsh startup + mise activate | 48.9 ms | 47.9 ms | 50.0 ms |
| runx `<key>` (one-time, warm cache) | 11.4 ms | 10.5 ms | 12.3 ms |
| runx pre-child (config) | 0.07 ms | — | — |
| runx pre-child (cache) | 0.03 ms | — | — |
| runx pre-child (PATH + spawn) | 0.15 ms | — | — |

The startup with `mise activate` measured **18.6 ms above baseline** on this
machine. A full `runx <key>` run includes the child process; runx's measured
pre-child work is **~0.25 ms, paid once per invocation**. These measurements do
not time repeated prompt renders.

The structural point is that runx has no shell hook; its measured overhead is
paid when a `runx` command starts.

Honest caveat: these are microbenchmarks on one machine with a warm OS cache.
Numbers vary with machine, shell config, and filesystem. The relationship
they show — runx has no shell hook, and its measured pre-child work is
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
