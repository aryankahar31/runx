# v0.7.0 launch checklist

1. Merge the release PR after CI is green on Linux, macOS, and Windows.

   - [Open pull requests](https://github.com/aryankahar31/runx/pulls)
   - [CI workflow](https://github.com/aryankahar31/runx/actions/workflows/ci.yml)
   - `gh pr checks feat/v0.7.0 --watch`
   - `gh pr merge feat/v0.7.0 --merge`

2. Tag the merged `main` commit and push the tag to start the release workflow.

   ```bash
   git switch main
   git pull --ff-only origin main
   git tag -a v0.7.0 -m "runx v0.7.0"
   git push origin v0.7.0
   ```

3. Verify the release assets and replace GitHub's generated notes with the
   draft in `RELEASE_NOTES_v0.7.0.md`.

   - [Release workflow runs](https://github.com/aryankahar31/runx/actions/workflows/release.yml)
   - [GitHub releases](https://github.com/aryankahar31/runx/releases)
   - Expected archives: `runx-linux-x64.tar.gz`, `runx-linux-arm64.tar.gz`,
     `runx-macos-x64.tar.gz`, `runx-macos-arm64.tar.gz`, and
     `runx-windows-x64.zip`, each with a `.sha256` and `.sigstore.json` file;
     also expect `SHA256SUMS` and `SHA256SUMS.sigstore.json`.
   - `gh release view v0.7.0 --repo aryankahar31/runx`
   - `gh release download v0.7.0 --repo aryankahar31/runx --dir /tmp/runx-v0.7.0-assets`
   - `cd /tmp/runx-v0.7.0-assets && shasum -a 256 -c SHA256SUMS`
   - Verify the manifest and each archive signature:

     ```bash
     cosign verify-blob SHA256SUMS --bundle SHA256SUMS.sigstore.json \
       --certificate-identity-regexp '^https://github\.com/aryankahar31/runx/\.github/workflows/release\.yml@refs/tags/v0\.7\.0$' \
       --certificate-oidc-issuer https://token.actions.githubusercontent.com
     for asset in runx-*.tar.gz runx-*.zip; do
       cosign verify-blob "$asset" --bundle "$asset.sigstore.json" \
         --certificate-identity-regexp '^https://github\.com/aryankahar31/runx/\.github/workflows/release\.yml@refs/tags/v0\.7\.0$' \
         --certificate-oidc-issuer https://token.actions.githubusercontent.com
     done
     ```
   - `gh release edit v0.7.0 --repo aryankahar31/runx --notes-file RELEASE_NOTES_v0.7.0.md`

4. Publish the crate to crates.io.

   ```bash
   cargo publish --dry-run
   cargo publish
   ```

   - [runx on crates.io](https://crates.io/crates/runx)

5. Publish the npm wrapper after the `RUNX_VERSION` path validation issue is
   fixed and the wrapper tests pass.

   - Download the `packaging-rendered` artifact from the successful release
     workflow run: [release workflow runs](https://github.com/aryankahar31/runx/actions/workflows/release.yml)
   - `gh run download --name packaging-rendered --dir /tmp/runx-packaging`
   - `cd /tmp/runx-packaging/npm`
   - `npm pack --dry-run`
   - `npm publish --access public`
   - [runx on npm](https://www.npmjs.com/package/runx)

6. Create the Homebrew tap and publish the rendered formula.

   - Create [`homebrew-runx`](https://github.com/new?name=homebrew-runx) or run
     `gh repo create aryankahar31/homebrew-runx --public --description "Homebrew tap for runx"`.
   - `gh repo clone aryankahar31/homebrew-runx`
   - `cd homebrew-runx && mkdir -p Formula`
   - `cp /tmp/runx-packaging/homebrew/runx.rb Formula/runx.rb`
   - `brew audit --new --formula Formula/runx.rb`
   - `git add Formula/runx.rb && git commit -m "Add runx 0.7.0" && git push -u origin main`
   - `brew tap aryankahar31/runx && brew install aryankahar31/runx/runx && runx --version`

7. Submit the Winget, Scoop, AUR, and Nix packages.

   - **Winget:** [Authoring guide](https://github.com/microsoft/winget-pkgs/blob/master/doc/Authoring.md),
     [manifest PRs](https://github.com/microsoft/winget-pkgs/pulls). On Windows run
     `winget install wingetcreate`, then
     `wingetcreate new https://github.com/aryankahar31/runx/releases/download/v0.7.0/runx-windows-x64.zip`
     to create the three-file manifest set, review it against the rendered
     installer draft, and on Windows run
     `winget validate --manifest .\manifests\a\aryankahar31\runx\0.7.0`.
   - **Scoop:** Create [`scoop-runx`](https://github.com/new?name=scoop-runx) or run
     `gh repo create aryankahar31/scoop-runx --public --description "Scoop bucket for runx"`.
     Then run `gh repo clone aryankahar31/scoop-runx`,
     `cd scoop-runx && mkdir -p bucket`,
     `cp /tmp/runx-packaging/scoop/runx.json bucket/runx.json`, and
     `git add bucket/runx.json && git commit -m "Add runx 0.7.0" && git push -u origin main`.
     Verify with `scoop bucket add runx https://github.com/aryankahar31/scoop-runx && scoop install runx`.
   - **AUR:** [AUR account and package index](https://aur.archlinux.org/).
     `git clone ssh://aur@aur.archlinux.org/runx-bin.git`
     `cd runx-bin && cp /tmp/runx-packaging/aur/PKGBUILD PKGBUILD`
     `makepkg --printsrcinfo > .SRCINFO`
     `makepkg --cleanbuild`
     `git add PKGBUILD .SRCINFO && git commit -m "Update to 0.7.0" && git push`
   - **Nix:** [nixpkgs pull requests](https://github.com/NixOS/nixpkgs/pulls).
     Correct the package's `meta.platforms` to match the four Linux/macOS flake
     systems. In the rendered package directory run
     `cd /tmp/runx-packaging/nix && nix build .#default`; copy the source hash
     from Nix's mismatch error into `__SOURCE_HASH__`, run `nix build .#default`
     again, then copy the cargo hash from its mismatch error into
     `__CARGO_HASH__` and run the build once more. Submit the corrected flake
     and package expression to the chosen Nix target.

8. Enable GitHub Discussions.

   - [Repository settings](https://github.com/aryankahar31/runx/settings) →
     Features → Discussions.

9. Update the website for v0.7.0 and deploy it.

   - [Vercel dashboard](https://vercel.com/dashboard)
   - [Live site](https://runx-cli.vercel.app/)

10. Publish the launch posts after the release and package links are live.

    - [Hacker News submission](https://news.ycombinator.com/submit) — use
      `launch/show-hn.md`.
    - [Reddit submission](https://www.reddit.com/submit) — use
      `launch/reddit.md`.
    - [LinkedIn post composer](https://www.linkedin.com/feed/) — adapt the
      launch summary and include the release link.
