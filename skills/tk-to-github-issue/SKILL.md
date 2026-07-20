---
name: tk-to-github-issue
description: Publish a local tk ticket as a GitHub issue with `tk publish github` — rendered as an issue-form submission (default field set is a maintenance-ticket form; adapt to any repo's form with --fields-json). Use when the user asks to file/raise/open a GitHub issue from a tk ticket, publish a tk ticket to GitHub, or push security/maintenance tickets to a GitHub repo. Covers the required readability pass, the tk-id-must-not-leak rule, and idempotent external_ref pinning.
---

# tk → GitHub issue (`tk publish github`)

`tk publish github <id> <owner/repo>` (tk ≥ 0.8.0) creates a GitHub
issue from a local ticket, with the body rendered the way GitHub
renders an **issue form** submission: one `### <field label>` heading
per field, value beneath. The default field set is a conventional
maintenance-ticket form; when the target repo has its own issue form,
pass `--fields-json` so the body uses that form's labels and parses
as a submission of it.

The command owns creation, field mapping, pinning, and priority —
never compose an issue by hand with `gh issue create`. You own the
**readability pass** (below): `--dry-run` renders the raw body, you
polish the prose, and `--body-file` feeds it back.

## When to use

The user wants a `tk` ticket published as a GitHub issue — e.g.
"raise a GitHub issue for ab-1cd7", "file these security tickets in
owner/infra-repo", "open auc-5699f on GitHub".

## Usage

```text
tk publish github <ID> <OWNER/REPO> [options]
```

Run `tk publish github --help` for the authoritative flag list.

| flag | default | meaning |
| --- | --- | --- |
| `--assignee NAME` | none | GitHub assignee (`@me` for the caller) |
| `--title-prefix STR` | `"[Maintenance]: "` | issue title prefix |
| `--fields-json PATH` | maintenance form | replace the field set (see below) |
| `--dry-run` | off | print title, body, `gh` command; create nothing |
| `--body-file PATH` | off | file becomes the body (metadata from ticket) |
| `--no-pin` | off | do not write `external_ref` back to the ticket |
| `--re-file` | off | allow re-filing an already-pinned ticket |
| `--no-priority` | off | do not set the repo Priority issue field |
| `--priority-field NAME` | `Priority` | single-select issue field to set |
| `--label LABEL` | none | add a label (repeatable) |

The ticket store is resolved like every other tk command: upward
from the current directory, or via `TICKETS_DIR`. To publish a
ticket from a *different* repo's store, run from inside that repo
(or set `TICKETS_DIR` to its `.tickets/` directory).

Runs are serialized per store (a lock file under `.tickets/`);
a second concurrent publish fails immediately with a clear message
rather than double-creating.

**Always `--dry-run` first** to render the raw body, then perform
the readability pass below and create the issue with `--body-file`.

## Readability pass (required)

tk tickets are written for agents and often arrive as dense
single-block prose. GitHub issues are read by humans, so the
ticket-derived text must be reformatted before the issue is created.
The standard flow:

1. Run with `--dry-run`. The body is only the `###` sections of the
   output — the `# would create...`, `TITLE:`, `# gh command...`,
   and `# would set Priority...` lines are dry-run chrome and must
   not reach the scratch file.
2. Rewrite the ticket-derived sections (the description field and,
   when populated from the ticket, the acceptance criteria) per the
   rules below. Save the full polished body — every `###` section,
   nothing else — to a scratch file.
3. Re-run with `--body-file <scratch-file>` to create the issue.
   Title, `external_ref` pinning, and priority still come from the
   ticket, so all other flags work as normal.

Formatting rules for the rewrite:

- **Preserve the form structure.** Keep every `### <field label>`
  heading byte-identical and in order — the body must still parse as
  a submission of the target form. Leave fixed-value fields exactly
  as rendered.
- **Preserve meaning exactly.** Reformat, never rewrite: no new
  claims and no dropped qualifiers or constraints.
- **No tk ids, including pre-existing ones.** If the ticket prose
  cross-references a tk id (its own or another ticket's), replace it
  with that ticket's `gh-<n>` external ref when pinned, or drop the
  id and describe the ticket in words. The command refuses to create
  a body containing the source ticket's id or any id from the local
  store, and warns on anything else shaped like one.
- **Code in backticks.** File paths, class/method names, code
  expressions, config keys, and constants get inline backticks
  (`` `Time.use_zone` ``, `` `app/models/bid.rb:112` ``); multi-line
  code gets fenced blocks.
- **Links are links.** Bare URLs become `[descriptive text](url)`;
  references to PRs/issues in the same repo become `#NNNN` so GitHub
  autolinks them.
- **Paragraphs, one idea each.** Break wall-of-text prose at the
  natural seams: symptom, cause, impact, fix.
- **Real lists.** Inline enumerations — "(1) … (2) …", comma-chained
  steps, multi-part fixes — become numbered lists or bullets. A
  multi-defect ticket gets one bold run-in heading per defect.

Skip the pass only when the dry-run body is already clean, short, and
formatted — then plain re-run without `--dry-run` is fine.

## Field mapping

Default fields (in order) and where their values come from:

| Issue form field | Value |
| --- | --- |
| title | prefix + ticket title |
| Describe the change required | ticket intro **+ implementation plan** |
| List the success/acceptance criteria... | `## Acceptance Criteria` |
| How many hours of developer time has this lost us? | `1` (fixed) |
| What happens if we don't do this? | standard default text |
| How easy is this to change or fix? | `Unsure` (fixed) |
| Which module(s) would making this change affect? | `Unsure/Other` (fixed) |

To target a different issue form, pass `--fields-json <path>` with a
JSON array replacing the whole set. Each entry is either a fixed
value or ticket-derived content:

```json
[
  {"label": "What needs to change?", "source": "describe"},
  {"label": "Definition of done", "source": "acceptance"},
  {"label": "Severity", "value": "Unknown"}
]
```

`source: describe` is the ticket intro plus its implementation plan
(falling back to `## Notes` when the plan section is empty);
`source: acceptance` is the ticket's acceptance criteria. Labels
must match the target repo's form field labels **byte-for-byte**
(GitHub only parses the body as a form submission when the headings
match), so read the repo's `.github/ISSUE_TEMPLATE/*.yml` first and
copy the `label:` values exactly.

## Priority (issue-fields preview)

After the issue is created, the command sets the repo's **`Priority`**
single-select issue field (GitHub's repository issue-fields preview — see
<https://github.com/orgs/community/discussions/189141>) from the ticket's tk
priority:

| tk priority | Priority option |
| --- | --- |
| P0 (`priority: 0`) | Urgent |
| P1 (`priority: 1`) | High |
| P2 (`priority: 2`) | Medium |
| P3 (`priority: 3`) | Low |

Option matching is case/emoji-insensitive, so a configured option like
`🌋 Urgent` still matches. This is **best-effort and never fails the run** — if
the repo has no `Priority` issue field, the mapped option name isn't present,
the ticket has no priority, or the token lacks permission to set fields, the
command prints a note/warning and continues (the issue is already created).
Setting the field needs a token that can write issue fields; reads/writes use
the `setIssueFieldValue` GraphQL mutation via `gh api graphql`.

Disable with `--no-priority`, or point at a differently-named field with
`--priority-field NAME`.

## The tk-id-must-not-leak rule

`tk` ticket ids are local-only and **must never appear** in a GitHub
issue, PR, branch, or commit. The command never writes the tk id into
the issue, and refuses to create one whose title or body contains the
source ticket's id **or any id present in the local store** (it also
warns on anything else shaped like a tk id). Instead, after creating
the issue it pins `external_ref: gh-<number>` onto the ticket, so the
link lives only on the private side.

Runs are **idempotent by default**: a ticket that already has an
`external_ref` is refused, since creating again would duplicate the
public issue and repoint the pin. Deliberate re-filing needs
`--re-file`; add `--no-pin` to keep the existing pin while doing so.

## Examples

Dry-run with the default (maintenance) field set:

```sh
tk publish github ab-1cd7 owner/infra-repo --dry-run
```

Create after the readability pass, assigned to the caller:

```sh
tk publish github ab-1cd7 owner/infra-repo \
    --body-file /tmp/polished-body.md --assignee @me
```

Target a repo with its own issue form:

```sh
tk publish github ab-1cd7 owner/other-repo \
    --fields-json ./other-repo-fields.json --title-prefix '[Bug]: '
```
