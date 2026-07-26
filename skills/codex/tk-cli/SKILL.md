---
name: tk-cli
description: Manage durable local work with the `tk` ticket CLI, whose Markdown tickets live under `.tickets/`. Use when Codex needs to create, inspect, update, link, prioritize, start, or close tk tickets; build an epic with child tickets and dependencies; query actionable or blocked work; or preserve a plan across sessions. Do not use for a small single-session checklist that belongs in Codex's in-session plan.
---

# Manage tickets with `tk`

Use `tk` for durable work tracking. Tickets normally live in a gitignored
`.tickets/` directory and may be picked up by another agent or session.

Follow the repository's `AGENTS.md` when it specifies whether to use `tk`, how
to structure tickets, or which lifecycle commands to run.

## Choose the right planning surface

- Use Codex's in-session plan for a granular checklist expected to finish in
  the current session.
- Use `tk` when the plan must survive compaction or a new thread, spans
  independently completable workstreams, or needs durable acceptance criteria
  and dependencies.
- Promote unfinished in-session work to `tk` before it outlives the session.
- Keep local ticket IDs out of shared commit messages, branches, pull requests,
  and GitHub issues unless the repository explicitly tracks them publicly.

## Discover the local interface

Prefer the installed command's help when this reference and the binary differ:

```sh
tk --help
tk <subcommand> --help
```

In the `tk` source repository, use `cargo run -- <subcommand>` when the task
requires exercising the working-copy implementation rather than the installed
binary.

## Use non-interactive commands

Core commands:

```text
tk create <title> [flags]
tk show <id> [--json]
tk edit <id> [update flags]
tk start <id> [--note <text>]
tk close <id> [--note <text>]
tk reopen <id> [--note <text>]
tk status <id> <open|in_progress|closed> [--note <text>]
tk add-note <id> <text>
tk dep <id> <dependency-id>
tk undep <id> <dependency-id>
tk link <id> <targets>...
tk unlink <id> <target>
tk ready
tk blocked
tk ls [filters]
tk query [filter]
tk tree [flags]
tk graph [flags]
```

There is no `tk add`, `tk done`, or `tk note`. Use `create`, `close`, and
`add-note`.

Do not invoke `tk edit --interactive` from an agent. Apply section updates with
flags:

```sh
tk edit <id> --description '...'
tk edit <id> --implementation-plan '...'
tk edit <id> --acceptance '...'
tk edit <id> --external-ref gh-1234
```

Each provided value replaces that section. An empty string clears it.
Unspecified sections remain unchanged.

## Create well-scoped tickets

Use a one-line title plus the fields that make the work independently
actionable:

```sh
tk create 'Add cache metrics' \
  --type task \
  --priority 2 \
  --description 'Measure current hit and miss rates.' \
  --implementation-plan 'Instrument the cache wrapper and dashboard.' \
  --acceptance 'Metrics are emitted and covered by tests.'
```

Useful creation flags:

```text
--description <text>
--implementation-plan <text>
--acceptance <text>
--type bug|feature|task|epic|chore
--priority 0|1|2|3|4
--assignee <name>
--external-ref <ref>
--parent <parent-id>
--tags <comma,separated>
--body-from-file <path>
```

For one ticket, work inline. When repository instructions call for delegation,
delegate only bulk mechanical operations such as creating an epic, several
children, and their dependency wiring; retain responsibility for the
decomposition and verify the resulting graph.

## Build an epic

Capture printed IDs instead of guessing them:

```sh
EPIC=$(tk create 'Cache cleanup' --type epic --external-ref gh-1234)
A=$(tk create 'Instrument cache hit rate' --parent "$EPIC")
B=$(tk create 'Remove obsolete fragment cache' --parent "$EPIC")
tk dep "$B" "$A"
```

`tk dep <id> <dependency-id>` means the first ticket depends on the second.
The command rejects cycles by default. Inspect the result:

```sh
tk tree --root "$EPIC" --status all
tk dep cycle
```

Use `tk graph` when a Mermaid dependency graph communicates the plan more
clearly. Add `--inverted` to show leaf work flowing toward its dependants.

## Query before reading files directly

Use structured output for agent workflows:

```sh
tk show <id> --json
tk ls --parent <epic-id> --json
tk query 'tags==backend' --format pretty
tk query 'title~cache'
```

Use `tk ready` for tickets whose dependencies are resolved and `tk blocked`
for tickets still waiting on dependencies. Partial IDs are accepted when they
resolve unambiguously.

`tk query` does not expose a parent field. Parent-child relationships are
derived from dependencies; enumerate children with
`tk ls --parent <id> --json`.

## Preserve ticket integrity

- Never hand-edit ticket frontmatter when a `tk` command can perform the
  mutation.
- Do not embed reserved section headings inside flag values:
  `## Implementation Plan`, `## Acceptance Criteria`, `## Notes`, or
  `## Design`.
- Treat `.tickets/` as local-only when it is gitignored.
- Pin public work privately with `tk edit <id> --external-ref gh-<number>`.
- Use `$tk-to-github-issue` for `tk publish github`; it defines the required
  dry-run, readability, leak-prevention, and pinning workflow.
