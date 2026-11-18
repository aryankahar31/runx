# Ship shell completions in packaged installs

**Labels:** enhancement, packaging, good first issue

## Problem

`runx completions <shell>` exists, but Homebrew/AUR/npm users have to run it
manually and wire it into their shell. That is friction, and completions are the
main discoverability aid for project keys.

## Proposal

Install completion files where the package manager expects them:

- Homebrew: `bash_completion`, `zsh/site-functions`, `fish/vendor_completions.d`.
- AUR: same paths under `/usr/share`.
- npm: document a post-install command rather than editing shell files silently.

## Acceptance

- `packaging/validate.sh` checks the files are generated and non-empty.
- No installer silently edits a shell rc file.
- README lists the one command for each channel.
