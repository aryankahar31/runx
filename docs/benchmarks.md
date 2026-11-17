# Benchmarks

## Shell overhead

mise, asdf and nvm integrate with the shell in different ways. runx does not:
there is no activation line, shim directory, or directory hook. **Nothing
happens until you type `runx <key>`, and runx's overhead is paid once per
invocation.**

Measured on macOS 27.2.1 (Apple silicon, zsh 5.9, hyperfine 1.20, runx 0.6.2, mise 2026.7.18, `--warmup 3 --runs 20`; full method in [`benchmarks/README.md`](../benchmarks/README.md)):

| Measurement | Mean |
| :-- | --: |
| zsh startup, no hooks (baseline) | 30.3 ms |
| zsh startup + `mise activate` | 48.9 ms — **+18.6 ms versus baseline** |
| `runx <key>` total, warm cache | 11.4 ms |
| — runx pre-child work (`RUNX_TIMINGS=1`): config | 0.07 ms |
| — cache lookup | 0.03 ms |
| — PATH build + spawn | 0.15 ms |

runx's measured contribution before your command starts is **~0.25 ms, once**.
The 18.6 ms figure is the difference in this harness's zsh startup timings; it
does not measure repeated prompt renders. A complete `runx <key>` run includes
the child process, so it should not be read as a prompt-to-command comparison.

These are directional startup and invocation numbers on one machine, not a
universal guarantee. The structural claim — *runx installs no shell hook* —
does not depend on the measurements.

## Registry freshness verification

The weekly freshness workflow checks Node, Go, Bun, and Python against their
vendor indexes. Java and Deno are not included in that probe. Results and
caveats live in [runtime-resolution.md](runtime-resolution.md#registry-freshness-no-third-party-sync).
