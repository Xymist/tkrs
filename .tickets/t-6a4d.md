---
id: t-6a4d
status: closed
deps: []
links: []
created: 2026-01-27T18:24:42Z
type: chore
priority: 2
assignee: OpenCode
tags: [optimize, cli]
---
# Optimize closed command: portable recency

How: keep all ordering in Rust (mtime fallback to id) and apply --since/--limit without relying on external tools; Why: predictable on big directories and platforms.

## Implementation plan

- In `cmd_closed`, keep using Rust stdlib but switch to collecting `(mtime,id,path,status,assignee,tags)` once, sort descending by mtime, and apply `--since` (Rfc3339) + `--limit` after sorting.
- Add a `--since` arg and document in README; ignore parse errors with a clear user-facing message.
- Retain fallback ordering by id when metadata is missing, keeping portability.
- Add tests for limit, since filtering, and stable fallback ordering when metadata missing.

## What changed

- `closed` now accepts `--since <RFC3339>` and filters after sorting newest-first by file mtime with deterministic fallback to id when metadata is missing.
- Sorting and filtering now run entirely in-memory using Rust stdlib; missing mtimes are treated as oldest.
- Added README command summary entry for `tk closed` with the new flags.
- Added CLI tests covering since filtering (future cutoff) and fallback ordering when metadata is absent.
