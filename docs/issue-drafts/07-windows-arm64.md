# Publish Windows ARM64 artifacts when signing supports it

**Labels:** packaging, blocked

## Problem

`runx-windows-x64.zip` runs on Windows ARM64 through x64 emulation, but there is
no native artifact. The blocker is documented in `docs/security.md`: the
cosign/Sigstore setup does not support the `windows-11-arm` runner yet.

## Proposal

Native `runx-windows-arm64.zip`, signed and included in `SHA256SUMS`, once the
signing path works on that runner. Do not publish an unsigned ARM64 artifact to
close the issue: an unsigned channel would quietly weaken the release policy.

## Acceptance

- Release workflow builds and signs on an ARM64 Windows runner.
- Install script selects the archive by architecture.
- A release dry run verifies the signature with cosign.
