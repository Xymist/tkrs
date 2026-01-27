---
id: t-9e4d
status: open
deps: []
links: []
created: 2026-01-27T21:32:14.643479636Z
type: task
priority: 2
assignee: Xymist
---
# Generate IDs with 3-char prefix

generate_id should use three-letter prefix for single-segment directory names (e.g., plan -> pla)

## Implementation Plan

- Update `generate_id` to derive a three-letter prefix from a single-segment directory name when dash/underscore splitting yields one segment; fall back to existing logic for multi-segment names.
- Ensure non-ASCII or very short names still produce a 3-char (or full name if shorter) prefix safely.
- Add tests validating prefixes for: multi-segment dirs (`foo-bar` -> `fb`), single-segment (`plan` -> `pla`), short names (`go` -> `go`), and names with underscores/dashes.

## Acceptance Criteria

- Generated IDs from single-segment directories use a three-letter prefix when available (e.g., `plan` -> `pla-xxxx`).
- Behavior for multi-segment names remains unchanged.
- Tests cover the prefix cases above and pass.
