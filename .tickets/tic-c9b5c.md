---
id: tic-c9b5c
type: task
status: closed
deps:
- tic-4326a
- tic-99451
links: []
priority: 2
assignee: Xymist
tags: []
created: 2026-07-20T15:08:08.164996625Z
closed_at: 2026-07-20T17:05:45.052315864Z
---
# tk publish: retire the Python script

Delete skills/tk-to-github-issue/tk_to_gh_issue.py; rewrite skills/tk-to-github-issue/SKILL.md as documentation of `tk publish github` (same triggers, flag reference, readability pass, field-spec examples); update README and CHANGELOG; release v0.8.0.

## Implementation Plan

-

## Acceptance Criteria

No Python remains; skills/tk-to-github-issue/SKILL.md documents the native command accurately against tk publish --help; skills/tk-cli/SKILL.md gains publish in its subcommand map, flag summary, and gotchas (id-leak hard failures, --re-file idempotency); README and CHANGELOG updated; v0.8.0 released.

## Notes

- Scope addition per James: the tk-cli skill must also be updated with the publish capabilities, not just the tk-to-github-issue skill rewrite. @ 2026-07-20 15:10:36 UTC

- [status_change: open -> closed] Shipped in v0.8.0: Python deleted, tk-to-github-issue SKILL.md rewritten for the native command, tk-cli SKILL.md documents publish. @ 2026-07-20 17:05:45 UTC
