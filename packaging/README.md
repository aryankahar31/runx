# Packaging templates

These files are **release-ready templates**, not published packages. The
release workflow renders them with the tagged version and each archive's
SHA-256 (`packaging/render.sh`) and uploads the result as the
`packaging-rendered` artifact. Submitting them is a manual step, tracked in
[`../LAUNCH_CHECKLIST.md`](../LAUNCH_CHECKLIST.md).

| Channel | Template | Publish target |
| --- | --- | --- |
| Homebrew | `homebrew/runx.rb` | a `homebrew-runx` tap repo (`Formula/runx.rb`) |
| winget | `winget/aryankahar31.runx.yaml` | `microsoft/winget-pkgs` PR |
| Scoop | `scoop/runx.json` | a Scoop bucket repo |
| AUR | `aur/PKGBUILD` | AUR `runx-bin` |
| Nix | `nix/flake.nix`, `nix/package.nix` | your flake / nixpkgs PR |
| npm | `npm/` | `npm publish` |

## Local checks

```bash
packaging/validate.sh                       # structure and placeholders
packaging/render.sh 0.7.0 dist /tmp/runx-pkg # substitute version + digests
```

The npm wrapper's unit tests need Node 18+:

```bash
cd packaging/npm && npm test
```

## Nix hashes

`nix/package.nix` ships with `__SOURCE_HASH__` and `__CARGO_HASH__`
placeholders because they depend on the tagged source tree, not on a release
archive. Run `nix build` once per release and paste the two hashes Nix prints
in the mismatch error. The renderer deliberately does not guess them.
