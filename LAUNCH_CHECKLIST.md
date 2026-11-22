# v0.7.2 remaining launch steps

The GitHub release and crates.io publication are complete. The package is
[`runx-launcher`](https://crates.io/crates/runx-launcher); its command is
`runx`. The release is at
https://github.com/aryankahar31/runx/releases/tag/v0.7.2.

The remaining items below are manual. npm and Homebrew are published for
0.7.2; Winget awaits review ([microsoft/winget-pkgs#483384](https://github.com/microsoft/winget-pkgs/pull/483384)), Scoop is pending its v0.7.2 update, AUR is blocked on Arch's registration freeze, and
Nix is still unfinished. No social post has been made. The website copy is
updated and live at
https://runx-cli.vercel.app/ (website workflow run `44303757173` succeeded).

## 1. Render the release manifests

Run from the `runx` repository after `gh auth login`. Use a fresh empty asset
directory:

```sh
mkdir -p /tmp/runx-v0.7.2-dist
gh release download v0.7.2 \
  --pattern 'runx-*.tar.gz' \
  --pattern runx-windows-x64.zip \
  --clobber \
  --dir /tmp/runx-v0.7.2-dist
packaging/render.sh 0.7.2 /tmp/runx-v0.7.2-dist /tmp/runx-v0.7.2-rendered
```

These files are templates for review and submission. Nix source and Cargo
hashes remain placeholders until its build step below succeeds.

## 2. Publish the npm wrapper (done)

```sh
cd packaging/npm
npm test
cd /tmp/runx-v0.7.2-rendered/npm
npm publish --access public
npm view runx-launcher@0.7.2 version
```

Published as `runx-launcher@0.7.2`; the commands above are the record of how.

## 3. Submit the Homebrew formula (done)

Published in [`aryankahar31/homebrew-tap`](https://github.com/aryankahar31/homebrew-tap) (`brew install aryankahar31/tap/runx-launcher`, updated to `0.7.2`).

Create the `homebrew-tap` repository at
[github.com/new](https://github.com/new), then run:

```sh
git clone https://github.com/aryankahar31/homebrew-tap.git
cd homebrew-tap
mkdir -p Formula
cp /tmp/runx-v0.7.2-rendered/homebrew/runx-launcher.rb Formula/runx-launcher.rb
git add Formula/runx-launcher.rb
git commit -m "chore(formula): bump runx-launcher to v0.7.2"
git push origin main
brew tap aryankahar31/tap
brew install aryankahar31/tap/runx-launcher
runx --version
brew test runx-launcher
brew audit --strict --new aryankahar31/tap/runx-launcher
```

## 4. Submit the Winget manifests (pending)

Validate all three manifests manually, then open a PR to
[microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs/pulls):

```powershell
cd /tmp/runx-v0.7.2-rendered/winget
winget validate --manifest aryankahar31.runx-launcher.yaml
winget validate --manifest aryankahar31.runx-launcher.locale.en-US.yaml
winget validate --manifest aryankahar31.runx-launcher.installer.yaml
```

The `winget validate` schema check is still manual; do not submit until all
three commands pass. The v0.7.1 submission
[microsoft/winget-pkgs#483384](https://github.com/microsoft/winget-pkgs/pull/483384)
is awaiting moderator review.

## 5. Submit the Scoop manifest (pending for v0.7.2)

Create the public `scoop-bucket` repository at
[github.com/new](https://github.com/new), then run:

```sh
git clone https://github.com/aryankahar31/scoop-bucket.git
cd scoop-bucket
# The manifest lives in the repository root, not a bucket/ subdirectory.
cp /tmp/runx-v0.7.2-rendered/scoop/runx-launcher.json .
git add runx-launcher.json
git commit -m "Add runx-launcher 0.7.2"
git push origin main
scoop bucket add aryankahar31 https://github.com/aryankahar31/scoop-bucket
scoop install runx-launcher
runx --version
```

## 6. Submit the AUR package

Run on Arch Linux after configuring your AUR SSH key:

```sh
mkdir runx-launcher-bin
cd runx-launcher-bin
cp /tmp/runx-v0.7.1-rendered/aur/PKGBUILD ./PKGBUILD
makepkg --printsrcinfo > .SRCINFO
makepkg --cleanbuild
git init -b master
git add PKGBUILD .SRCINFO
git commit -m "Add runx-launcher-bin 0.7.1"
git remote add origin ssh://aur@aur.archlinux.org/runx-launcher-bin.git
git push -u origin master
```

## 7. Finish and submit the Nix package

Nix stays marked "pending" in the docs. Both `__SOURCE_HASH__` and
`__CARGO_HASH__` in `package.nix` are still placeholders, so the package does
not build until they are resolved; do not publish it before that.

Replace `__SOURCE_HASH__` and `__CARGO_HASH__` in `package.nix` with the values
from the successive hash-mismatch outputs. Repeat `nix build` until it passes,
then send the package to the intended Nix channel (for example,
[Nixpkgs](https://github.com/NixOS/nixpkgs/pulls)).

```sh
cd /tmp/runx-v0.7.1-rendered/nix
nix build .#default
nix run .#default -- --version
```

## 8. Enable GitHub Discussions

Open [repository settings](https://github.com/aryankahar31/runx/settings), then
select **Features → Discussions**.

## 9. Publish the launch posts

Post after the relevant package links work:

- [Hacker News](https://news.ycombinator.com/submit) — `launch/show-hn.md`
- [Reddit](https://www.reddit.com/submit) — `launch/reddit.md`
- [LinkedIn](https://www.linkedin.com/feed/) — adapt the launch summary and
  include the release and crates.io links.
