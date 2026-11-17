#!/bin/sh
# Fill a release's version and archive digests into the package manifests.
#
#   packaging/render.sh <version> <dist-dir> <out-dir>
#
# <dist-dir> holds the GitHub release assets (runx-*.tar.gz / .zip). The
# rendered files are starting points for the Homebrew tap, Scoop/Winget
# AUR and npm submissions. Winget needs companion manifests. Nix hashes are
# intentionally left for `nix build` to reveal: they cannot be computed from a
# binary archive.
set -eu

version="$1"
dist="$2"
out="$3"
root="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"

sha() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

asset_sha() {
  file="$dist/$1"
  [ -f "$file" ] || { echo "render: missing release asset $file" >&2; exit 1; }
  sha "$file"
}

render() {
  src="$1"
  dest="$2"
  mkdir -p "$(dirname "$dest")"
  sed \
    -e "s|__VERSION__|$version|g" \
    -e "s|__SHA256_linux_x64__|$(asset_sha runx-linux-x64.tar.gz)|g" \
    -e "s|__SHA256_linux_arm64__|$(asset_sha runx-linux-arm64.tar.gz)|g" \
    -e "s|__SHA256_macos_x64__|$(asset_sha runx-macos-x64.tar.gz)|g" \
    -e "s|__SHA256_macos_arm64__|$(asset_sha runx-macos-arm64.tar.gz)|g" \
    -e "s|__SHA256_windows_x64__|$(asset_sha runx-windows-x64.zip)|g" \
    "$src" > "$dest"
}

render "$root/homebrew/runx.rb" "$out/homebrew/runx.rb"
render "$root/winget/aryankahar31.runx.yaml" "$out/winget/aryankahar31.runx.yaml"
render "$root/scoop/runx.json" "$out/scoop/runx.json"
render "$root/aur/PKGBUILD" "$out/aur/PKGBUILD"
render "$root/nix/package.nix" "$out/nix/package.nix"
mkdir -p "$out/nix"
cp "$root/nix/flake.nix" "$out/nix/flake.nix"
render "$root/npm/package.json" "$out/npm/package.json"
mkdir -p "$out/npm/bin"
cp "$root/npm/bin/runx.js" "$out/npm/bin/runx.js"
cp "$root/npm/README.md" "$out/npm/README.md"

echo "Rendered v$version package manifests in $out"
