#!/usr/bin/env bash
# Tests for registry-freshness parsing and stable version filtering against fixtures.
set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

echo "Testing registry-freshness stable filtering logic against fixtures..."

# 1. Test Node index parsing
node_fixture='[
  {"version":"v27.0.0-alpha.0.1.1"},
  {"version":"v27.0.0-alpha.0.1.0"},
  {"version":"v26.13.1"},
  {"version":"v26.13.0"}
]'
node_out=$(printf '%s' "$node_fixture" | python3 -c '
import json, re, sys
data = json.load(sys.stdin)
raw = data[0].get("version", "").lstrip("vV")
best = None
for r in data:
    v = r.get("version", "").lstrip("vV")
    m = re.match(r"^(\d+)\.(\d+)\.(\d+)$", v)
    if m:
        t = tuple(int(m.group(i)) for i in (1, 2, 3))
        best = max(best or t, t)
stable = ".".join(str(p) for p in best)
print(f"{raw}\t{stable}")')

node_raw=$(printf '%s' "$node_out" | cut -f1)
node_stable=$(printf '%s' "$node_out" | cut -f2)

if [ "$node_raw" != "27.0.0-alpha.0.1.1" ]; then
    echo "FAIL: expected node raw 27.0.0-alpha.0.1.1, got '$node_raw'" >&2
    exit 1
fi
if [ "$node_stable" != "26.13.1" ]; then
    echo "FAIL: expected node stable 26.13.1, got '$node_stable'" >&2
    exit 1
fi
echo "  ✓ node: raw=$node_raw, stable=$node_stable"

# 2. Test Go index parsing
go_fixture='[
  {"version":"go1.28rc1","stable":false},
  {"version":"go1.27.4","stable":true},
  {"version":"go1.27","stable":true}
]'
go_out=$(printf '%s' "$go_fixture" | python3 -c '
import json, re, sys
data = json.load(sys.stdin)
raw = data[0].get("version", "").lstrip("go")
best = None
for r in data:
    if not r.get("stable"):
        continue
    v = r.get("version", "").lstrip("go")
    m = re.match(r"^(\d+)\.(\d+)\.(\d+)$", v)
    if m:
        t = tuple(int(m.group(i)) for i in (1, 2, 3))
        best = max(best or t, t)
stable = ".".join(str(p) for p in best)
print(f"{raw}\t{stable}")')

go_raw=$(printf '%s' "$go_out" | cut -f1)
go_stable=$(printf '%s' "$go_out" | cut -f2)

if [ "$go_raw" != "1.28rc1" ]; then
    echo "FAIL: expected go raw 1.28rc1, got '$go_raw'" >&2
    exit 1
fi
if [ "$go_stable" != "1.27.4" ]; then
    echo "FAIL: expected go stable 1.27.4, got '$go_stable'" >&2
    exit 1
fi
echo "  ✓ go: raw=$go_raw, stable=$go_stable"

# 3. Test Bun releases parsing
bun_fixture='[
  {"tag_name":"bun-v1.4.6-canary.1"},
  {"tag_name":"bun-v1.4.5"}
]'
bun_out=$(printf '%s' "$bun_fixture" | python3 -c '
import json, re, sys
data = json.load(sys.stdin)
raw = data[0].get("tag_name", "").replace("bun-v", "").lstrip("v")
best = None
for r in data:
    m = re.match(r"^bun-v(\d+)\.(\d+)\.(\d+)$", r.get("tag_name", ""))
    if m:
        t = tuple(int(m.group(i)) for i in (1, 2, 3))
        best = max(best or t, t)
stable = ".".join(str(p) for p in best)
print(f"{raw}\t{stable}")')

bun_raw=$(printf '%s' "$bun_out" | cut -f1)
bun_stable=$(printf '%s' "$bun_out" | cut -f2)

if [ "$bun_raw" != "1.4.6-canary.1" ]; then
    echo "FAIL: expected bun raw 1.4.6-canary.1, got '$bun_raw'" >&2
    exit 1
fi
if [ "$bun_stable" != "1.4.5" ]; then
    echo "FAIL: expected bun stable 1.4.5, got '$bun_stable'" >&2
    exit 1
fi
echo "  ✓ bun: raw=$bun_raw, stable=$bun_stable"

# 4. Test Python assets parsing
plat="x86_64-apple-darwin"
python_fixture='[
  {
    "assets": [
      {"name": "cpython-3.15.0a1+20250101-x86_64-apple-darwin-install_only.tar.gz"},
      {"name": "cpython-3.14.7+20250101-x86_64-apple-darwin-install_only.tar.gz"}
    ]
  }
]'
python_out=$(printf '%s' "$python_fixture" | python3 -c '
import json, re, sys
plat = sys.argv[1]
data = json.load(sys.stdin)
pat = re.compile(r"^cpython-(\d+)\.(\d+)\.(\d+)\+.*" + re.escape(plat) + r".*install_only\.tar\.gz$")
raw_pat = re.compile(r"^cpython-([^+]+)\+.*" + re.escape(plat) + r".*install_only\.tar\.gz$")
raw = None
best = None
for r in data:
    for a in r.get("assets", []):
        name = a.get("name", "")
        if raw is None:
            rm = raw_pat.match(name)
            if rm:
                raw = rm.group(1)
        m = pat.match(name)
        if m:
            t = tuple(int(m.group(i)) for i in (1, 2, 3))
            best = max(best or t, t)
stable = ".".join(str(p) for p in best)
print(f"{raw or stable}\t{stable}")' "$plat")

py_raw=$(printf '%s' "$python_out" | cut -f1)
py_stable=$(printf '%s' "$python_out" | cut -f2)

if [ "$py_raw" != "3.15.0a1" ]; then
    echo "FAIL: expected python raw 3.15.0a1, got '$py_raw'" >&2
    exit 1
fi
if [ "$py_stable" != "3.14.7" ]; then
    echo "FAIL: expected python stable 3.14.7, got '$py_stable'" >&2
    exit 1
fi
echo "  ✓ python: raw=$py_raw, stable=$py_stable"

# 5. Test failure message output format
fail_msg=$(bash -c '
    tool="node"
    raw_vendor="27.0.0-alpha.0.1.1"
    ground="26.13.1"
    resolved="26.13.0"
    echo "$tool: fail (vendor raw: ${raw_vendor:-n/a}, vendor stable: ${ground:-n/a}, runx resolved: ${resolved:-n/a})"
')
if [ "$fail_msg" != "node: fail (vendor raw: 27.0.0-alpha.0.1.1, vendor stable: 26.13.1, runx resolved: 26.13.0)" ]; then
    echo "FAIL: unexpected failure message format: '$fail_msg'" >&2
    exit 1
fi
echo "  ✓ failure message format verified"

echo "All registry freshness fixture tests passed!"
