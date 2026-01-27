---
id: t-ceb5
status: open
deps: []
links: []
created: 2026-01-27T18:24:42Z
type: chore
priority: 2
assignee: OpenCode
tags: [optimize, cli]
---
# Optimize undep command: resilient removal

How: use the shared dep-set helper to prune dependencies and normalize empty arrays; Why: eliminate regex edge cases and make undep idempotent.

## Implementation plan
- Reuse the new dep-set helper (shared with add) to remove a dep from the set in memory, keeping deps sorted/deduped.
- Write back only when the set changes; if the dep is absent, do nothing and return success to maintain idempotence.
- Normalize empty deps to `deps: []` to avoid stray whitespace and ensure consistent parsing.
- Add tests for idempotent removal, empty-normalization, and ambiguous id handling.
