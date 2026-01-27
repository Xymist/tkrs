---
id: t-bb55
status: open
deps: []
links: []
created: 2026-01-27T18:24:42Z
type: chore
priority: 2
assignee: OpenCode
tags: [optimize, cli]
---
# Optimize edit command: env-aware fallback

How: respect VISUAL/EDITOR precedence, add --print path for non-tty, and validate ticket existence before launch; Why: better UX in shells and automations.

## Implementation plan
- Validate ticket exists before launching editor; reuse `resolve_ticket_path` and avoid double reads.
- Choose editor using precedence VISUAL > EDITOR > `vi`; add `--print` flag to just echo the path when running in non-interactive contexts.
- Detect non-tty stdout and fallback to `--print` behavior unless explicitly forced to open, to avoid blocking in CI.
- Add tests covering editor selection via env, print mode, and missing ticket error.
