#!/bin/sh
set -eu

root="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
work="$(mktemp -d "${TMPDIR:-/tmp}/runx-render-test.XXXXXX")"
trap 'rm -rf "$work"' EXIT HUP INT TERM

dist="$work/dist"
fakebin="$work/fakebin"
mkdir -p "$dist" "$fakebin"
: > "$dist/runx-linux-x64.tar.gz"
: > "$dist/runx-linux-arm64.tar.gz"
: > "$dist/runx-macos-x64.tar.gz"
: > "$dist/runx-macos-arm64.tar.gz"
: > "$dist/runx-windows-x64.zip"

cat > "$fakebin/sha256sum" <<'SH'
#!/bin/sh
count=0
if [ -f "$COUNT_FILE" ]; then
  count=$(cat "$COUNT_FILE")
fi
count=$((count + 1))
printf '%s\n' "$count" > "$COUNT_FILE"

if [ "$FAKE_HASH_MODE" = fail-late ] && [ "$count" -eq 5 ]; then
  echo "synthetic checksum failure" >&2
  exit 1
fi
if [ "$FAKE_HASH_MODE" = invalid ] && [ "$count" -eq 3 ]; then
  printf 'not-a-sha256  %s\n' "$1"
  exit 0
fi
if [ "$FAKE_HASH_MODE" = empty ] && [ "$count" -eq 3 ]; then
  printf '  %s\n' "$1"
  exit 0
fi

printf '%s  %s\n' "0000000000000000000000000000000000000000000000000000000000000000" "$1"
SH
chmod +x "$fakebin/sha256sum"

run_case() {
  mode="$1"
  expected="$2"
  out="$work/out-$mode"
  stderr="$work/$mode.stderr"
  count_file="$work/$mode.count"

  if PATH="$fakebin:$PATH" FAKE_HASH_MODE="$mode" COUNT_FILE="$count_file" \
    "$root/render.sh" 0.7.1 "$dist" "$out" > "$work/$mode.stdout" 2> "$stderr"; then
    echo "test-render: expected $mode checksum case to fail" >&2
    exit 1
  fi

  if [ -e "$out" ]; then
    echo "test-render: $mode checksum failure created output at $out" >&2
    exit 1
  fi
  if ! grep -F "$expected" "$stderr" >/dev/null; then
    echo "test-render: missing expected error '$expected' for $mode case" >&2
    cat "$stderr" >&2
    exit 1
  fi
}

run_case fail-late "failed to compute SHA-256"
run_case invalid "invalid SHA-256"
run_case empty "invalid SHA-256"

out="$work/out-success"
PATH="$fakebin:$PATH" FAKE_HASH_MODE=success COUNT_FILE="$work/success.count" \
  "$root/render.sh" 0.7.1 "$dist" "$out" > "$work/success.stdout"

for file in \
  homebrew/runx-launcher.rb \
  winget/aryankahar31.runx-launcher.yaml \
  winget/aryankahar31.runx-launcher.locale.en-US.yaml \
  winget/aryankahar31.runx-launcher.installer.yaml \
  scoop/runx-launcher.json \
  aur/PKGBUILD \
  nix/package.nix \
  nix/flake.nix \
  npm/package.json \
  npm/bin/runx.js \
  npm/README.md; do
  if [ ! -f "$out/$file" ]; then
    echo "test-render: successful render is missing $file" >&2
    exit 1
  fi
done

if grep -R -E '__VERSION__|__SHA256_[[:alnum:]_]*__' "$out"; then
  echo "test-render: a version or archive checksum placeholder remains" >&2
  exit 1
fi
if ! grep -F 'PackageVersion: 0.7.1' "$out/winget/aryankahar31.runx-launcher.yaml" >/dev/null; then
  echo "test-render: Winget version manifest was not rendered" >&2
  exit 1
fi
if ! grep -F 'PackageIdentifier: aryankahar31.runx-launcher' "$out/winget/aryankahar31.runx-launcher.yaml" >/dev/null; then
  echo "test-render: Winget package identifier was not renamed" >&2
  exit 1
fi
if ! grep -F '"name": "runx-launcher"' "$out/npm/package.json" >/dev/null ||
   ! grep -F '"runx": "bin/runx.js"' "$out/npm/package.json" >/dev/null; then
  echo "test-render: npm package name or command mapping is wrong" >&2
  exit 1
fi
if ! grep -F 'bin-dir = "runx{ binary-ext }"' "$root/../Cargo.toml" >/dev/null; then
  echo "test-render: cargo-binstall must select the runx binary" >&2
  exit 1
fi
if ! grep -F 'hash = "__SOURCE_HASH__"' "$out/nix/package.nix" >/dev/null ||
   ! grep -F 'cargoHash = "__CARGO_HASH__"' "$out/nix/package.nix" >/dev/null; then
  echo "test-render: Nix hash placeholders were not preserved" >&2
  exit 1
fi

echo "test-render: checksum failures leave no output; valid digests render all manifests"
