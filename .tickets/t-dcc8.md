---
id: t-dcc8
status: closed
deps: []
links: []
created: 2026-01-27T18:24:42Z
type: chore
priority: 2
assignee: OpenCode
tags: [optimize, cli]
closed_at: 2026-01-27T21:50:39.214153478Z
---
# Optimize add-note command: safer appends

How: enforce trailing newline, avoid duplicate Notes headers, and allow --tag metadata; Why: prevent formatting drift and support structured notes.

## Implementation plan

- Ensure `add-note` always appends a trailing newline and inserts `## Notes` once; detect existing section to avoid duplicates.
- Add `--tag <label>` option that prefixes the note with a tag (e.g., `[infra]`) before the timestamp or within the note block.
- Write notes via buffered append with newline normalization to prevent missing separators between notes.
- Add tests for duplicate heading avoidance, tag formatting, and newline correctness.

## Notes

- `add-note` now buffers writes, trims trailing newlines, appends a newline, and avoids duplicating the `## Notes` heading.
- Added `--tag <label>` to prefix notes (e.g., `[infra] **ts**`).
- Tests cover single heading, tag formatting, and newline normalization.

2026-01-27T21:50:39.214175588Z
