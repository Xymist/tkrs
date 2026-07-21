---
id: tic-554ef
type: bug
status: closed
deps: []
links:
- tic-b949a
priority: 3
assignee: Xymist
tags: []
created: 2026-07-21T20:15:09.003822321Z
closed_at: 2026-07-21T21:53:18.416310348Z
---
# Duplicate-id stores: scoped-inverted eligibility and walker visited-ordering can hide tickets

Two related quirks affecting only hand-corrupted stores where one frontmatter id appears on multiple tickets with differing deps/status. (1) is_in_sink_scc resolves reachability through an id-keyed last-wins lookup, so a closed duplicate with different deps can make every open cycle member ineligible, emptying tk tree --root --inverted while tk graph still renders (documented caveat on is_in_sink_scc). (2) children_recursively inserts a child id into visited before applying the selection filter, so a selection-failing duplicate consumes the id and masks its selection-passing twin and that twins subtree; this ordering predates the scoped-inverted work. Fix together: compute eligibility over the same duplicate-aware edge relation the inverted walk uses, and apply selection before visited insertion (or canonicalize/reject duplicate ids at load time, which would subsume both).

## Implementation Plan

-

## Acceptance Criteria

Reversed-file-order duplicate-id regressions (cyclic and acyclic, mixed status) pass for tree and graph; or duplicate ids are rejected/canonicalized at load with a clear error.

## Notes

- New repro from v0.11.0 review: rendering resolves deps via the unfiltered last-wins lookup while fallback eligibility uses a selection-filtered lookup, so with open r->x plus an open x AND a closed duplicate x, the renderer prunes the closed copy while the filtered SCC view sees x as non-eligible, omitting open x entirely (tree diverges from graph). Also: tui.rs Tree::new expect panics on duplicate top-level ids and add_child errors are swallowed by unwrap_or_default (blank pane) — harden the TUI boundary with path-qualified widget identifiers or load-time duplicate rejection. @ 2026-07-21 21:18:28 UTC

- [status_change: open -> closed] Shipped: load-time rejection with path-naming error; assembly layer unchanged; TUI boundary renders errors visibly. Committed as tic-554ef. @ 2026-07-21 21:53:18 UTC
