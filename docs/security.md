# Security Model

## Summary of hardening guarantees

- **SHA-256 verification** before extraction using published checksums, except for older Deno releases described below
- **Locked execution verifies against `runx.lock` itself**: `--locked` requires a platform artifact digest, authenticates the retained archive, and checks the installed runtime tree before execution
- **Sigstore/cosign signatures** on runx's own release archives and `SHA256SUMS`; installers and self-update verify the archive signature, not vendor runtime signatures
- **Staged installs**: extraction and locked verification finish before publication. Interrupted installs can leave staging files; replacement of an existing runtime is not crash-atomic
- **Strict version validation at a single chokepoint** (`src/runtime.rs::resolve_runtime`) — version strings become filesystem paths and download URLs, so path traversal is rejected before anything else happens
- **Archive extraction hardening** — symlink escape rejected, exec bits preserved
- **Diagnostic integrity checking**: `runx doctor --verify` compares entry-point bytes with a local receipt when available. This is not full-tree authentication, and unavailable Python/Go metadata can leave entries unverifiable even with a healthy overall result
- **Fail-closed installers** — no checksum tool available means no install

## Locked execution

Use `runx run check --locked` (equivalently `runx run --locked check`). A command key is required. Every declared runtime must have a SHA-256 artifact digest for the current platform in `runx.lock`. Missing, malformed, unsupported, or all-zero digests cannot authorize execution.

New installs retain the downloaded archive as `.runx-archive`. Before locked cache reuse, runx requires a readable receipt with matching runtime identity and archive digest, hashes the retained archive against the lockfile, then compares the installed tree with a fresh normalized extraction of that archive. File contents, symlink targets, file types, added/missing entries, and Unix file modes are checked; runx's own bookkeeping files are excluded from the tree comparison. The receipt alone is never proof that the installed bytes are authentic. Cold downloads are checked before extraction, and the same locked-cache verification runs before the staging directory is published.

Any verification failure aborts without running the project command or falling back to another cached runtime. Legacy caches without a retained archive or required digests are rejected under `--locked`, not silently adopted. Remove the affected cache entry and reinstall from a trusted lockfile. Missing platform artifacts must be recorded with `runx lock` on that platform before using `--locked`.

This retains an additional archive and re-extracts/hashes on each locked run. The runtime tree must remain immutable: installing global packages or adding generated files inside it invalidates locked reuse. Unlocked execution retains its existing, weaker cache behavior. The lockfile must be trusted; these checks do not sandbox the child or defend against another process modifying files concurrently between verification and execution.

Verification itself needs no network. However, Python/Go provisioning currently needs fresh asset metadata even for an installed exact pin. `--offline` can therefore refuse those runs when metadata is missing or expired. It blocks runx's own network operations, not child-process networking.

## Install-script verification

Both install scripts (`install.sh` / `install.ps1`) verify the downloaded archive against its published SHA-256 before extraction. They fail closed by default if verification is unavailable. The Unix script uses `sha256sum`, `shasum`, or `openssl`; PowerShell uses `Get-FileHash`. Only the Unix script supports `RUNX_SKIP_CHECKSUM=1`, which also bypasses its signature block when checksum tools are absent; do not combine it with strict signature requirements.

Checksums confirm the download is intact and matches what the publisher listed. They are fetched from the same origin as the artifact, so they are not by themselves protection against a compromised release host.

### Signature verification (Sigstore/cosign)

Since v0.4.2 runx release archives and the `SHA256SUMS` manifest are signed with Sigstore/cosign keyless signing. When cosign is installed, the install scripts and `runx self update` verify the archive signature against `aryankahar31/runx`'s `release.yml` identity on a numeric release tag or `main`. This checks workflow provenance, not that the signature belongs to the exact requested tag. It does not verify Node, Python, Bun, Go, or Deno signatures.

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

> **Note:** Windows ARM64 (`runx-windows-arm64.zip`) is **not currently published**.
> The Sigstore/cosign signing infrastructure does not yet support the Windows ARM64
> runner (`windows-11-arm`). The x64 binary (`runx-windows-x64.zip`) runs on ARM64
> Windows via x64 emulation. This will be revisited when cosign supports that runner.

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
