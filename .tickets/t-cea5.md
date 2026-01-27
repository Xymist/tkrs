---
id: t-cea5
status: open
deps: []
links: []
created: 2026-01-27T18:24:42Z
type: chore
priority: 2
assignee: OpenCode
tags: [optimize, cli]
---
# Optimize ready command: reuse metadata

How: compute readiness from cached deps/status maps, adopt asorti sorting, and add --status filter; Why: faster on large sets with consistent ordering.

## Implementation plan
- Reuse a precomputed map of id -> ticket to evaluate readiness without re-reading files; compute `ready` via deps being `closed` only.
- Add a `--status` filter to include tickets with specific status values (default open/in_progress), still ensuring deps are closed.
- Sort results with consistent comparator (priority then id) and print deps count optionally to aid prioritization.
- Add tests for status filter, correct ready determination, and ordering stability.
