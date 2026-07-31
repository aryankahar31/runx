#!/usr/bin/env sh
set -eu

REPO="${RUNX_REPO:-aryankahar31/runx}"
VERSION="${RUNX_VERSION:-latest}"
INSTALL_DIR="${RUNX_INSTALL_DIR:-$HOME/.runx/bin}"

os="$(uname -s | tr '[:upper:]' '[:lower:]')"
arch="$(uname -m)"

case "$os" in
  linux) platform="linux" ;;
  darwin) platform="macos" ;;
  *) echo "Unsupported OS: $os" >&2; exit 1 ;;
esac

case "$arch" in
  x86_64|amd64) cpu="x64" ;;
  arm64|aarch64) cpu="arm64" ;;
  *) echo "Unsupported architecture: $arch" >&2; exit 1 ;;
esac

asset="runx-${platform}-${cpu}.tar.gz"
if [ "$VERSION" = "latest" ]; then
  base_url="https://github.com/${REPO}/releases/latest/download"
else
  base_url="https://github.com/${REPO}/releases/download/${VERSION}"
fi
url="${base_url}/${asset}"

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

# ---------------------------------------------------------------------------
# SHA-256 checksum verification
# ---------------------------------------------------------------------------

# Detect available SHA-256 tool (macOS may lack sha256sum; Linux may lack shasum).
#
# A missing tool is a hard error rather than a silent skip: quietly installing
# an unverified binary is the one outcome checksum verification exists to
# prevent. Set RUNX_SKIP_CHECKSUM=1 to override deliberately.
sha256_cmd=""
if command -v sha256sum >/dev/null 2>&1; then
  sha256_cmd="sha256sum"
elif command -v shasum >/dev/null 2>&1; then
  sha256_cmd="shasum -a 256"
elif command -v openssl >/dev/null 2>&1; then
  sha256_cmd="openssl"
fi

if [ -z "$sha256_cmd" ] && [ "${RUNX_SKIP_CHECKSUM:-0}" != "1" ]; then
  cat >&2 <<'EOF'
Error: no SHA-256 tool found (looked for sha256sum, shasum, openssl).
Refusing to install an unverified binary.

Install one of those tools, or re-run with RUNX_SKIP_CHECKSUM=1 to accept
the risk explicitly.
EOF
  exit 1
fi

# Compute the SHA-256 hash of a file and print only the lowercase hex digest.
# Usage: compute_sha256 <filepath>
compute_sha256() {
  case "$sha256_cmd" in
    sha256sum) sha256sum "$1" | awk '{print tolower($1)}' ;;
    openssl)   openssl dgst -sha256 "$1" | awk '{print tolower($NF)}' ;;
    *)         shasum -a 256 "$1" | awk '{print tolower($1)}' ;;
  esac
}

# Verify a file against a SHA256SUMS manifest.
# Usage: verify_checksum <filepath> <filename> <sha256sums_path>
# Returns 0 on success, exits on failure.
#
# The lookup matches the filename field exactly rather than with `grep "$_name"`.
# An unanchored grep is wrong twice over: the name contains `.` characters that
# act as regex wildcards, and a substring match can select the digest of a
# different artifact whose name merely contains ours (e.g. a `.asc` signature
# line matching a request for the archive). The digest is also required to be
# 64 hex characters, so an HTML error page served in place of the manifest
# cannot be mistaken for a hash.
verify_checksum() {
  _file="$1"
  _name="$2"
  _sums="$3"

  _expected="$(
    awk -v name="$_name" '
      {
        field = $2
        sub(/^\*/, "", field)          # coreutils binary-mode marker
        sub(/^.*\//, "", field)        # compare basenames only
        if (field == name && $1 ~ /^[0-9a-fA-F]{64}$/) {
          print tolower($1)
          exit
        }
      }
    ' "$_sums"
  )"

  if [ -z "$_expected" ]; then
    echo "Error: no valid SHA-256 entry for $_name in SHA256SUMS." >&2
    exit 1
  fi

  _computed="$(compute_sha256 "$_file")"

  if [ "$_computed" != "$_expected" ]; then
    cat >&2 <<EOF
Error: checksum verification failed for $_name.
Expected: $_expected
Got:      $_computed
This may indicate a corrupted download or a compromised release. Aborting.
EOF
    exit 1
  fi

  echo "Checksum verified."
}

# ---------------------------------------------------------------------------
# Download, verify, and install
# ---------------------------------------------------------------------------

mkdir -p "$INSTALL_DIR"
echo "Downloading $url"
curl -fsSL --retry 3 --retry-delay 1 "$url" -o "$tmp/$asset"

if [ -n "$sha256_cmd" ]; then
  echo "Downloading SHA256SUMS"
  curl -fsSL --retry 3 --retry-delay 1 "${base_url}/SHA256SUMS" -o "$tmp/SHA256SUMS"
  verify_checksum "$tmp/$asset" "$asset" "$tmp/SHA256SUMS"
else
  # Only reachable when RUNX_SKIP_CHECKSUM=1 was set deliberately.
  echo "Warning: skipping checksum verification (RUNX_SKIP_CHECKSUM=1)." >&2
fi

tar -xzf "$tmp/$asset" -C "$tmp"

if [ ! -f "$tmp/runx" ]; then
  echo "Error: archive did not contain the expected runx binary." >&2
  exit 1
fi

install -m 0755 "$tmp/runx" "$INSTALL_DIR/runx"

echo "Installed runx to $INSTALL_DIR/runx"
case ":$PATH:" in
  *":$INSTALL_DIR:"*) ;;
  *) echo "Add $INSTALL_DIR to PATH to run runx from any directory." ;;
esac
