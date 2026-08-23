# Security Model

## Summary of hardening guarantees

- **SHA-256 verification** of every download, before extraction
- **Sigstore/cosign signature verification** (keyless) on release archives and `SHA256SUMS`
- **Atomic installs** — extraction happens in `.staging-*` directories and is renamed into place only after verification; an interrupted download can never corrupt the cache
- **Strict version validation at a single chokepoint** (`src/runtime.rs::resolve_runtime`) — version strings become filesystem paths and download URLs, so path traversal is rejected before anything else happens
- **Archive extraction hardening** — symlink escape rejected, exec bits preserved
- **Fail-closed installers** — no checksum tool available means no install

## Install-script verification

Both install scripts (`install.sh` / `install.ps1`) verify the downloaded binary against the SHA-256 checksum published with each release, and abort without extracting or installing anything if it does not match. They also **fail closed**: if no SHA-256 tool is available (`sha256sum`, `shasum`, or `openssl`), the install stops rather than silently proceeding unverified. Set `RUNX_SKIP_CHECKSUM=1` to override that deliberately.

Checksums confirm the download is intact and matches what the publisher listed. They are fetched from the same origin as the artifact, so they are not by themselves protection against a compromised release host.

### Signature verification (Sigstore/cosign)

Since v0.4.2 every release archive and the `SHA256SUMS` manifest are additionally signed with Sigstore/cosign *keyless* signing: the release workflow asks GitHub for an OIDC token, and Sigstore's Fulcio CA issues a short-lived certificate bound to that identity (the workflow path, repository, and triggering ref — no private keys are stored anywhere). When `cosign` is installed, the install scripts and `runx self update` verify each download against that signature with the identity pinned to `aryankahar31/runx`'s `release.yml` on a `v*.*.*` tag, so only a real run of the release workflow can have produced the file.

This closes the checksum gap: a compromised release host cannot forge a signature without also compromising the signing identity.

Verification degrades gracefully and honestly: if `cosign` is not installed (or you are installing a release published before v0.4.2, which has no signature), a warning is printed and the install proceeds on checksum verification alone — the fail-closed checksum behavior above is unchanged. Set `RUNX_REQUIRE_SIGNATURE=1` to make a missing `cosign` or signature an error instead of a warning. A signature that is present but *fails* verification is always fatal: that is the tamper case this feature exists to catch. `runx self update` follows the same policy.

## Verifying a Release Manually

If you download a binary directly from [GitHub Releases](https://github.com/aryankahar31/runx/releases) instead of using the install script, you can verify it manually.

### Linux / macOS

```bash
# Download the archive and the SHA256SUMS file
curl -fsSLO https://github.com/aryankahar31/runx/releases/latest/download/runx-linux-x64.tar.gz
curl -fsSLO https://github.com/aryankahar31/runx/releases/latest/download/SHA256SUMS

# Verify (prints OK if the checksum matches)
sha256sum -c SHA256SUMS --ignore-missing
# or on macOS:
shasum -a 256 -c SHA256SUMS --ignore-missing
```

### Windows PowerShell

```powershell
# Download the archive and the per-file checksum
Invoke-WebRequest -Uri https://github.com/aryankahar31/runx/releases/latest/download/runx-windows-x64.zip -OutFile runx-windows-x64.zip
Invoke-WebRequest -Uri https://github.com/aryankahar31/runx/releases/latest/download/runx-windows-x64.zip.sha256 -OutFile runx-windows-x64.zip.sha256

# Compare
$expected = (Get-Content .\runx-windows-x64.zip.sha256).Split(' ')[0]
$computed = (Get-FileHash .\runx-windows-x64.zip -Algorithm SHA256).Hash
if ($expected -ieq $computed) { Write-Host "OK" } else { Write-Error "MISMATCH" }
```

## Deno caveat

Deno releases from v2.0.1 publish a per-asset `.sha256sum` sidecar that runx verifies exactly like Node, Bun and Go. Older Deno releases (the 1.x line and v2.0.0) publish **no archive checksum**, so runx installs them with TLS-only verification and prints a warning at install time.
