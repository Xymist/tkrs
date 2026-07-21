---
id: tic-cbc40
type: feature
status: open
deps: []
links: []
priority: 2
assignee: Xymist
tags: []
created: 2026-07-21T18:30:04.677623704Z
---
# tk tree --root: restrict output to one ticket subtree

Add a -r/--root <ID> flag to tk tree. When given, the forest contains a single tree rooted at that ticket (partial-ID resolvable), walked in the requested orientation. Status selection applies at every level including the root itself. Unknown ID is an error via resolve_partial_id.

## Implementation Plan

-

## Acceptance Criteria

tk tree --root <id> prints only that subtree; works with --inverted and --status; partial IDs resolve; unknown ID errors; docs (README, skills/tk-cli/SKILL.md) updated; unit tests cover the new assembly path.

## Notes

-
