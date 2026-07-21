---
id: tic-cbc40
type: feature
status: closed
deps: []
links: []
priority: 2
assignee: Xymist
tags: []
created: 2026-07-21T18:30:04.677623704Z
closed_at: 2026-07-21T19:01:30.390773601Z
---
# tk tree --root: restrict output to one ticket subtree

Add a -r/--root <ID> flag to tk tree. When given, the forest contains a single tree rooted at that ticket (partial-ID resolvable), walked in the requested orientation. Status selection applies at every level including the root itself. Unknown ID is an error via resolve_partial_id.

## Implementation Plan

-

## Acceptance Criteria

tk tree --root <id> prints only that subtree; works with --inverted and --status; partial IDs resolve; unknown ID errors; docs (README, skills/tk-cli/SKILL.md) updated; unit tests cover the new assembly path.

## Notes

- [status_change: open -> closed] Shipped in v0.10.0: -r/--root on tk tree, partial-ID resolvable, composes with --status (applies to the root too) and --inverted. @ 2026-07-21 19:01:30 UTC
