#!/bin/sh
# Fill a release's version and archive digests into the package manifests.
#
#   packaging/render.sh <version> <dist-dir> <out-dir>
#
# <dist-dir> holds the GitHub release assets (runx-*.tar.gz / .zip). The
# rendered files are starting points for package submissions. Nix source and
# Cargo hashes remain documented placeholders for `nix build` to resolve.
set -eu

if [ "$#" -ne 3 ]; then
  echo "usage: packaging/render.sh <version> <dist-dir> <out-dir>" >&2
  exit 2
fi

version="$1"
dist="$2"
out="$3"
root="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"

sha() {
  file="$1"
  if command -v sha256sum >/dev/null 2>&1; then
    if output=$(sha256sum "$file"); then
      :
    else
      echo "render: failed to compute SHA-256 for $file with sha256sum" >&2
      return 1
    fi
  else
    if command -v shasum >/dev/null 2>&1; then
      if output=$(shasum -a 256 "$file"); then
        :
      else
        echo "render: failed to compute SHA-256 for $file with shasum" >&2
        return 1
      fi
    else
      echo "render: no SHA-256 utility found (need sha256sum or shasum)" >&2
      return 1
    fi
  fi

  digest=${output%%[[:space:]]*}
  case "$digest" in
    ''|*[!0123456789abcdefABCDEF]*)
      echo "render: invalid SHA-256 for $file (expected 64 hexadecimal characters)" >&2
      return 1
      ;;
  esac
  if [ "${#digest}" -ne 64 ]; then
    echo "render: invalid SHA-256 for $file (expected 64 hexadecimal characters)" >&2
    return 1
  fi

  printf '%s\n' "$digest"
}

asset_sha() {
  file="$dist/$1"
  [ -f "$file" ] || { echo "render: missing release asset $file" >&2; exit 1; }
  sha "$file" || exit 1
}

# Validate every archive before creating the output directory or writing any
# manifest. This keeps a failed checksum tool from leaving partial packages.
sha_linux_x64=$(asset_sha runx-linux-x64.tar.gz) || exit 1
sha_linux_arm64=$(asset_sha runx-linux-arm64.tar.gz) || exit 1
sha_macos_x64=$(asset_sha runx-macos-x64.tar.gz) || exit 1
sha_macos_arm64=$(asset_sha runx-macos-arm64.tar.gz) || exit 1
sha_windows_x64=$(asset_sha runx-windows-x64.zip) || exit 1

render() {
  src="$1"
  dest="$2"
  mkdir -p "$(dirname "$dest")"
  sed \
    -e "s|__VERSION__|$version|g" \
    -e "s|__SHA256_linux_x64__|$sha_linux_x64|g" \
    -e "s|__SHA256_linux_arm64__|$sha_linux_arm64|g" \
    -e "s|__SHA256_macos_x64__|$sha_macos_x64|g" \
    -e "s|__SHA256_macos_arm64__|$sha_macos_arm64|g" \
    -e "s|__SHA256_windows_x64__|$sha_windows_x64|g" \
    "$src" > "$dest"
}

render "$root/homebrew/runx-launcher.rb" "$out/homebrew/runx-launcher.rb"
render "$root/winget/aryankahar31.runx-launcher.yaml" "$out/winget/aryankahar31.runx-launcher.yaml"
render "$root/winget/aryankahar31.runx-launcher.locale.en-US.yaml" "$out/winget/aryankahar31.runx-launcher.locale.en-US.yaml"
render "$root/winget/aryankahar31.runx-launcher.installer.yaml" "$out/winget/aryankahar31.runx-launcher.installer.yaml"
render "$root/scoop/runx-launcher.json" "$out/scoop/runx-launcher.json"
render "$root/aur/PKGBUILD" "$out/aur/PKGBUILD"
render "$root/nix/package.nix" "$out/nix/package.nix"
mkdir -p "$out/nix"
cp "$root/nix/flake.nix" "$out/nix/flake.nix"
render "$root/npm/package.json" "$out/npm/package.json"
mkdir -p "$out/npm/bin"
cp "$root/npm/bin/runx.js" "$out/npm/bin/runx.js"
cp "$root/npm/README.md" "$out/npm/README.md"

echo "Rendered v$version package manifests in $out"
