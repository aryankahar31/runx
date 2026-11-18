# v0.7.1 release checklist

1. Merge the `fix/crates-name` PR only after required CI is green on Linux,
   macOS, and Windows.

   - [Open pull requests](https://github.com/aryankahar31/runx/pulls)
   - [CI workflow](https://github.com/aryankahar31/runx/actions/workflows/ci.yml)
   - `gh pr checks <PR_NUMBER> --watch`
   - `gh pr merge <PR_NUMBER> --squash --delete-branch`

2. From the merged main HEAD, wait for its CI push run to finish successfully.

   - `git switch main`
   - `git pull --ff-only origin main`
   - `gh run list --branch main --limit 3`
   - `gh run watch <CI_RUN_ID> --exit-status`

3. Tag that exact main commit only after the CI run succeeds.

   - `git tag v0.7.1`
   - `git push origin v0.7.1`
   - `gh run list --workflow release.yml --limit 5`
   - `gh run watch <RELEASE_RUN_ID> --exit-status`

   If only the CI-gate job fails because the main CI run was still finishing,
   confirm that CI run is green, then run
   `gh run rerun <RELEASE_RUN_ID> --failed`. Do not move or delete the tag.

4. Verify the release assets and set the release body from
   `RELEASE_NOTES_v0.7.1.md`.

   - [GitHub release](https://github.com/aryankahar31/runx/releases/tag/v0.7.1)
   - Expected: all five `runx-*` platform archives, each `.sha256` and
     `.sigstore.json`, plus `SHA256SUMS` and `SHA256SUMS.sigstore.json`.
   - `gh release view v0.7.1 --json assets,isDraft,isPrerelease`
   - `gh release download v0.7.1 --pattern SHA256SUMS --pattern runx-macos-arm64.tar.gz --dir /tmp/runx-v0.7.1-assets`
   - `cd /tmp/runx-v0.7.1-assets && shasum -a 256 -c SHA256SUMS --ignore-missing`
   - Verify the archive and manifest bundles with `cosign verify-blob`, using
     the `release.yml` tag identity and token.actions.githubusercontent.com issuer.
   - `gh release edit v0.7.1 --notes-file RELEASE_NOTES_v0.7.1.md`

5. After the release exists, publish the Cargo package. Check credentials
   without printing their contents.

   - `test -s ~/.cargo/credentials.toml`
   - `cargo publish`
   - `cargo install runx-launcher --force --root /tmp/runx-from-crates`
   - `/tmp/runx-from-crates/bin/runx --version`

   The package name is `runx-launcher`; the installed command and Rust library
   crate are `runx`. Do not label Cargo as available before publication succeeds.

6. Verify the release installer in an isolated cache and install directory.

   - `RUNX_HOME=/tmp/runx-home-test RUNX_INSTALL_DIR=/tmp/runx-home-test/bin RUNX_VERSION=0.7.1 sh install.sh`
   - `/tmp/runx-home-test/bin/runx --version`
   - `RUNX_HOME=/tmp/runx-home-test /tmp/runx-home-test/bin/runx doctor`

7. Prepare, but do not publish, the npm wrapper.

   - `cd packaging/npm && npm test`
   - `cd packaging/npm && npm publish --dry-run`

   Its package name is `runx-launcher`; it exposes the `runx` command.

8. Submit the remaining package-manager templates manually, in order.

   - **Homebrew:** create the `aryankahar31/homebrew-runx` tap, copy
     `packaging-rendered/homebrew/runx-launcher.rb` to
     `Formula/runx-launcher.rb`, then run
     `brew audit --new --formula Formula/runx-launcher.rb`.
   - **Winget:** submit the three rendered `aryankahar31.runx-launcher`
     manifests to [microsoft/winget-pkgs](https://github.com/microsoft/winget-pkgs/pulls).
     Validate each with `winget validate --manifest <manifest.yaml>` first.
   - **Scoop:** copy `packaging-rendered/scoop/runx-launcher.json` into the
     bucket as `bucket/runx-launcher.json`; verify with
     `scoop install runx-launcher`.
   - **AUR:** submit `packaging-rendered/aur/PKGBUILD` as `runx-launcher-bin`;
     regenerate `.SRCINFO` with `makepkg --printsrcinfo > .SRCINFO`, then run
     `makepkg --cleanbuild`.
   - **Nix:** in `packaging-rendered/nix`, fill the source and Cargo hash
     placeholders by running `nix build .#default` until it succeeds, then
     submit the package to the chosen Nix target.

9. Enable GitHub Discussions.

   - [Repository settings](https://github.com/aryankahar31/runx/settings) →
     Features → Discussions.

10. Update and deploy the website for v0.7.1.

    - [Vercel dashboard](https://vercel.com/dashboard)
    - [Live site](https://runx-cli.vercel.app/)

11. Publish the launch posts after the release and package links are live.

    - [Hacker News](https://news.ycombinator.com/submit) — `launch/show-hn.md`
    - [Reddit](https://www.reddit.com/submit) — `launch/reddit.md`
    - [LinkedIn](https://www.linkedin.com/feed/) — adapt the launch summary and
      include the release and crates.io links.
