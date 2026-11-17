# Website

Source of truth for <https://runx-cli.vercel.app/>. Static HTML, CSS and vanilla
JS with a small build step, no framework, and no runtime dependencies.

## Structure

| Path | Purpose |
| :-- | :-- |
| `website/index.html` | The whole landing page |
| `website/style.css` | Design tokens, light/dark themes, layout, motion |
| `website/script.js` | Theme, demo terminal, detection picker, CLI filter, palette, live GitHub data |
| `website/404.html` | Not-found page |
| `website/assets/fonts/` | Self-hosted Inter + JetBrains Mono (Latin woff2, OFL licence) |
| `website/og.png` | 1200×630 Open Graph image (regenerate with Chrome headless if the copy changes) |
| `website/scripts/build-data.mjs` | Fetches release version and stars from GitHub into `data.json` |
| `website/vercel.json` | Cache + security headers |
| `website/scripts/check.mjs` | Local links, anchors, required meta and honesty checks |

## Updating content

- **Version and stars** are fetched during the Vercel build and refreshed
  client-side from the GitHub API, cached for 10 minutes. Never hardcode them.
  If either request fails, the page keeps the build-time value or shows the
  "latest release" / "—" fallback.
- **CLI reference** is hand-written from real `runx --help` output. Re-check the
  commands and descriptions against the released binary when the CLI changes.
- **Benchmarks** must match `benchmarks/results.json` and
  `benchmarks/README.md`. Every number links to the methodology and raw output.
- **Comparison** claims live in `website/index.html` and must stay aligned with
  `docs/comparison.md`. Keep the "Where runx is weaker" list honest.
- **Terminal demo** is explicitly labelled illustrative. Do not imply it is a
  captured session unless you replace it with one.

## Checks

```sh
node --check website/script.js
node --check website/scripts/build-data.mjs
node website/scripts/build-data.mjs
node website/scripts/check.mjs
```

The `website` GitHub Actions workflow runs the same check on every push and PR
that touches `website/`.

## Deploy

The Vercel project `runx-cli` is connected to this repository with **Root
Directory = `website`**. Its build command is `node scripts/build-data.mjs` and
its output directory is `.`. Pushes to `main` deploy production; every PR gets
a preview. No `vercel` CLI deploy is needed for normal changes.

If the project is ever recreated: set the same root directory, framework
preset "Other", build command `node scripts/build-data.mjs`, output directory
`.`, and production branch `main`.
