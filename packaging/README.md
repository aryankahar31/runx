# Packaging templates

These files are **submission templates**, not published packages. The
release workflow renders them with the tagged version and each archive's
SHA-256 (`packaging/render.sh`) and uploads the result as the
`packaging-rendered` artifact. Rendering is not package validation or
submission: Winget's three manifests still need manual schema validation, Nix
needs source and Cargo hashes, and each downstream repository must accept its
own submission. These are manual steps, tracked in
[`../LAUNCH_CHECKLIST.md`](../LAUNCH_CHECKLIST.md).

| Channel | v0.7.1 status | Template / publish target |
| --- | --- | --- |
| Cargo / cargo-binstall | Available now | Published package `runx-launcher`; binary and library `runx` |
| Homebrew | Planned / pending submission | `homebrew/runx-launcher.rb`; `homebrew-runx` tap |
| Winget | Planned / pending submission | Three `aryankahar31.runx-launcher` manifests; `microsoft/winget-pkgs` PR |
| Scoop | Planned / pending submission | `scoop/runx-launcher.json`; a Scoop bucket repo |
| AUR | Planned / pending submission | `aur/PKGBUILD`; AUR `runx-launcher-bin` |
| Nix | Planned / pending submission | `nix/flake.nix`, `nix/package.nix`; package `runx-launcher` |
| npm | Planned / pending submission | Package `runx-launcher`; exposes command `runx` |

## Local checks

```bash
packaging/validate.sh                       # structure, placeholders, npm syntax, renderer regression test
packaging/render.sh 0.7.1 dist /tmp/runx-pkg # substitute version + digests
```

The validator tests renderer failure handling but does not validate Winget
schemas or package-manager submissions. Use `wingetcreate` and
`winget validate` manually before submitting the three Winget manifests, as
described in the [launch checklist](../LAUNCH_CHECKLIST.md).

The npm wrapper's unit tests need Node 18+:

```bash
cd packaging/npm && npm test
```

## Nix hashes

`nix/package.nix` keeps `__SOURCE_HASH__` and `__CARGO_HASH__` placeholders
because they depend on the tagged source tree, not on a release archive. Build
once to obtain the source hash, replace it, build again to obtain the Cargo
hash, then build again to verify the package. `meta.platforms` matches the
four Linux/macOS systems in `flake.nix`.
