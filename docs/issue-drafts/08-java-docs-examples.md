# Add Java examples to the runtime docs

**Labels:** documentation, good first issue

## Problem

Java support is implemented and tested at the resolver level, but the docs show
only `.java-version` and `.sdkmanrc`. Users coming from Maven/Gradle projects
need a copy-pasteable example and an explicit statement of the Java 9+ limit.

## Proposal

In `docs/zero-config.md` and `docs/runtime-resolution.md`:

- A minimal `runx.toml` pinning Temurin 21.
- Both `.java-version` and `.sdkmanrc` examples, including the `java=` form.
- A note that Java 8 is not supported, with the reason.
- The checksum source (Adoptium metadata) named.

## Acceptance

- Every command shown works as written.
- No claim that Maven/Gradle are managed by runx.
