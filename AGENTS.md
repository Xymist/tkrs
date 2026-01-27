# AGENTS.md

This file provides guidance to LLMs when working with code in this repository.

See @README.md for usage documentation. Run `tk help` for command reference. Always update the README.md usage content when adding/changing commands and flags.

## Architecture

Single-file bash implementation (~900 lines). Uses awk for performant bulk operations on large ticket sets.

Key functions:

- `generate_id()` - Creates IDs from directory name prefix + timestamp hash
- `ticket_path()` - Resolves partial IDs to full file paths
- `yaml_field()` / `update_yaml_field()` - YAML frontmatter manipulation via sed
- `cmd_*()` - Command handlers
- `cmd_ready()`, `cmd_blocked()`, `cmd_ls()` - awk-based bulk listing with sorting

Dependencies: bash, sed, awk, find. Optional: ripgrep (faster grep), jq (for query command).

## Changelog

When committing notable changes to the `ticket` script (new commands, flags, bug fixes, behavior changes), update CHANGELOG.md in the same commit:

- Create `## [Unreleased]` section at top if it doesn't exist
- Add bullet points under appropriate heading (Added, Fixed, Changed, Removed)
- Only script changes need logging; docs/workflow changes don't

## Releases & Packaging

Before tagging a release:

1. Ensure CHANGELOG.md has a section for the new version with release date
2. Update "Unreleased" to the version number and today's date
3. Commit the changelog update as part of the release

```bash
# Example release flow
# 1. Update CHANGELOG.md: change "## [0.3.0] - Unreleased" to "## [0.3.0] - 2026-01-15"
# 2. Commit and tag
jj commit -m "release: v0.3.0"
git tag v0.3.0
jj git push && git push origin v0.3.0
```

The GitHub Actions workflow (`.github/workflows/release.yml`) automatically:

1. Extracts the changelog section for this version as the release body
2. Updates the Homebrew formula in `wedow/homebrew-tools` tap
3. Updates the AUR package (builds `.SRCINFO` via Docker)

Both package managers install the script as `tk` in the user's PATH.
