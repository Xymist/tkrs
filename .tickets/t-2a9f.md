---
id: t-2a9f
status: open
deps: []
links: []
created: 2026-01-27T18:24:42Z
type: chore
priority: 2
assignee: OpenCode
tags: [optimize, cli]
---
# Optimize create command: validation & templates

How: validate priority/type/tags, add --template/--body-from-file options, and reuse a single write pass; Why: prevent malformed frontmatter and speed creation on slow disks.

## Implementation plan

- In `cmd_create` (`src/main.rs`), add validation: enforce `priority` within 0-4 (already clap-parsed) and reject tags containing whitespace or brackets; ensure `ticket_type` is limited to enum (already) but surface friendly errors.
- Add `--template <path>` and `--body-from-file <path>` flags that, if set, load content once; template replaces the entire body (after frontmatter) with placeholders resolved for id/title/created; body-from-file appends raw body after generated heading.
- Refactor the write path to build the complete file content in memory once (frontmatter + title + body) and write with a single `fs::write` call to minimize IO churn.
- Add tests in `tests/cli.rs` for template usage, body-from-file, and invalid tag rejection.
