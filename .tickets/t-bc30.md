---
id: t-bc30
status: open
deps: []
links: []
created: 2026-01-27T18:24:42Z
type: chore
priority: 2
assignee: OpenCode
tags: [optimize, cli]
---
# Optimize status command: validation + notes

How: strengthen status validation, allow inline note append, and avoid multiple path resolutions; Why: fewer edge-case failures and clearer history.

## Implementation plan

- Centralize status validation (accepted set, case-insensitive input) and reuse in `status`, `start`, `close`, `reopen` to ensure consistent errors.
- Accept an optional `--note`/`--message` to append a timestamped note alongside the status change, using a single write pass (update frontmatter + append note).
- Avoid multiple path resolutions by reading once into a struct, modifying, and writing back via helper; ensure no-op when setting the same status unless a note is provided.
- Add tests for invalid status rejection, note append, and idempotent same-status updates.
