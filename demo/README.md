# Demo recording

`runx.tape` is a [VHS](https://github.com/charmbracelet/vhs) script that records
the real flow: detect a project from `.nvmrc` and `package.json`, generate `runx.toml`, install the
runtime on first run, then reuse the cache on the second run.

```sh
brew install vhs        # or: go install github.com/charmbracelet/vhs@latest
vhs demo/runx.tape      # writes demo/runx.gif
```

Network access is required because the first `runx dev` downloads Node. The
recording is intentionally not committed: it is regenerated from the released
binary so the demo cannot drift from what the code does.
