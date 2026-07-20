---
id: tic-c7e19
type: task
status: closed
deps: []
links: []
priority: 2
assignee: Xymist
tags: []
created: 2026-07-20T15:08:08.138239586Z
closed_at: 2026-07-20T17:05:45.02944496Z
---
# tk publish: body rendering and field templates

Render an issue body the way GitHub renders an issue-form submission: one '### <label>' heading per field. Built-in default field set = the maintenance-ticket form used by the Python script (labels in tk_to_gh_issue.py). Ticket-derived sources: 'describe' (intro + implementation plan, falling back to Notes when the plan section is empty) and 'acceptance'. Configurable via a fields spec (JSON file or TOML config): entries of {label, value} or {label, source}; validate labels non-empty/single-line/unique and values as strings. --title-prefix flag, default '[Maintenance]: '. Section extraction uses the existing TicketBody model directly — no reparsing.

## Implementation Plan

-

## Acceptance Criteria

Default body byte-matches the Python script's output for the same ticket; custom fields spec validated with actionable errors; placeholder '-' sections treated as empty.

## Notes

- [status_change: open -> in_progress] Status updated to in_progress @ 2026-07-20 15:09:31 UTC

- [status_change: in_progress -> closed] Shipped in v0.8.0: default fields byte-match the Python, --fields-json validated, placeholder handling via the model. @ 2026-07-20 17:05:45 UTC
