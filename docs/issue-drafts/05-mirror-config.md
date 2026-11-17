# Allow per-runtime download mirrors without weakening verification

**Labels:** enhancement, security-sensitive

## Problem

Teams behind a proxy can use the global `HTTP_PROXY`, `HTTPS_PROXY`, or
`ALL_PROXY` settings, and users can pre-populate `RUNX_HOME`. However, runx has
no per-runtime mirror setting for choosing an alternate vendor host when a
specific CDN is blocked or slow.

## Proposal

A config/env setting maps a runtime to a mirror base URL. runx still verifies
against the vendor's published digest, or the lockfile digest under `--locked`.
Mirrors never bypass checksum validation and never become the source of truth
for version lists unless explicitly configured.

## Acceptance

- Design doc first, including the trust model.
- A mirror that serves mutated bytes fails installation.
- Tests use a local HTTP server, never a public mirror.
