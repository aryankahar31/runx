#!/usr/bin/env bash
# Registry freshness: runx resolves ranges directly from each vendor's own
# release index, with only a 6-hour local cache (registry.rs INDEX_TTL_SECS) —
# no third-party sync service in between. This script keeps that claim
# falsifiable:
#
#   1. fetch the latest version from the vendor's canonical source (curl,
#      parsed independently with python3)
#   2. let runx resolve a fresh "*" range in an isolated RUNX_HOME, with a
#      fake runtime planted at the expected version — resolution is
#      exercised end-to-end (index fetch -> best_match -> lockfile) with no
#      downloads when the claim holds
#   3. compare the runx.lock pin against the ground truth
#
# A mismatch fails the script (exit 1). Runtimes whose vendor source cannot
# be scripted on this platform are skipped with a message, never faked.
#
# Usage: benchmarks/registry-freshness.sh [path-to-runx-binary]
# Writes benchmarks/registry-results.json (also uploaded as a CI artifact).
set -eu

cd "$(dirname "$0")"

RUNX_BIN="${1:-${RUNX_BIN:-runx}}"
RESULTS="registry-results.json"

command -v curl >/dev/null 2>&1 || { echo "error: curl is required" >&2; exit 1; }
command -v python3 >/dev/null 2>&1 || { echo "error: python3 is required for parsing" >&2; exit 1; }
command -v "$RUNX_BIN" >/dev/null 2>&1 || {
    echo "error: runx binary not found at '$RUNX_BIN' (pass its path as \$1)" >&2
    exit 1
}

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Python asset platform (matches runtime.rs::python_platform).
python_platform() {
    case "$(uname -s)/$(uname -m)" in
        Linux/x86_64) echo "x86_64-unknown-linux-gnu" ;;
        Linux/aarch64) echo "aarch64-unknown-linux-gnu" ;;
        Darwin/x86_64) echo "x86_64-apple-darwin" ;;
        Darwin/arm64) echo "aarch64-apple-darwin" ;;
        *) echo "" ;;
    esac
}

# Ground truth: the newest release the vendor's canonical index advertises.
latest_node() {
    curl -fsSL --retry 3 --retry-all-errors --retry-delay 2 --max-time 60 https://nodejs.org/dist/index.json | python3 -c '
import json, sys
print(json.load(sys.stdin)[0]["version"].lstrip("v"))'
}

latest_go() {
    curl -fsSL --retry 3 --retry-all-errors --retry-delay 2 --max-time 60 "https://go.dev/dl/?mode=json&include=all" | python3 -c '
import json, sys
print(next(r for r in json.load(sys.stdin) if r["stable"])["version"].lstrip("go"))'
}

latest_bun() {
    curl -fsSL --retry 3 --retry-all-errors --retry-delay 2 --max-time 60 "https://api.github.com/repos/oven-sh/bun/releases?per_page=100" | python3 -c '
import json, re, sys
best = None
for r in json.load(sys.stdin):
    m = re.match(r"bun-v(\d+)\.(\d+)\.(\d+)$", r["tag_name"])
    if m:
        t = tuple(int(m.group(i)) for i in (1, 2, 3))
        best = max(best or t, t)
if best is None:
    raise SystemExit("no stable bun-vX.Y.Z tag found")
print(".".join(str(p) for p in best))'
}

latest_python() {
    local plat
    plat="$(python_platform)"
    if [ -z "$plat" ]; then
        echo "skip: python-build-standalone asset names are platform-specific and $(uname -s)/$(uname -m) is not mapped" >&2
        return 2
    fi
    curl -fsSL --retry 3 --retry-all-errors --retry-delay 2 --max-time 60 "https://api.github.com/repos/astral-sh/python-build-standalone/releases?per_page=10" | python3 -c '
import json, re, sys
plat = sys.argv[1]
pat = re.compile(r"cpython-(\d+)\.(\d+)\.(\d+)\+.*" + re.escape(plat) + r".*install_only\.tar\.gz$")
best = None
for r in json.load(sys.stdin):
    for a in r["assets"]:
        m = pat.match(a["name"])
        if m:
            t = tuple(int(m.group(i)) for i in (1, 2, 3))
            best = max(best or t, t)
if best is None:
    raise SystemExit(f"no install_only cpython asset for {plat}")
print(".".join(str(p) for p in best))' "$plat"
}

