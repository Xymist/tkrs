---
id: t-6a4d
status: open
deps: []
links: []
created: 2026-01-27T18:24:42Z
type: chore
priority: 2
assignee: OpenCode
tags: [optimize, cli]
---
# Optimize closed command: portable recency

How: replace the ls/head pipeline with find/awk ordered by mtime and allow --since/--limit; Why: more portable and predictable on big directories.

## Implementation plan
- In `cmd_closed`, keep using Rust stdlib but switch to collecting `(mtime,id,path,status,assignee,tags)` once, sort descending by mtime, and apply `--since` (Rfc3339) + `--limit` after sorting.
- Add a `--since` arg and document in README; ignore parse errors with a clear user-facing message.
- Retain fallback ordering by id when metadata is missing, keeping portability.
- Add tests for limit, since filtering, and stable fallback ordering when metadata missing.
