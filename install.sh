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

# Add the install directory only with the user's permission.  In particular,
# an installer invoked as `curl ... | sh` must not wait for input that cannot
# be provided.
path_setup_instructions() {
  _rc_file="$1"
  _path_line="$2"

  echo "Runx is installed but not on your PATH."
  if [ -n "$_rc_file" ]; then
    echo "Add this line to $_rc_file:"
  else
    echo "Add this line to your shell's startup file:"
  fi
  printf '\n  %s\n\n' "$_path_line"
}

setup_path() {
  case ":$PATH:" in
    *":$INSTALL_DIR:"*) return ;;
  esac

  # Keep the default entry portable if the home directory changes.  Custom
  # directories are written as supplied.
  case "$INSTALL_DIR" in
    "$HOME") _path_entry='$HOME' ;;
    "$HOME"/*) _path_entry='$HOME'"${INSTALL_DIR#"$HOME"}" ;;
    *) _path_entry="$INSTALL_DIR" ;;
  esac
  _path_line="export PATH=\"$_path_entry:\$PATH\""

  _shell_name="${SHELL##*/}"
  _rc_file=""
  case "$_shell_name" in
    zsh) _rc_file="$HOME/.zprofile" ;;
    bash)
      if [ -f "$HOME/.bash_profile" ]; then
        _rc_file="$HOME/.bash_profile"
      else
        _rc_file="$HOME/.bashrc"
      fi
      ;;
  esac

  if [ "${RUNX_INSTALL_NO_MODIFY_PATH:-}" = "1" ] || [ ! -t 0 ] || [ -z "$_rc_file" ]; then
    path_setup_instructions "$_rc_file" "$_path_line"
    return
  fi

  printf 'Runx is installed but not on your PATH.\nAdd it automatically to %s? [Y/n] ' "$_rc_file"
  _reply=""
  if ! IFS= read -r _reply; then
    printf '\n'
    path_setup_instructions "$_rc_file" "$_path_line"
    return
  fi
  case "$_reply" in
    ""|y|Y|yes|YES|Yes)
      if [ -f "$_rc_file" ] && grep -Fqx "$_path_line" "$_rc_file"; then
        echo "PATH entry already exists in $_rc_file."
      else
        {
          printf '\n# Added by runx installer\n%s\n' "$_path_line"
        } >> "$_rc_file"
        echo "Added runx to PATH in $_rc_file."
      fi
      echo "Restart your terminal or run: source $_rc_file"
      ;;
    *) path_setup_instructions "$_rc_file" "$_path_line" ;;
  esac
}

setup_path
