---
id: t-2a9f
status: closed
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

## What changed

- `create` now validates tags (no spaces/brackets) before writing; clap already bounds priority/type.
- Added `--template` with `{id},{title},{created}` substitution and `--body-from-file` to append external body content; both are single-pass reads.
- Refactored `cmd_create` to build the full file content in memory and write once, honoring template vs generated headings.
- Added CLI tests for invalid tags, template substitution, and body-from-file in `tests/cli.rs`.
