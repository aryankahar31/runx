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
| Homebrew | Available now | `homebrew/runx-launcher.rb`; published in `aryankahar31/homebrew-tap` (`aryankahar31/tap/runx-launcher`) |
| Winget | Planned / pending submission | Three `aryankahar31.runx-launcher` manifests; `microsoft/winget-pkgs` PR |
| Scoop | Available now | `scoop/runx-launcher.json`; published in `aryankahar31/scoop-bucket` |
| AUR | Planned / pending submission | `aur/PKGBUILD`; AUR `runx-launcher-bin` (blocked: Arch registration temporarily closed) |
| Nix | Planned / pending submission | `nix/flake.nix`, `nix/package.nix`; package `runx-launcher` |
| npm | Planned / pending submission | Package `runx-launcher`; exposes command `runx` |

## Local checks

```bash
packaging/validate.sh                       # structure, placeholders, npm syntax, renderer regression test
packaging/render.sh 0.7.1 dist /tmp/runx-pkg # substitute version + digests
```

## Updating the Homebrew tap on each release

After a new GitHub release `v<VERSION>` finishes building its archives, re-render the formula and update [`aryankahar31/homebrew-tap`](https://github.com/aryankahar31/homebrew-tap):

```bash
VERSION=0.7.1 # replace with the new version (without leading "v")
rm -rf "/tmp/runx-v${VERSION}-dist" "/tmp/runx-v${VERSION}-rendered"
mkdir -p "/tmp/runx-v${VERSION}-dist"

# 1. Download the release archives and re-render all manifests (version + SHA-256 digests)
gh release download "v${VERSION}" \
  --repo aryankahar31/runx \
  --pattern 'runx-*.tar.gz' \
  --pattern 'runx-windows-x64.zip' \
  --clobber \
  --dir "/tmp/runx-v${VERSION}-dist"
packaging/render.sh "${VERSION}" "/tmp/runx-v${VERSION}-dist" "/tmp/runx-v${VERSION}-rendered"

# 2. Copy the rendered formula into aryankahar31/homebrew-tap, audit, and push
git clone https://github.com/aryankahar31/homebrew-tap.git /tmp/homebrew-tap
cp "/tmp/runx-v${VERSION}-rendered/homebrew/runx-launcher.rb" /tmp/homebrew-tap/Formula/runx-launcher.rb
cd /tmp/homebrew-tap
git add Formula/runx-launcher.rb
git commit -m "chore(formula): bump runx-launcher to v${VERSION}"
git push origin main

# 3. Verify on macOS
brew update
brew reinstall aryankahar31/tap/runx-launcher
brew test runx-launcher
brew audit --strict --new aryankahar31/tap/runx-launcher
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
