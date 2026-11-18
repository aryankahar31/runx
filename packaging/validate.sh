#!/bin/sh
# Basic packaging-template check: required files, version placeholders, npm
# wrapper syntax, and fail-closed renderer behavior. It does not validate
# package-manager schemas, real release checksums, Nix hashes, or submissions.
set -eu

root="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"

failed=0
check() {
  if [ ! -f "$root/$1" ]; then
    echo "packaging: missing $1" >&2
    failed=1
  fi
}

for file in \
  homebrew/runx.rb \
  winget/aryankahar31.runx.yaml \
  winget/aryankahar31.runx.locale.en-US.yaml \
  winget/aryankahar31.runx.installer.yaml \
  scoop/runx.json \
  aur/PKGBUILD \
  nix/flake.nix \
  nix/package.nix \
  npm/package.json \
  npm/bin/runx.js \
  npm/README.md \
  render.sh \
  test-render.sh \
  test-install-version.sh; do
  check "$file"
done

# Every versioned manifest must be a template; render.sh substitutes the
# version at release time so a stale literal cannot ship.
for file in homebrew/runx.rb winget/aryankahar31.runx.yaml \
            winget/aryankahar31.runx.locale.en-US.yaml \
            winget/aryankahar31.runx.installer.yaml scoop/runx.json \
            aur/PKGBUILD nix/package.nix npm/package.json; do
  [ -f "$root/$file" ] || continue
  if ! grep -q '__VERSION__' "$root/$file"; then
    echo "packaging: $file has no __VERSION__ placeholder" >&2
    failed=1
  fi
done

# The npm wrapper is plain JavaScript; syntax-check it when node is available.
if command -v node >/dev/null 2>&1; then
  node --check "$root/npm/bin/runx.js"
fi

sh "$root/test-render.sh"
sh "$root/test-install-version.sh"

exit "$failed"
