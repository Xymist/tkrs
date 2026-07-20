---
id: tic-b949a
type: task
status: open
deps: []
links: []
priority: 2
assignee: Xymist
tags: []
created: 2026-07-20T18:07:28.145865194Z
---
# Ticket tree assembly can hide components under status filters and cycles

Shared forest assembly (src/tree.rs, used by both tk tui and tk tree) has two long-standing quirks inherited from the TUI: (1) dependency_ids is collected before the status filter, so a ticket that matches the filter but whose only dependant is filtered out never appears (e.g. an open dep of a closed ticket under the default open view); a component that is purely a dependency cycle has no root and vanishes entirely. (2) The visited set is per-root rather than path-local, so in a diamond (A->B, A->C, B->D, C->D) D renders under only one of B/C. Both behaviours are currently documented and locked by tests. If fixed, fix in the shared assembly so tui and tree stay consistent, and update README, skills/tk-cli/SKILL.md, and the tests that lock the current behaviour.

## Implementation Plan

-

## Acceptance Criteria

-

## Notes

-
