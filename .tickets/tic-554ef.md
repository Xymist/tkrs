---
id: tic-554ef
type: bug
status: open
deps: []
links:
- tic-b949a
priority: 3
assignee: Xymist
tags: []
created: 2026-07-21T20:15:09.003822321Z
---
# Duplicate-id stores: scoped-inverted eligibility and walker visited-ordering can hide tickets

Two related quirks affecting only hand-corrupted stores where one frontmatter id appears on multiple tickets with differing deps/status. (1) is_in_sink_scc resolves reachability through an id-keyed last-wins lookup, so a closed duplicate with different deps can make every open cycle member ineligible, emptying tk tree --root --inverted while tk graph still renders (documented caveat on is_in_sink_scc). (2) children_recursively inserts a child id into visited before applying the selection filter, so a selection-failing duplicate consumes the id and masks its selection-passing twin and that twins subtree; this ordering predates the scoped-inverted work. Fix together: compute eligibility over the same duplicate-aware edge relation the inverted walk uses, and apply selection before visited insertion (or canonicalize/reject duplicate ids at load time, which would subsume both).

## Implementation Plan

-

## Acceptance Criteria

Reversed-file-order duplicate-id regressions (cyclic and acyclic, mixed status) pass for tree and graph; or duplicate ids are rejected/canonicalized at load with a clear error.

## Notes

-
