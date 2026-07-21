---
id: tic-d527d
type: chore
status: open
deps: []
links: []
priority: 3
assignee: Xymist
tags: []
created: 2026-07-21T20:15:09.028411146Z
---
# Replace per-candidate sink-SCC reachability with one Tarjan/Kosaraju condensation

The scoped-inverted fallback sweep calls is_in_sink_scc per unrepresented candidate, each doing a DFS plus one DFS per forward-reachable id: Theta(n^3) on an upstream chain ending in a cycle. Fine at realistic store sizes but wasteful; compute SCCs once in O(V+E), derive sink components from the condensation, and make eligibility an O(1) membership check. Keep slice-order determinism of seed choice within each sink component.

## Implementation Plan

-

## Acceptance Criteria

Single SCC pass replaces per-candidate reachability; existing tree/graph tests unchanged; determinism preserved.

## Notes

-
