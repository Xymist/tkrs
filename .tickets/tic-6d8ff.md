---
id: tic-6d8ff
type: task
status: closed
deps:
- tic-c7e19
links: []
priority: 2
assignee: Xymist
tags: []
created: 2026-07-20T15:08:08.146029189Z
closed_at: 2026-07-20T17:05:45.035557253Z
---
# tk publish: gh integration, creation, and priority field

Create the issue by shelling out to `gh issue create --repo <r> --title <t> --body-file <tmp>` (plus optional --assignee incl. @me, repeatable --label). Parse the issue number from the returned URL. Best-effort set the repo's single-select Priority issue field via `gh api graphql` (setIssueFieldValue), mapping tk priority 0-3 to Urgent/High/Medium/Low with case/emoji-insensitive option matching; never fail the run on priority errors. --dry-run prints title, body, the gh command, and intended priority without creating. --no-priority and --priority-field NAME flags.

## Implementation Plan

-

## Acceptance Criteria

Issue created and URL printed; priority set when the field exists; all priority failures downgrade to warnings; dry-run creates nothing.

## Notes

- [status_change: open -> in_progress] Status updated to in_progress @ 2026-07-20 15:09:31 UTC

- [status_change: in_progress -> closed] Shipped in v0.8.0: gh issue create subprocess, GraphQL priority best-effort, --dry-run. @ 2026-07-20 17:05:45 UTC
