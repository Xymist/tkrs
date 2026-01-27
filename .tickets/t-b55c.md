---
id: t-b55c
status: closed
deps: []
links: []
created: 2026-01-27T18:24:42Z
type: chore
priority: 2
assignee: OpenCode
tags: [optimize, cli]
closed_at: 2026-01-27T20:36:06.086427566Z
---
# Optimize query command: proper escaping

How: add JSON escaping, support ndjson/pretty formats, and stream fields directly; Why: prevent invalid JSON and make script consumption reliable.

## Implementation plan

- In `cmd_query`, perform filtering in-memory when a filter is supplied and otherwise emit NDJSON with proper JSON escaping (use `serde_json::to_writer` to stdout for streaming).
- Add `--format ndjson|pretty` flag: ndjson writes one object per line; pretty uses `to_string_pretty`; keep filter semantics consistent without requiring external tools.
- Stream tickets: iterate and write directly to stdout to reduce memory; ensure fields like `title` and `description` are escaped.
- Add tests covering ndjson vs pretty, filter behavior without external tools, and large ticket sets (no quadratic buffering).

## Notes

2026-01-27T20:36:06.086546537Z Added built-in query filtering with `--format` support and NDJSON streaming; tests cover ndjson vs pretty, filters, and large-set streaming.
