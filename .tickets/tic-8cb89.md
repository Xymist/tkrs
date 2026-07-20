---
id: tic-8cb89
type: task
status: closed
deps: []
links: []
priority: 2
assignee: Xymist
tags: []
created: 2026-07-20T11:15:23.226210308Z
closed_at: 2026-07-20T14:09:43.83249609Z
external_ref: null
---
# Distribute the tk-cli agent skill from this repo

The tk-cli skill (Claude Code skill documenting the CLI) currently lives only in ~/.claude/skills/tk-cli. Move the canonical copy into this repo under skills/tk-cli/ and symlink it back to ~/.claude/skills/tk-cli so it ships with the repo and stays in lockstep with the CLI it documents.

## Implementation Plan

Copy the skill into skills/tk-cli/ in-repo; rewrite its content to match the v0.7.0 edit behaviour (non-interactive by default, -i/--interactive for $EDITOR, empty-string clearing, --external-ref updatable post-creation) and fix stale flag names (--design is now --implementation-plan); replace ~/.claude/skills/tk-cli with a symlink to the repo copy; note the symlink installation step in the README. Release as v0.7.1.

## Acceptance Criteria

Skill content lives in-repo and accurately describes the v0.7.0 CLI surface; ~/.claude/skills/tk-cli is a symlink to it; skill loads correctly via the Skill tool; README documents how to install the symlink; released as v0.7.1.

## Notes

- Skill moved in-repo at skills/tk-cli/SKILL.md, rewritten for v0.7.0 edit behaviour (stale --design flag fixed, validation gotchas documented); ~/.claude/skills/tk-cli is now a symlink to it; README documents installation. Shipping as v0.7.1. @ 2026-07-20 14:09:43 UTC

- [status_change: open -> closed] Status updated to closed @ 2026-07-20 14:09:43 UTC
