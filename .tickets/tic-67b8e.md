---
id: tic-67b8e
type: task
status: closed
deps: []
links: []
priority: 2
assignee: Xymist
tags: []
created: 2026-07-20T18:31:13.938202106Z
closed_at: 2026-07-20T18:55:57.191378148Z
---
# tk tree --inverted: leaf-first orientation

Add an --inverted flag to tk tree. Inverted orientation walks the reversed dependency graph: roots (column 0) are tickets with no resolvable deps (the leaf work items), and each node's children are its dependants, so indentation increases along the work path until it reaches the epic(s) on the far right. Same status-filter semantics, same per-root visited guard, same glyphs. Implemented in the shared assembly (src/tree.rs) so the TUI could adopt it later; CLI-only surface for now.

## Implementation Plan

-

## Acceptance Criteria

-

## Notes

- Implemented: Orientation { Normal, Inverted } on assemble_ticket_forest; inverted roots are tickets with no resolvable dep, children are dependants in slice order, walked by the shared children_recursively via a ChildSource enum (Normal reads each ticket's own deps() so duplicate ids keep their own dependency lists — regression found by Codex cross-review and locked by test). CLI: tk tree --inverted, composes with -s/--status. 16 new tests (unit + integration, MC/DC block extended with Decision D). Docs: README, skills/tk-cli/SKILL.md, CHANGELOG. Quirk symmetry noted on tic-b949a. @ 2026-07-20 18:55:56 UTC

- [status_change: open -> closed] Status updated to closed @ 2026-07-20 18:55:57 UTC
