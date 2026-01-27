---
id: t-eb47
status: closed
deps: []
links: []
created: 2026-01-27T18:24:42Z
type: chore
priority: 2
assignee: OpenCode
tags: [optimize, cli]
closed_at: 2026-01-27T21:54:17.337730846Z
---
# Optimize dep tree command: faster traversal

How: reuse cached graph metadata, switch to asorti ordering, and add --status/--only-open filters; Why: improves speed and readability on large graphs.

## Implementation plan

- Build a graph cache once (id -> deps/status/title) and reuse it for traversal to avoid repeated file reads during `dep tree`.
- Add flags `--status` (filter nodes by status) and `--only-open` to skip closed deps while rendering; defaults to current behavior.
- Sort children with a consistent comparator (id or priority when available) before printing to stabilize output across runs.
- Add tests for status filtering, only-open traversal, and deterministic ordering on shared graphs.

## Notes

- `dep tree` now reuses the cached graph from `read_ticket_graph`, adds `--status` filter and `--only-open` flag, and keeps sorted children for stable output.
- Added tests for status filtering and only-open traversal.

2026-01-27T21:54:17.337757577Z
