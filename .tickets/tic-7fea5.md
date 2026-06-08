---
id: tic-7fea5
type: task
status: closed
deps: []
links: []
priority: 2
assignee: Xymist
tags:
- tui
created: 2026-06-07T23:39:21.577346593Z
closed_at: null
external_ref: null
---
# Ticket navigation should be possible in the TUI

The TUI so far just renders the list of tickets. Carets appropriately
show for tickets with dependencies, but there's no way to navigate the list.

## Implementation Plan

### Context

`src/tui.rs` currently renders a static `tui_tree_widget::Tree` but wires
up no navigation:

- `handle_key_event` only reacts to `Esc` (exit); arrows/space are ignored.
- `TreeState` is created with `default()`, so nothing is selected on first
  render and there is no highlight style.
- The scrollbar is plumbed manually via a `vertical_scroll: u16` field that
  is never mutated, and rendered as a separate `Scrollbar` widget whose
  `ScrollbarState` position is therefore always 0 (dead code path).
- The tree is rendered into a hand-computed inset `Rect::new(1, 1, w-2, h-2)`
  while also carrying its own `Block::bordered()`, which double-offsets the
  border and leaves the manual scrollbar misaligned with the tree body.

`tui-tree-widget` 0.24 already provides everything required
(`TreeState::{key_up,key_down,key_left,key_right,toggle_selected,
select_first}` and a built-in `experimental_scrollbar`). The work is to wire
these up and remove the broken manual scrollbar plumbing.

### Steps

1. **Trim `TuiApp` state.** Remove the unused `vertical_scroll: u16` field
   (and its initialiser in `new`). The widget tracks scroll offset internally
   via `TreeState::offset`, so no replacement field is needed.

2. **Select the first ticket on construction.** In `TuiApp::new`, after
   building the state, call `tree_state.select_first()` so the first row is
   selected on the initial render. Because selection is by identifier path and
   the tree is rebuilt each frame from the same ticket list, the selection
   remains valid across renders. (AC: first ticket selected/highlighted.)

3. **Add a highlight style.** In the `Widget for &mut TuiApp` impl, chain
   `.highlight_style(...)` onto the `Tree` builder (e.g. reversed/bold or a
   bg colour via `ratatui::style::{Style, Modifier, Color}`) so the selected
   row is visibly distinct. Optionally add `.highlight_symbol("> ")`.
   (AC: visibly selected/highlighted.)

4. **Use the widget's built-in scrollbar; delete the manual one.** Replace the
   separate `Scrollbar` + `ScrollbarState` + manual `Rect` rendering with
   `Tree::experimental_scrollbar(Some(Scrollbar::new(
   ScrollbarOrientation::VerticalRight).begin_symbol(Some("↑"))
   .end_symbol(Some("↓"))))`. Render the tree once via
   `frame.render_stateful_widget` / `StatefulWidget::render` into the **full**
   `area` (drop the hand-rolled `Rect::new(1, 1, ...)` inset — the
   `Block::bordered()` already reserves the border, and the built-in scrollbar
   positions itself inside the block). This makes the scrollbar track the
   selection automatically. (AC: scrollbar responds to up/down.)

5. **Wire key handling.** Rewrite `handle_key_event` to dispatch on
   `key_event.code`:
   - `KeyCode::Esc` (and optionally `Char('q')`) -> `self.exit()`
   - `KeyCode::Up` -> `self.tree_state.key_up()`
   - `KeyCode::Down` -> `self.tree_state.key_down()`
   - `KeyCode::Char(' ')` -> `self.tree_state.toggle_selected()`
   - `KeyCode::Right` -> `self.tree_state.key_right()`
   - `KeyCode::Left` -> `self.tree_state.key_left()`

   The `TreeState` mutators return `bool` (changed); the return can be ignored
   here since the loop redraws every iteration. Keep the existing
   `KeyEventKind::Press` guard in `handle_events`. (AC: up/down navigate,
   space expands, right enters deps, left exits to parent.)

   Note on space vs. right: `toggle_selected` opens/closes the node in place
   (the caret flips) without moving selection, satisfying "spacebar expands";
   `key_right` opens the node *and* descends into its first child, satisfying
   "navigates into a dependency list". `key_left` closes the node or moves to
   the parent, satisfying "navigates out to the parent". These are distinct
   behaviours and both are required.

### Tests

Add a `#[cfg(test)]` module to `src/tui.rs`. Construct `TuiApp::new` with a
small fixture of `Ticket`s (one with a dependency on another so the tree has
an expandable node), then drive `handle_key_event` directly with synthesised
`KeyEvent`s and assert on `tree_state.selected()`:

- After `new`, `selected()` is non-empty and equals the first ticket's id
  path (AC 1).
- A `Down` press changes `selected()` to the second visible row; a following
  `Up` returns it (AC 2).
- On a ticket with deps, `Right` opens the node and descends (selected path
  length increases / selection becomes the first dep); a following `Left`
  returns selection to the parent (AC 4).
- `Char(' ')` on a deps ticket toggles `tree_state` open-state without
  changing `selected()` (AC 3). Assert via two toggles returning to the
  start, or by checking the flattened/opened set if exposed.
- `Esc` sets `exit == true`.

`handle_key_event` and the fixture builders may need to be reachable from the
test module (they are in-file, so `pub(crate)`/private is fine). Rendering can
optionally be smoke-tested with `ratatui::backend::TestBackend` + a
`Terminal`, asserting the buffer contains the first summary and a highlight
cell, but the state-machine assertions above are the primary coverage.

### Definition of done

- `cargo nextest run` passes (incl. the new tests).
- `cargo clippy` clean (the removed `vertical_scroll` / manual scrollbar
  should also clear any dead-code paths).
- `cargo fmt` applied.
- Manual smoke check via `cargo run -- tui`: first row highlighted, arrows
  move selection, scrollbar thumb tracks, space toggles deps caret, right/left
  descend/ascend.
- README has no command/flag surface change here (TUI keybindings only); add a
  brief keybinding note to the TUI section if one exists.
- CHANGELOG.md `Unreleased` gets an `Added` entry for TUI list navigation.

## Acceptance Criteria

- On render, the first ticket is visibly selected and highlighted.
- The up and down arrow keys navigate the list up and down, and the scrollbar responds.
- The spacebar expands a ticket with dependencies.
- The right arrow key navigates into a dependency list, and the left arrow navigates out of it to the parent.

## Notes

- [status_change: open -> closed] Status updated to closed @ 2026-06-08 12:54:10 UTC
