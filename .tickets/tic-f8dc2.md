---
id: tic-f8dc2
type: task
status: closed
deps: []
links: []
priority: 2
assignee: Xymist
tags: []
created: 2026-07-20T17:48:28.571089677Z
closed_at: 2026-07-20T18:27:54.235923951Z
---
# tk tree: print the full ticket tree to the terminal

Add a new subcommand `tk tree` that prints the same ticket tree shown in the left panel of `tk tui`, but fully expanded, as plain terminal output (no TUI). Rendering should reuse/mirror the TUI tree's ordering, hierarchy, and status markers so the two stay consistent.

## Implementation Plan

-

## Acceptance Criteria

-

## Notes

- Implemented: new src/tree.rs holds TicketSelection, TicketNode, assemble_ticket_forest (extracted from tui.rs so TUI and CLI share one walk), render_forest/print_forest (tree-style box-drawing glyphs). New subcommand tree with -s/--status all|open|in-progress|closed, default open (matches TUI default view). tui.rs now maps the shared forest into TreeItems. 23 new tests (unit in tree.rs incl. MC/DC block + connector rendering, integration in tests/cli.rs). Docs: README, skills/tk-cli/SKILL.md, CHANGELOG. Known inherited quirks (filter-hidden components, per-root diamond dedup) recorded as tic-b949a. @ 2026-07-20 18:27:54 UTC

- [status_change: open -> closed] Status updated to closed @ 2026-07-20 18:27:54 UTC
