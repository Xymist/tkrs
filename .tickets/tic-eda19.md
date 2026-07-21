---
id: tic-eda19
type: chore
status: closed
deps: []
links: []
priority: 3
assignee: Xymist
tags: []
created: 2026-07-21T21:18:28.540013826Z
closed_at: 2026-07-21T22:42:30.445828485Z
---
# Bounded expansion policy for path-local tree repeats on dense DAGs

Path-local repeats (v0.11.0, tic-b949a quirk 2) make tk tree/tui output grow with the number of distinct dependency paths: a layered graph with two tickets per level, each depending on both tickets in the next level, has O(d) tickets but O(2^d) rendered nodes, and the TUI reassembles the forest on refresh. Realistic stores (shallow, sparse diamonds) are unaffected; an adversarial or auto-generated dense ladder can hang the CLI or exhaust memory. Add a bounded expansion policy: a per-root or total node cap with an explicit truncation marker in the output (and a flag to lift it), or shared-substructure rendering with reference markers. Documented in README/SKILL.md as of v0.11.0 with tk graph recommended for dense graphs.

## Implementation Plan

-

## Acceptance Criteria

Dense layered-diamond stress fixture renders within the bound with a visible truncation marker instead of hanging; default behaviour on shallow stores unchanged; TUI protected by the same bound.

## Notes

- [status_change: open -> closed] Shipped: 10k default node budget with marker, --unbounded flag, linear closure DFS, empty-id load rejection. Committed as tic-eda19. @ 2026-07-21 22:42:30 UTC
