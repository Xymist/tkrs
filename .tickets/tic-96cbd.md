---
id: tic-96cbd
type: feature
status: closed
deps: []
links: []
priority: 2
assignee: Xymist
tags: []
created: 2026-06-23T14:16:29.191282259Z
closed_at: 2026-06-23T15:25:51.028104269Z
external_ref: null
---
# Add mouse support to the TUI

How: route crossterm mouse events into the existing two-pane TUI, reusing `tui-tree-widget`'s built-in `click_at`/`scroll_*` helpers and recording each pane's `Rect` during render for hit-testing; Why: let users select tickets, switch focus, and scroll without the keyboard.

## Changes made

- Enabled mouse capture around the TUI session in `main.rs` (switched from `ratatui::run` to explicit `init`/`restore` so `EnableMouseCapture`/`DisableMouseCapture` bracket the run, disabling capture even when `run` returns `Err`).
- `TuiApp` now records `tree_area`/`content_area` during `render`; a new `handle_mouse_event` routes events: left-click focuses the clicked pane and selects the tree row under the cursor; the scroll wheel scrolls whichever pane the cursor is over.
- Updated the tree title hint to mention click/scroll; documented the controls in README and CHANGELOG.
- Added 9 unit tests in `src/tui.rs` covering scroll routing, scroll saturation, focus switching, and out-of-bounds / right-click no-ops.

Known limitation: a panic inside the TUI loop leaves mouse capture enabled (ratatui's panic hook restores the screen but not the separately-enabled mouse capture); this matches the pre-existing robustness of the screen restore.

## Notes

- [status_change: open -> closed] Status updated to closed @ 2026-06-23 15:25:51 UTC
