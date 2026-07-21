---
name: tk-cli
description: >
  Reference for the `tk` ticket CLI (tickets stored as Markdown under
  `.tickets/`, usually gitignored). Load this when the task involves
  creating, updating, querying, linking, or closing `tk` tickets —
  including building epics with child tickets and dependency wiring.
  Skip for `TodoWrite`-scale single-session checklists; AGENTS.md has
  the policy on which tool to pick.
---

# `tk` CLI Reference

`tk` is a local-only ticket store. Tickets are Markdown files under
`.tickets/` (usually gitignored). IDs look like `auc-1f89`. Partial
ID matching works for `show`, `start`, `close`, `edit`, etc.

## Subcommand Map

- `create` -- create a ticket; prints the new ID
- `show <id>` -- print full ticket body (`--json` for structured)
- `edit <id>` -- update ticket sections non-interactively from flags;
  pass `-i`/`--interactive` to open `$EDITOR` instead
- `start <id>` -- status to `in_progress`
- `close <id>` -- status to `closed`
- `reopen <id>` -- status to `open`
- `status <id> <state>` -- set status explicitly (`open`,
  `in_progress`, or `closed`)
- `add-note <id> "<text>"` -- append a timestamped note
- `dep <id> <dep_id>` -- add dependency (`<id>` depends on `<dep_id>`)
- `dep tree <id>` -- print dependency tree
- `dep cycle` -- detect cycles
- `undep <id> <dep_id>` -- remove a dependency
- `link <id> <targets>...` -- symmetric link, multi-target
- `unlink <a> <b>` -- remove a link
- `ls` -- list tickets (filterable)
- `ready` -- open/in-progress with all deps resolved
- `blocked` -- open/in-progress with unresolved deps
- `closed` -- recently closed
- `query [FILTER]` -- tickets as NDJSON or pretty JSON
- `tree` -- print the full ticket tree, fully expanded, as plain text
  (`--status all|open|in-progress|closed`, default `open`); mirrors
  the `tui` tree pane's ordering and nesting; a shared dependency
  repeats under every root that depends on it *and* under every branch
  within a single root that reaches it (a diamond shows up twice, not
  just under the first branch); every selection-matching ticket is
  guaranteed to appear somewhere -- a pure dependency cycle, or a
  ticket whose only dependant is filtered out, still renders as its
  own root rather than vanishing; `--inverted` walks the reversed
  graph, rooting at leaf work items and nesting dependants (the same
  completeness guarantee applies here too);
  `-r/--root <id>` restricts output to one ticket's *scope* -- that
  ticket plus its selection-filtered dependency closure (partial IDs
  accepted, `--status` still applied to the root itself). `--inverted`
  combined with `--root` does **not** switch to walking dependants: it
  only re-presents that same fixed scope leaf-first, ascending back to
  the root, so a ticket outside the scope that depends on one of its
  members is still excluded. Output repeats a ticket once per distinct
  dependency path reaching it (no truncation), so a densely
  diamond-shaped graph grows rapidly -- prefer `graph` there, since it
  dedupes each ticket to a single node
- `graph` -- print the ticket dependency graph as a Mermaid
  `flowchart TD` to stdout (for sharing plans with nontechnical
  stakeholders); takes the same `--status`, `--inverted`, and
  `-r/--root` flags as `tree`, including the scope-then-orientation
  `--root`/`--inverted` combination above, but dedupes each ticket to a
  single node while keeping every selection-passing edge, so a shared
  dependency shows as one node with multiple incoming arrows instead
  of repeating per branch; without `--root`, every selection-matching
  ticket gets a node even when no root reaches it (rootless
  dependency cycles still render)
- `publish github <id> <owner/repo>` -- file the ticket as a GitHub
  issue via `gh` (see the `tk-to-github-issue` skill for the full
  workflow, including the required readability pass)
- `update` -- self-update `tk` to the latest GitHub release
- `tui` -- experimental interactive browser (keyboard + mouse)

`start`, `close`, `reopen`, and `status` all accept `--note <TEXT>`
to record a timestamped note alongside the status change.

There is **no `tk add`, no `tk done`, no `tk note`**. Use `create`,
`close`, `add-note`.

## `create` Flags

