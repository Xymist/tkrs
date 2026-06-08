---
id: tic-5de61
type: feature
status: closed
deps:
- tic-7fea5
links: []
priority: 2
assignee: Xymist
tags:
- tui
created: 2026-06-08T12:51:05.590675452Z
closed_at: null
external_ref: null
---
# Display contents of the selected ticket in the TUI

Being able to scroll the tickets alone is not sufficient; we should see a scrollable box with the content of the ticket for review.

## Implementation Plan

- Add another block with a scrollbar
- Set up a focus selection enum and key triggers
- Render the content of the selected ticket to the new box

## Acceptance Criteria

- Selecting a ticket in the TUI displays its content
- The content has a scrollbar and is scrollable if longer than the TUI
- Nothing crashes if no ticket is selected
- Toggling back and forth between the tickets and the content should be a single key (probably Tab)

## Notes

- [status_change: open -> closed] Status updated to closed @ 2026-06-08 12:54:28 UTC
