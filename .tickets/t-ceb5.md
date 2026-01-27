---
id: t-ceb5
status: closed
deps: []
links: []
created: 2026-01-27T18:24:42Z
type: chore
priority: 2
assignee: OpenCode
tags: [optimize, cli]
closed_at: 2026-01-27T21:48:20.444407819Z
---
# Optimize undep command: resilient removal

How: use the shared dep-set helper to prune dependencies and normalize empty arrays; Why: eliminate text edge cases and make undep idempotent.

## Implementation plan

- Reuse the new dep-set helper (shared with add) to remove a dep from the set in memory, keeping deps sorted/deduped.
- Write back only when the set changes; if the dep is absent, do nothing and return success to maintain idempotence.
- Normalize empty deps to `deps: []` to avoid stray whitespace and ensure consistent parsing.
- Add tests for idempotent removal, empty-normalization, and ambiguous id handling.

## Notes

- `undep` now reuses a shared dependency mutator to sort/dedup, writes only on change, and reports already-removed deps without error.
- Normalizes empty dependencies to `deps: []` on write.
- Added tests for idempotent removal, empty normalization, and existing partial resolution paths.

2026-01-27T21:48:20.444436479Z
