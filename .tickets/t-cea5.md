---
id: t-cea5
status: closed
deps: []
links: []
created: 2026-01-27T18:24:42Z
type: chore
priority: 2
assignee: OpenCode
tags: [optimize, cli]
closed_at: 2026-01-27T21:44:58.675736385Z
---
# Optimize ready command: reuse metadata

How: compute readiness from cached deps/status maps, adopt consistent sorting, and add --status filter; Why: faster on large sets with consistent ordering.

## Implementation plan

- Reuse a precomputed map of id -> ticket to evaluate readiness without re-reading files; compute `ready` via deps being `closed` only.
- Add a `--status` filter to include tickets with specific status values (default open/in_progress), still ensuring deps are closed.
- Sort results with consistent comparator (priority then id) and print deps count optionally to aid prioritization.
- Add tests for status filter, correct ready determination, and ordering stability.

## Notes

- Implemented `ready --status <open|in_progress|closed>` with default open/in_progress filtering and reused cached ticket metadata for dependency checks.
- Added `--show-deps` to print dependency counts and kept priority/id ordering for stability.
- Updated tests covering status filter, dependency counts, and default behavior; documented flags in README.

2026-01-27T21:44:58.675766476Z
