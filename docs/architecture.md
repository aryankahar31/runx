# Architecture

## Flow

```
runx
  │
  ▼
Parse runx.toml / detect project files
  │
  ▼
Resolve runtime versions (ranges → concrete releases)
  │
  ▼
Check local cache (~/.runx/runtimes/)
  │
  ├─ Cache hit ──────────────┐
  │                          │
  └─ Cache miss              │
       │  Download           │
       │  Verify checksum    │
       │  Extract atomically │
       └─────────────────────┘
  │
  ▼
Build isolated child PATH
  │
  ▼
Execute the project command
```

Step by step:

1. Read `runx.toml` (or auto-detect from project files)
2. Resolve runtime versions
3. Check local cache
4. Download missing runtime
5. Extract portable runtime
6. Build isolated PATH
7. Execute command

## Isolation

Runx never modifies:

- Your global `PATH`
- Shell startup files (`.bashrc`, `.zshrc`, profiles)
- System-installed runtimes
- Anything outside `~/.runx` and your project directory

Every command runs with the cached runtime's `bin` directories **prepended** to `PATH`, so the project's versions take priority over anything installed system-wide. The existing `PATH` is then **appended**, so ordinary tools (`git`, `make`, `curl`, Homebrew) keep working — runx isolates *runtime versions*, not the entire environment.

Because the change is scoped to the child process, **no shell integration is required**: no `eval "$(… init)"` line, no shim directory, no directory hooks. Nothing about your shell changes until you type `runx`.

## Atomic installs

Installs are staged: archives are extracted into `.staging-*` directories inside the runtime's cache path and only renamed to the canonical `<tool>/<version>` directory after verification succeeds, at which point a `.runx-complete.json` receipt is written. A crashed or interrupted install leaves only staging directories behind, which never appear as usable runtimes. Concurrent installs of the same version race safely: the first commit wins and later processes adopt the completed tree instead of failing.

Runtimes installed before receipts existed are adopted on use if their executable works — they are not re-downloaded.
 
 ## Dependency installation isolation
 
 Runx extends the same isolation guarantees to project dependencies:
 
 - **Python:** Every project gets its own `.venv/` directory created by the runx-managed Python. The virtual environment's `pip` installs dependencies into this project-local directory — never into the shared runx-managed Python's `site-packages`. Two projects using Python 3.11.7 get completely independent dependency sets.
 - **Node/Bun/pnpm/Yarn:** Each project's `node_modules/` directory is local to the project. Runx never installs globally.
 - **Go:** `go mod download` writes `go.sum` in the project directory.
 - **Deno:** `deno install` uses the lockfile in the project directory.
 
 This ensures that:
 - Different projects using the same runtime version never share dependencies.
 - Version conflicts between projects are impossible.
 - Removing a project directory removes its dependencies completely.
 
 The dependency installation flow extends the core architecture:
 
 ```
 runx install / runx --install <key>
   │
   ▼
 Detect all dependency managers (all lockfiles)
   │
   ▼
 Resolve JS conflicts (npm wins over pnpm/Yarn/Bun)
   │
   ▼
 Provision required runtimes (shared cache)
   │
   ▼
 For each manager: run install command with managed runtime on PATH
   │
   ▼
 Dependencies written to project-local directories only
 ```
