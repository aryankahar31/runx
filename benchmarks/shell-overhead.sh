#!/usr/bin/env bash
# Shell/startup overhead: runx vs mise vs a plain shell.
#
# mise hooks every interactive prompt via `eval "$(mise activate zsh)"`;
# runx has no shell integration, so it costs nothing per prompt. This script
# measures both claims:
#
#   1. zsh startup time with a clean config (baseline)
#   2. zsh startup time with `mise activate` in the config   (if mise installed)
#   3. the one-time cost of a `runx <key>` invocation with a warm cache
#
# The runx project is hermetic: a fake cached runtime and an exact-version pin
# mean no network access and no dependence on your real ~/.runx.
#
# Requirements: zsh, hyperfine (brew install hyperfine). mise is optional.
# Usage: benchmarks/shell-overhead.sh [path-to-runx-binary]
set -eu

cd "$(dirname "$0")"

RUNX_BIN="${1:-${RUNX_BIN:-runx}}"
WARMUP=3
RUNS=20

command -v zsh >/dev/null 2>&1 || { echo "error: zsh is required" >&2; exit 1; }

if ! command -v hyperfine >/dev/null 2>&1; then
    echo "hyperfine not found." >&2
    if command -v brew >/dev/null 2>&1; then
        echo "installing via: brew install hyperfine" >&2
        brew install hyperfine
    fi
    command -v hyperfine >/dev/null 2>&1 || {
        echo "error: hyperfine is still unavailable; install it with 'brew install hyperfine' and rerun" >&2
        exit 1
    }
fi

if ! command -v "$RUNX_BIN" >/dev/null 2>&1; then
    echo "error: runx binary not found at '$RUNX_BIN' (pass its path as \$1)" >&2
    exit 1
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Hermetic benchmark project: a fake cached "node" runtime plus an exact-pin
# config, so the measured invocation is pure runx overhead, no network.
bench="$work/project"
cache="$work/cache"
mkdir -p "$bench" "$cache/runtimes/node/20.11.0/bin"
printf '#!/bin/sh\nexit 0\n' > "$cache/runtimes/node/20.11.0/bin/node"
chmod +x "$cache/runtimes/node/20.11.0/bin/node"
cat > "$bench/runx.toml" <<'TOML'
[runtimes]
node = "20.11.0"

[run]
bench = "node --version"
TOML

# One warm-up run so the cache-adoption receipt is written before measuring.
(cd "$bench" && RUNX_HOME="$cache" "$RUNX_BIN" bench >/dev/null)

# Interactive zsh with a controlled ZDOTDIR: an empty rc is the baseline, one
# with `mise activate` is the activated shell. User-specific rc overhead is
# deliberately excluded — the delta isolates mise's hook cost.
mkdir -p "$work/zdot-plain" "$work/zdot-mise"
: > "$work/zdot-plain/.zshrc"
printf 'eval "$(mise activate zsh)"\n' > "$work/zdot-mise/.zshrc"

commands=("ZDOTDIR=$work/zdot-plain zsh -i -c exit")
labels=("zsh startup, no hooks (baseline)")
if command -v mise >/dev/null 2>&1; then
    commands+=("ZDOTDIR=$work/zdot-mise zsh -i -c exit")
    labels+=("zsh startup + mise activate (per-prompt cost)")
else
    echo "mise not installed — skipping the mise comparison." >&2
    echo "Install with 'brew install mise', rerun, and it will be measured." >&2
fi
commands+=("cd $bench && RUNX_HOME=$cache $RUNX_BIN bench")
labels+=("runx <key> (one-time, warm cache)")

args=(hyperfine --warmup "$WARMUP" --runs "$RUNS" --export-json results.json --export-csv results.csv)
for i in "${!commands[@]}"; do
    args+=(-n "${labels[$i]}" "${commands[$i]}")
done
"${args[@]}"

echo
echo "Machine: $(uname -sm), macOS $(sw_vers -productVersion), zsh $(zsh --version | cut -d' ' -f2)"
echo "runx: $("$RUNX_BIN" --version)"
echo
echo "runx pre-child breakdown (RUNX_TIMINGS=1, one sample):"
(cd "$bench" && RUNX_HOME="$cache" RUNX_TIMINGS=1 "$RUNX_BIN" bench >/dev/null)
echo "Results written to benchmarks/results.json and benchmarks/results.csv"