```text
tk create <TITLE>
  [-d|--description <TEXT>]
  [--implementation-plan <TEXT>]    # implementation-plan section
  [--acceptance <TEXT>]             # acceptance-criteria section
  [-t|--type bug|feature|task|epic|chore]   # default: task
  [-p|--priority 0|1|2|3|4]    # default: 2
  [-a|--assignee <NAME>]
  [--external-ref <e.g. gh-24177>]
  [--parent <PARENT_ID>]
  [-T|--tags <comma,separated>]
  [--body-from-file <PATH>]
  [--edit]                     # open $EDITOR after creation
```

Prints the new ID on stdout. Capture it (`NEW=$(tk create ...)`)
when scripting epic-plus-children flows. Titles must be a single
line.

## `edit` Flags (non-interactive by default)

```text
tk edit <ID>
  [-d|--description <TEXT>]         # replace description
  [--implementation-plan <TEXT>]    # replace implementation plan
  [--acceptance <TEXT>]             # replace acceptance criteria
  [--external-ref <REF>]            # replace external reference
  [--body-from-file <PATH>]         # description from file
  [-i|--interactive]                # open $EDITOR (after any updates)
  [--print]                         # print ticket path
  [--force]                         # editor without a TTY (with -i)
```

- Each flag **replaces** its section wholesale; unspecified sections
  are untouched. Passing an **empty string clears** a section (or the
  external ref).
- `--description` plus `--body-from-file` combine as description
  first, then file content, blank-line separated (same as `create`).
- At least one flag is required: bare `tk edit <id>` is a usage
  error, not an editor launch.
- This is the way to set or change `--external-ref` after creation
  (e.g. when the GitHub issue is opened later): `tk edit <id>
  --external-ref gh-24177`.

## `ls` and `query`

`tk ls` is the human-readable lister; `tk query` is the structured
one. Use `query` from agents.

```sh
tk ls -s open                       # by status
tk ls -a Xymist                     # by assignee
tk ls -T backend,api                # by tags
tk ls --parent auc-1f89             # direct children of an epic
tk ls --columns id,status,title,deps,priority,assignee,tags
tk ls --json                        # JSON; includes `parent`
```

```sh
tk query                            # all tickets, NDJSON
tk query 'tags==backend' --format pretty
tk query 'title~api'                # substring match (~)
```

