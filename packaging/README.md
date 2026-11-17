# Packaging templates

These files are **submission templates**, not published packages. The
release workflow renders them with the tagged version and each archive's
SHA-256 (`packaging/render.sh`) and uploads the result as the
`packaging-rendered` artifact. Rendering is not package validation or
submission: Winget needs companion version/default-locale manifests and schema
validation, Nix needs source and Cargo hashes, and each downstream repository
must accept its own submission. These are manual steps, tracked in
[`../LAUNCH_CHECKLIST.md`](../LAUNCH_CHECKLIST.md).

| Channel | Template | Publish target |
| --- | --- | --- |
| Cargo / cargo-binstall | `Cargo.toml` package metadata | crates.io release; binstall uses GitHub release assets |
| Homebrew | `homebrew/runx.rb` | a `homebrew-runx` tap repo (`Formula/runx.rb`) |
| Winget | `winget/aryankahar31.runx.yaml` (draft installer manifest only) | `microsoft/winget-pkgs` PR with version and default-locale manifests |
| Scoop | `scoop/runx.json` | a Scoop bucket repo |
| AUR | `aur/PKGBUILD` | AUR `runx-bin` |
| Nix | `nix/flake.nix`, `nix/package.nix` | your flake / nixpkgs PR |
| npm | `npm/` | `npm publish` |

## Local checks

```bash
packaging/validate.sh                       # structure, version placeholders, npm syntax
packaging/render.sh 0.7.0 dist /tmp/runx-pkg # substitute version + digests
```

The validator does not validate Winget schemas or package-manager submissions.
The Winget draft currently declares schema 1.6.0 and has not been validated
against the current submission repository guidance.

The npm wrapper's unit tests need Node 18+:

```bash
cd packaging/npm && npm test
```

## Nix hashes

`nix/package.nix` ships with `__SOURCE_HASH__` and `__CARGO_HASH__`
placeholders because they depend on the tagged source tree, not on a release
archive. Run `nix build` once per release and paste the two hashes Nix prints
in the mismatch error. The renderer deliberately does not guess them. Before
submission, also correct `meta.platforms`: the package currently declares
Windows, while `flake.nix` lists only Linux and macOS targets.
