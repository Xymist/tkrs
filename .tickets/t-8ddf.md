---
id: t-8ddf
status: closed
deps: []
links: []
created: 2026-01-27T21:25:55.553816883Z
type: chore
priority: 2
assignee: OpenCode
tags: [cli, bug]
closed_at: 2026-01-27T21:28:53.094558003Z
---
# Walk parent dirs for .tickets

How: resolve the tickets directory by walking upward from the current working directory until a `.tickets` folder is found (or root), falling back to `TICKETS_DIR` when set, and ensuring callers use the resolved path; Why: CLI commands currently only work from the repo root despite the changelog claiming parent directory discovery.

## Implementation plan

- Add a resolver that starts at `current_dir` and ascends parents to locate `.tickets`; if `TICKETS_DIR` is set, prefer that absolute/relative path.
- Update `tickets_dir()` to return the discovered path and adjust command helpers to use it consistently (create, status, dep/undep, link/unlink, ls/ready/blocked/closed, show/edit/add-note/query).
- Add tests covering discovery from nested subdirectories, fallback to env override, and error behavior when no directory is found; ensure existing tempdir tests create `.tickets` at the root they operate on.

## Notes

- Implemented parent-walk with `TICKETS_DIR` override in `tickets_dir()`.
- Added CLI tests for parent discovery and env override.
- Updated README and changelog to reflect behavior.

2026-01-27T21:28:53.094587453Z