`tk query` JSON includes `external_ref`, `links`, `deps`, `assignee`,
`priority`, `tags`, `status`, `created`, `id`, `title` — but **no
parent field**. Parent-child is stored as a dependency of the parent
(`create --parent EPIC` adds the child to the epic's `deps`), and
`tk ls --json` / `tk show --json` / `tk ls --parent <id> --json`
expose a derived `parents` array of IDs. To enumerate an epic's
children, use `tk ls --parent <id> --json`, or read the epic's own
`deps` via `tk query 'id==<epic>'`.

## Common Patterns

### Build an epic with linked children and dependencies

```sh
EPIC=$(tk create 'Caching cleanup (umbrella for gh-24177)' \
  -t epic --external-ref gh-24177 \
  -d 'Eight-stage cleanup tracked in #24177.')

A=$(tk create 'Instrument current cache hit rate' --parent "$EPIC" -t task)
B=$(tk create 'Remove fragment cache A' --parent "$EPIC" -t task)
C=$(tk create 'Remove fragment cache B' --parent "$EPIC" -t task)

tk dep "$B" "$A"    # B depends on A (cycle guard is on by default)
tk dep "$C" "$A"    # C depends on A
```

### Update a ticket after creation (agent-safe, no editor)

```sh
tk edit auc-1f89 --acceptance 'All call sites migrated; suite green.'
tk edit auc-1f89 --body-from-file ./plan.md      # replace description
tk edit auc-1f89 --external-ref gh-24312         # pin to GH issue
tk edit auc-1f89 --implementation-plan ''        # clear a section
```

### List and triage children of an epic

```sh
tk ls --parent auc-1f89 --json \
  | jq -r '.[] | "\(.id)\t\(.status)\t\(.title)"'
```

### Close a ticket with a closing note

```sh
tk add-note auc-d307 'Patches landed; Serena MCP healthy. Closing.'
tk close auc-d307
```

### Find what's actionable right now

```sh
tk ready                # nothing blocking; pick from here
tk blocked              # waiting on deps
```

### Publish a ticket to GitHub

```sh
tk publish github auc-1f89 owner/repo --dry-run   # render, create nothing
tk publish github auc-1f89 owner/repo \
  --body-file polished.md --assignee @me          # create + pin external_ref
```

Load the `tk-to-github-issue` skill before publishing — it documents
the required readability pass, `--fields-json` for repos with their
own issue forms, and the pinning/re-file semantics.

## Gotchas

- **Verb names**: `create` / `close` / `add-note`. Not `add`, `done`,
  `note`.
- **`tk edit` is non-interactive by default** (since v0.7.0): flags
  update sections in place and it is safe from subagents and
  non-interactive sessions. Only `-i`/`--interactive` opens `$EDITOR`
  and blocks -- never pass `-i` from a subagent.
- **Empty string clears; `-` is reserved.** `--description ''` clears
  the section. A value of exactly `-` is rejected (it is the
  empty-section placeholder on disk).
- **Reserved headings are rejected in values.** Section values, note
  text, and note tags may not contain lines starting with
  `## Implementation Plan`, `## Acceptance Criteria`, `## Notes`, or
  `## Design` -- the parser would treat them as section delimiters.
  Set each section with its own flag instead of embedding headings.
- **`tk ls --json` includes `parents`** (an array of IDs, derived
  from reverse dependencies); `tk query` output has no parent field
  at all, so `parent==` / `parents==` query filters match nothing.
- **`tk dep` argument order**: `tk dep <id> <dep_id>` means "id
  depends on dep_id". The cycle guard is on by default; pass
  `--check-cycle false` to skip it.
- **`ready`, `blocked`, and `ls` accept `--parent <id>`** to scope
  results to one epic's children.
- **IDs are partial-matchable**. `tk show 5c4` resolves to
  `nw-5c46` etc. Useful when the user pastes a short prefix.
- **`.tickets/` is usually gitignored**. Where it is, ticket IDs are
  not meaningful outside the checkout -- don't reference them in
  commit messages, PR descriptions, branch names, or GitHub comments;
  use `--external-ref gh-NNNNN` to pin a ticket to a public issue.
  (The `tk` repo itself commits its tickets and uses IDs in commit
  messages; follow the convention of the repo you are in.)
- **`tk publish` enforces the id-leak rule and idempotency.** It
  hard-fails if the outbound issue would contain the source ticket's
  id or any id from the local store, and refuses to publish an
  already-pinned ticket without `--re-file`. Publishes are serialized
  per store via `.tickets/.publish.lock`.
- **No bulk delete**. To abandon a ticket, `tk close` it with a note
  explaining why; don't try to remove the file by hand.
- **`tk tree` nests dependencies, it doesn't list every ticket at the
  top level**. A ticket that another ticket depends on is only shown
  under its dependant(s) -- if two roots share a dependency, it
  appears under both, and if two branches *within* the same root both
  reach the same dependency (a diamond), it appears under both
  branches there too, not just the first one reached. Both `tk tree`
  and `tk graph` guarantee every selection-matching ticket appears
  somewhere -- a pure dependency cycle, or a ticket whose only
  dependant is filtered out (e.g. an open dependency of a closed
  ticket under the default open view), still renders as its own root
  rather than silently vanishing. Use `tk ls` or `tk query` for a flat
  view of every ticket regardless of dependency shape.
  `--inverted` flips the same nesting rule around dependants instead
  of dependencies, so an epic reachable from several leaf tickets
  repeats once per work path (the completeness guarantee applies here
  too: an open epic whose only resolvable dependency is a closed leaf
  still renders). `tk graph` covers the same data but dedupes each
  ticket to a single node, so where `tk tree` repeats a shared ticket
  once per branch or per root, `tk graph` shows it once with every
  incoming edge intact.

## When to Dispatch the `@tk-handler` Subagent

For multi-step bulk operations — creating an epic plus more than
two children, wiring a dependency graph, closing many tickets with
notes — dispatch `@tk-handler` instead of running the commands inline. The
subagent keeps the verbose CLI churn out of the parent context and
returns a structured summary of IDs touched.

For one-off operations (`tk show`, single `tk create`, single
`tk close`), run them inline.
