---
id: t-8f1d
status: closed
deps: []
links: []
created: 2026-01-27T18:24:42Z
type: chore
priority: 2
assignee: OpenCode
tags: [optimize, cli]
closed_at: 2026-01-27T20:23:35.487891375Z
---
# Optimize link command: symmetric sets

How: build link sets in memory, write once per file, and skip rewrites when unchanged; Why: faster on many tickets and guarantees symmetry.

## Implementation plan

- Introduce a reusable link-set builder that reads both primary and targets once, merges links in memory, and writes only when changed; share it with `unlink`.
- Normalize order (sorted, deduped) before comparing/writing to guarantee symmetric sets and minimize diffs.
- Add a dry-run flag to print intended changes without writing, useful on large batches.
- Add tests for symmetric linking, dry-run no-write, and unchanged inputs.

## Notes

2026-01-27T20:23:35.487997296Z
