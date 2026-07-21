---
id: tic-5bbab
type: feature
status: closed
deps:
- tic-cbc40
links: []
priority: 2
assignee: Xymist
tags: []
created: 2026-07-21T18:30:04.690427568Z
closed_at: 2026-07-21T19:01:30.402666668Z
---
# tk graph: Mermaid diagram of the ticket graph

New subcommand tk graph taking the same flags as tk tree (--status, --inverted, --root). Emits a Mermaid flowchart (top-down) of the ticket dependency graph to stdout: one node per ticket (deduped, unlike tree), one edge per dependency edge reachable under the current filters. Intended for sharing project plans with nontechnical stakeholders.

## Implementation Plan

-

## Acceptance Criteria

tk graph emits valid Mermaid flowchart TD; nodes deduped with all edges present (diamond A->B->D, A->C->D shows both edges into D); labels escaped; --status/--inverted/--root behave as in tk tree; docs updated; unit tests cover assembly and rendering.

## Notes

- [status_change: open -> closed] Shipped in v0.10.0: tk graph emits Mermaid flowchart TD with straight edges (curve: linear init directive), deduped t_-prefixed nodes, all selection-passing edges, and a fallback sweep so rootless cycles render. @ 2026-07-21 19:01:30 UTC