# One runtime: ground truth vs runx resolution. Records JSON line to $res.
check() {
    local tool="$1" ground="" resolved="" status="fail" source=""
    local ground_out
    ground_out="$(latest_$tool 2>&1)" || {
        if [ "$?" -eq 2 ]; then status="skip"; ground=""; else
            echo "  $tool: FAILED to fetch ground truth: $(printf '%s' "$ground_out" | head -1)" >&2
        fi
        printf '%s\t%s\t%s\t%s\n' "$tool" "$status" "$ground" "$resolved" >> "$res"
        echo "$tool: $status"
        return 0
    }
    ground="$ground_out"

    # Vendor index fetches can hit transient network failures (runx's fetch
    # itself has no retry); retry the resolution a few times before failing.
    local attempt
    for attempt in 1 2 3; do
        local proj="$work/proj-$tool-$attempt" cache="$work/cache-$tool-$attempt"
        mkdir -p "$proj" "$cache/runtimes/$tool/$ground/bin"
        printf '#!/bin/sh\nexit 0\n' > "$cache/runtimes/$tool/$ground/bin/$tool"
        chmod +x "$cache/runtimes/$tool/$ground/bin/$tool"
        printf '[runtimes]\n%s = "*"\n\n[run]\nbench = "echo x"\n' "$tool" > "$proj/runx.toml"

        (cd "$proj" && RUNX_HOME="$cache" "$RUNX_BIN" lock >/dev/null 2>&1) &
        pid=$!
        # macOS has no `timeout`; wait up to 90s, then kill. Python's index
        # (10 pages of python-build-standalone releases) is ~150MB cold, so a
        # slow network legitimately exceeds this on the first attempt.
        ok=0
        for _ in $(seq 1 90); do
            if ! kill -0 "$pid" 2>/dev/null; then ok=1; break; fi
            sleep 1
        done
        [ "$ok" -eq 1 ] || kill "$pid" 2>/dev/null || true
        wait "$pid" 2>/dev/null || true

        if [ -f "$proj/runx.lock" ]; then
            resolved="$(grep -A2 "^\[runtimes\.$tool\]" "$proj/runx.lock" | grep '^version' | cut -d'"' -f2)"
            break
        fi
        resolved=""
    done

    if [ "$resolved" = "$ground" ]; then status="pass"; else status="fail"; fi
    printf '%s\t%s\t%s\t%s\n' "$tool" "$status" "$ground" "$resolved" >> "$res"
    echo "$tool: $status (vendor latest: ${ground:-n/a}, runx resolved: ${resolved:-n/a})"
}

res="$work/results"
: > "$res"
echo "Checking registry freshness against vendor indexes..."
check node
check go
check bun
check python

# Collate results.json and derive the exit code.
status="pass"
{
    echo "{"
    first=1
    while IFS=$'\t' read -r tool st ground resolved; do
        [ "$first" -eq 0 ] && echo ","
        first=0
        printf '  "%s": {"status": "%s", "vendor_latest": "%s", "runx_resolved": "%s"}' \
            "$tool" "$st" "${ground:-}" "${resolved:-}"
        [ "$st" != "pass" ] && [ "$st" != "skip" ] && status="fail"
    done < "$res"
    echo
    echo "}"
} > "$RESULTS"

echo
echo "Results written to benchmarks/registry-results.json"
[ "$status" = "pass" ] || { echo "registry freshness check FAILED" >&2; exit 1; }
echo "registry freshness check passed"
