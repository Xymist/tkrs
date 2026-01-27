---
id: t-bc30
status: closed
deps: []
links: []
created: 2026-01-27T18:24:42Z
type: chore
priority: 2
assignee: OpenCode
tags: [optimize, cli]
closed_at: 2026-01-27T21:00:58.038035227Z
---
# Optimize status command: validation + notes

How: strengthen status validation, allow inline note append, and avoid multiple path resolutions; Why: fewer edge-case failures and clearer history.

## Implementation plan

- Centralize status validation (accepted set, case-insensitive input) and reuse in `status`, `start`, `close`, `reopen` to ensure consistent errors.
- Accept an optional `--note`/`--message` to append a timestamped note alongside the status change, using a single write pass (update frontmatter + append note).
- Avoid multiple path resolutions by reading once into a struct, modifying, and writing back via helper; ensure no-op when setting the same status unless a note is provided.
- Add tests for invalid status rejection, note append, and idempotent same-status updates.

## Notes

- Implemented `parse_status` to normalize and validate statuses for `status/start/close/reopen`.
- `set_status_with_note` now works in one pass (status + closed_at + optional notes), preserves notes header, and no-ops on same status unless a note/timestamp is requested.
- `status` accepts `--note`/`--message`, case-insensitive values; same-status idempotency kept.
- Added CLI tests for invalid status, case-insensitive status with note, and same-status idempotent behavior.
- Ran `cargo fmt`, `cargo clippy`, and `cargo nextest run` successfully.

2026-01-27T21:00:58.038064637Z
