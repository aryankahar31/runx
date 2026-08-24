# Zero-Config Detection

Runx works **without a `runx.toml`** if standard version files are already present in your project. This page documents exactly what is detected, in what priority order, and what is *not* inferred.

## How it works

When you run `runx dev` and no `runx.toml` is found, runx automatically scans the project directory for well-known ecosystem files and infers the runtime versions from them.

If a `runx.toml` *does* exist it is **always used exclusively** — explicit configuration always wins over auto-detection, with no merging. See [Configuration Precedence](#configuration-precedence) at the bottom of this page.

## Detected files and priority order

### Node.js (first match wins)

| Priority | File | Notes |
|----------|------|-------|
| 1 | `.nvmrc` | Plain text, leading `v` stripped |
| 2 | `.node-version` | Plain text, leading `v` stripped |
| 3 | `package.json` → `engines.node` | JSON, range resolved ([runtime resolution](runtime-resolution.md)) |

### Python (first match wins)

| Priority | File | Notes |
|----------|------|-------|
| 1 | `.python-version` | Plain text, leading `v` stripped |
| 2 | `pyproject.toml` → `[project].requires-python` | TOML, range resolved |
| 3 | `Pipfile` → `[requires].python_version` | TOML; a bare minor like `3.11` is an X-range resolving to the newest 3.11.x |

### Bun (first match wins)

| Priority | File | Notes |
|----------|------|-------|
| 1 | `.bun-version` | Plain text, leading `v` stripped |
| 2 | `package.json` → `engines.bun` | JSON, range resolved |
| 3 | `package.json` → `packageManager` | `bun@1.1.0` form; the `+sha512.…` digest is ignored |
| 4 | `bun.lock`, `bun.lockb`, `bunfig.toml` | Authoritative Bun markers with no version — resolves to the newest release |

A generic `package.json` is **not** a Bun indicator: Node projects contain one without using Bun. Only the files above mark a project as Bun-managed.

### Go (first match wins)

| Priority | File | Notes |
|----------|------|-------|
| 1 | `.go-version` | Plain text (mise/asdf convention) |
| 2 | `go.mod` → `go` directive | Plain text; the `toolchain` directive is ignored |

### Deno

Detected from `.dvmrc` / `.deno-version`, or an explicit entry in `runx.toml`.

### Project-root markers

A detected version file also marks a project root for auto-detection, alongside `runx.toml`: `.nvmrc`, `.node-version`, `.python-version`, `.go-version`, `.bun-version`, `.dvmrc`, `.deno-version`, `package.json`, `pyproject.toml`, `Pipfile`, `go.mod`, `bun.lock`, `bun.lockb` and `bunfig.toml`.

## Multiple runtimes per project

Detection is a **collection**, not a single winner. A project may require several runtimes at once — say Python for its tooling and Bun for its front end — and runx resolves, installs and caches every one of them, then builds **one isolated environment** containing all of their `bin` directories. Anything the project's command spawns — even `npm run dev` internally calling `bun run …` — finds every runtime on its PATH.

```
No runx.toml found — detected from project files:
  python 3.13 (from .python-version)
  bun (from bun.lock)
Resolved python `3.13` to 3.13.15
Resolved bun `*` to 1.3.14
Installing python 3.13.15
Installing bun 1.3.14
✓ Checksums verified
Running `bun run dev`
```

The same works from `runx.toml` — just list both runtimes:

```toml
[runtimes]
python = "3.13"
bun = "1.3.14"

[run]
dev = "npm run dev"
build = "npm run build"
```

## Run-command inference

Every script in `package.json` becomes a run command keyed by its script name: `npm run <script>` for npm-style projects, `bun run <script>` for Bun-managed ones (see the Bun table above). That means `runx dev`, but also `runx test`, `runx build`, `runx lint` — whatever your package.json declares — work without any runx.toml. No other commands are guessed: Go, Python and Rust-only projects have no inferable command and get a clear error listing what was detected plus a pointer to `runx init`.

An explicit `[run]` table in `runx.toml` always replaces inference entirely (see [Configuration Precedence](#configuration-precedence)).

## Example output

With only a `.nvmrc` pinning `v20.11.0` and a `package.json` that has a `dev` script:

```
No runx.toml found — detected from project files:
  node 20.11.0 (from .nvmrc)
Installing node 20.11.0
Downloading https://nodejs.org/dist/v20.11.0/node-v20.11.0-linux-x64.tar.xz
✓ Checksum verified
Extracting to /home/user/.runx/runtimes/node/.staging-20.11.0-4127-...
Running `npm run dev`
```

On subsequent runs the cached runtime is reused:

```
No runx.toml found — detected from project files:
  node 20.11.0 (from .nvmrc)
Using cached node 20.11.0 at /home/user/.runx/runtimes/node/20.11.0
Running `npm run dev`
```

When the project declares a **range** — say `"engines": { "node": ">=20" }` — runx reports which concrete release it picked:

```
No runx.toml found — detected from project files:
  node >=20 (from package.json (engines.node))
Resolved node `>=20` to 22.11.0
Installing node 22.11.0
```

Pin the result with `runx lock` if you need that choice to stay fixed ([lockfiles](lockfile.md)).

## Configuration Precedence

- If `runx.toml` exists → it is the sole source of truth. Auto-detection is never consulted, and the file is never modified.
- Auto-detection is the fallback *only* when no `runx.toml` is present.
- Auto-detection **never writes to disk**. To persist a detected configuration, run `runx init` which creates a starter `runx.toml`.
