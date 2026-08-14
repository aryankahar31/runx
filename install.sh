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
        if (field == name && length($1) == 64 && $1 ~ /^[0-9a-fA-F]+$/) {
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
# Sigstore/cosign signature verification (graceful)
# ---------------------------------------------------------------------------
#
# Since v0.4.2 every release asset carries a Sigstore bundle (`<asset>.sigstore.json`)
# signed keylessly by the release workflow itself. `cosign` is not a hard
# dependency: most installers will not have it, and the checksum above already
# failed closed. So a missing `cosign` (or a release published before
# signatures existed) prints a warning and the install continues — set
# RUNX_REQUIRE_SIGNATURE=1 to make that a hard error instead. A signature that
# is present but FAILS verification is always fatal: that is the tampered
# release this feature exists to catch.
sigstore_identity='^https://github\.com/aryankahar31/runx/\.github/workflows/release\.yml@refs/(tags/v[0-9]+\.[0-9]+\.[0-9]+|heads/main)$'

verify_signature() {
  _file="$1"
  _bundle="$2"
  cosign verify-blob "$_file" \
    --bundle "$_bundle" \
    --certificate-identity-regexp "$sigstore_identity" \
    --certificate-oidc-issuer https://token.actions.githubusercontent.com
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

  # Signature verification. Skipping requires RUNX_SKIP_CHECKSUM=1, in which
  # case skipping the signature too is the user's explicit risk-to-accept.
  if command -v cosign >/dev/null 2>&1; then
    if curl -fsSL --retry 3 --retry-delay 1 "${base_url}/${asset}.sigstore.json" \
      -o "$tmp/$asset.sigstore.json" 2>/dev/null; then
      if verify_signature "$tmp/$asset" "$tmp/$asset.sigstore.json"; then
        echo "Signature verified."
      else
        cat >&2 <<'EOF'
Error: signature verification FAILED for the downloaded archive.
The Sigstore signature does not match the release workflow's identity or the
artifact has been tampered with. Aborting.
EOF
        exit 1
      fi
    elif [ "${RUNX_REQUIRE_SIGNATURE:-0}" = "1" ]; then
      echo "Error: RUNX_REQUIRE_SIGNATURE=1 but no signature bundle was published for this release." >&2
      exit 1
    else
      echo "Warning: no signature bundle published for this release; signature verification skipped (checksum still verified)." >&2
    fi
  else
    if [ "${RUNX_REQUIRE_SIGNATURE:-0}" = "1" ]; then
      echo "Error: RUNX_REQUIRE_SIGNATURE=1 but cosign is not installed." >&2
      exit 1
    fi
    cat >&2 <<'EOF'
Warning: cosign not found — release signature verification skipped.
The SHA-256 checksum was still verified and enforced. Install cosign
(https://docs.sigstore.dev/cosign/installation/) to verify release signatures.
EOF
  fi
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

  _shell_name="$(basename "${SHELL:-}")"
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
