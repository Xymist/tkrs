# AGENTS.md

This file provides guidance to LLMs when working with code in this repository.

See @README.md for usage documentation. Run `tk help` for command reference. Always update the README.md usage content when adding/changing commands and flags.

## Architecture

Key functions:

- `generate_id()` - Creates IDs from directory name prefix + timestamp hash
- `ticket_path()` - Resolves partial IDs to full file paths
- `yaml_field()` / `update_yaml_field()` - YAML frontmatter manipulation
- `cmd_*()` - Command handlers
- `cmd_ready()`, `cmd_blocked()`, `cmd_ls()`

## Workflow

- Find the next open ticket with `cargo run -- ready`
- Implement the changes suggested in the ticket
- Add tests for the changes and run them with `cargo nextest run`
- Document what was changed, appended to the ticket
- Mark the ticket as closed using `cargo run -- close <ticket_id>`
- Use `jj` to commit with the message formatted as `<ticket-number>: <short-description>`

## Changelog

When committing notable changes to the program (new commands, flags, bug fixes, behavior changes), update CHANGELOG.md in the same commit:

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

## Testing

- Always add tests if making changes to observable behaviour. Exercise all visible features.
- Test with `cargo nextest run`
