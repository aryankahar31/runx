#!/bin/sh
set -eu

root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
work="$(mktemp -d "${TMPDIR:-/tmp}/runx-install-version-test.XXXXXX")"
trap 'rm -rf "$work"' EXIT HUP INT TERM

fakebin="$work/fakebin"
mkdir -p "$fakebin"
cat > "$fakebin/curl" <<'SH'
#!/bin/sh
: > "$CURL_STUB_MARKER"
exit 97
SH
chmod +x "$fakebin/curl"

capture_base_url() {
  name="$1"
  version="$2"
  stdout="$work/$name.stdout"
  stderr="$work/$name.stderr"
  marker="$work/$name.curl-called"

  if PATH="$fakebin:$PATH" \
    RUNX_VERSION="$version" \
    RUNX_REPO="aryankahar31/runx" \
    RUNX_INSTALL_DIR="$work/$name-install" \
    RUNX_SKIP_CHECKSUM=1 \
    CURL_STUB_MARKER="$marker" \
    sh "$root/install.sh" > "$stdout" 2> "$stderr"; then
    echo "test-install-version: expected curl stub to stop the installer" >&2
    exit 1
  fi

  if [ ! -f "$marker" ]; then
    echo "test-install-version: installer did not call the curl stub" >&2
    cat "$stderr" >&2
    exit 1
  fi

  line=$(sed -n 's/^Downloading //p' "$stdout" | sed -n '1p')
  case "$line" in
    https://github.com/aryankahar31/runx/releases/download/v0.7.1/runx-*) ;;
    *)
      echo "test-install-version: unexpected download URL for $version: $line" >&2
      exit 1
      ;;
  esac
  printf '%s\n' "${line%/runx-*}"
}

prefixed_base=$(capture_base_url prefixed v0.7.1)
bare_base=$(capture_base_url bare 0.7.1)

if [ "$prefixed_base" != "$bare_base" ]; then
  echo "test-install-version: version forms produced different URLs:" >&2
  echo "  v-prefixed: $prefixed_base" >&2
  echo "  bare:       $bare_base" >&2
  exit 1
fi

echo "test-install-version: RUNX_VERSION=v0.7.1 and 0.7.1 both resolve to $prefixed_base"
