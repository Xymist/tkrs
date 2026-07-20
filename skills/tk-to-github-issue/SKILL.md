---
name: tk-to-github-issue
description: Convert a local tk ticket into a GitHub issue rendered as an issue-form submission (default field set is a maintenance-ticket form; adapt to any repo's form with --fields-json) and open it in a repo chosen at call time. Use when the user asks to file/raise/open a GitHub issue from a tk ticket, publish a tk ticket to GitHub, or push security/maintenance tickets to a GitHub repo. Handles the tk-id-must-not-leak rule and pins external_ref back onto the ticket automatically.
---

# tk → GitHub issue

Takes a local `tk` ticket and creates a GitHub issue whose body is
rendered the way GitHub renders an **issue form** submission: one
`### <field label>` heading per field, value beneath. The default
field set is a conventional maintenance-ticket form; when the target
repo has its own issue form, pass `--fields-json` so the body uses
that form's labels and parses as a submission of it.

## When to use

The user wants a `tk` ticket published as a GitHub issue — e.g. "raise
a GitHub issue for ab-1cd7", "file these security tickets in
owner/infra-repo", "open auc-5699f on GitHub". The script owns
creation, field mapping, pinning, and priority — never compose an
issue outside it. You own the **readability pass** (below): the script
renders the raw body, you polish the prose, and `--body-file` feeds it
back.

## Usage

```
python3 ~/.claude/skills/tk-to-github-issue/tk_to_gh_issue.py \
    <ticket-id> <owner/repo> [options]
```

- `<ticket-id>` — a ticket in a reachable `.tickets/` store.
- `<owner/repo>` — target GitHub repo.

Options:

| flag | default | meaning |
|---|---|---|
| `--assignee NAME` | none | GitHub assignee (`@me` for yourself) |
| `--title-prefix STR` | `[Maintenance]: ` | issue title prefix |
| `--fields-json PATH` | built-in maintenance form | replace the body field set (see "Field mapping") |
| `--tickets-root DIR` | auto-detect upward from CWD | repo root holding `.tickets/` |
| `--dry-run` | off | print title + body + the `gh` command + intended priority; create nothing |
| `--body-file PATH` | off | use this file's contents as the issue body instead of rendering one from the ticket (title, pinning and priority still come from the ticket) |
| `--no-pin` | off | do not write `external_ref` back to the ticket |
| `--re-file` | off | create another issue for an already-pinned ticket (refused otherwise) |
| `--no-priority` | off | do not set the repo Priority issue field |
| `--priority-field NAME` | `Priority` | name of the single-select issue field to set |
| `--label LABEL` | none | add a label (repeatable) |

**`--tickets-root` matters across stores.** Each repo keeps its own
local `.tickets/` store with its own id prefix. The script auto-detects
by walking up from the current directory, so a ticket belonging to a
*different* repo's store will not be found from here — pass
`--tickets-root <path-to-that-repo>` (or run from inside it).

**Always `--dry-run` first** to render the raw body, then perform the
readability pass below and create the issue with `--body-file`.

## Readability pass (required)

tk tickets are written for agents and often arrive as dense
single-block prose. GitHub issues are read by humans, so the
ticket-derived text must be reformatted before the issue is created.
The standard flow:

1. Run the script with `--dry-run`. The body is only the `###`
   sections of the output — the `# would create...`, `TITLE:`,
   `# gh command...`, and `# would set Priority...` lines are dry-run
   chrome and must not reach the scratch file.
2. Rewrite the ticket-derived sections (the description field and,
   when populated from the ticket, the acceptance criteria) per the
   rules below. Save the full polished body — every `###` section,
   nothing else — to a scratch file.
3. Re-run the script with `--body-file <scratch-file>` to create the
   issue. Title, `external_ref` pinning, and priority still come from
   the ticket, so all other flags work as normal.

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
  id and describe the ticket in words. The script refuses to create a
  body containing the source ticket's id and warns on anything else
  shaped like one.
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
|---|---|
| title | `[Maintenance]: <ticket title>` (prefix via `--title-prefix`) |
| Describe the change required | ticket intro **+ implementation plan** |
| List the success/acceptance criteria for this ticket | ticket `## Acceptance Criteria` |
| How many hours of developer time has this lost us? | `1` (fixed) |
| What happens if we don't do this? | standard default text (tk has no such field) |
| How easy is this to change or fix? | `Unsure` (fixed) |
| Which module(s) would making this change affect? | `Unsure/Other` (fixed) |

To target a different issue form, pass `--fields-json <path>` with a
JSON array replacing the whole set. Each entry is either a fixed value
or ticket-derived content:

```json
[
  {"label": "What needs to change?", "source": "describe"},
  {"label": "Definition of done", "source": "acceptance"},
  {"label": "Severity", "value": "Unknown"}
]
```

`source: describe` is the ticket intro plus its implementation plan;
`source: acceptance` is the ticket's acceptance criteria. Labels must
match the target repo's form field labels **byte-for-byte** (GitHub
only parses the body as a form submission when the headings match),
so read the repo's `.github/ISSUE_TEMPLATE/*.yml` first and copy the
`label:` values exactly.

The implementation plan is pulled from the ticket's `## Implementation
Plan` section when populated, and falls back to `## Notes` for tickets
whose plan was appended via `tk add-note` (a common convention before
`tk edit` gained non-interactive section flags in v0.7.0; going
forward, prefer `tk edit <id> --implementation-plan` so the plan lives
in its own section). Section parsing anchors only on tk's canonical
headings (`Implementation Plan`, `Acceptance Criteria`, `Notes`, plus
the legacy `Design` alias for the plan), so `##`/`###` headings
*inside* a plan body are treated as content, not as new sections.

## Priority (issue-fields preview)

After the issue is created, the script sets the repo's **`Priority`**
single-select issue field (GitHub's repository issue-fields preview — see
<https://github.com/orgs/community/discussions/189141>) from the ticket's tk
priority:

| tk priority | Priority option |
|---|---|
| P0 (`priority: 0`) | Urgent |
| P1 (`priority: 1`) | High |
| P2 (`priority: 2`) | Medium |
| P3 (`priority: 3`) | Low |

Option matching is case/emoji-insensitive, so a configured option like
`🌋 Urgent` still matches. This is **best-effort and never fails the run** — if
the repo has no `Priority` issue field, the mapped option name isn't present,
the ticket has no priority, or the token lacks permission to set fields, the
script prints a note/warning and continues (the issue is already created).
Setting the field needs a token that can write issue fields; reads/writes use
the `setIssueFieldValue` GraphQL mutation via `gh api graphql`.

Disable with `--no-priority`, or point at a differently-named field with
`--priority-field NAME`.

## The tk-id-must-not-leak rule

`tk` ticket ids are local-only and **must never appear** in a GitHub
issue, PR, branch, or commit. The script never writes the tk id into
the issue, and refuses to create one whose title or body contains the
source ticket's id (it also warns on anything else shaped like a tk
id, e.g. a cross-reference surviving in ticket prose or a
`--body-file`). Instead, after creating the issue it pins the ticket
via `tk edit <id> --external-ref gh-<number>` (writing the frontmatter
directly only if `tk` is not on the PATH), so the link lives only on
the private side.

Runs are **idempotent by default**: a ticket that already has an
`external_ref` is refused, since creating again would duplicate the
public issue and repoint the pin. Deliberate re-filing needs
`--re-file`; add `--no-pin` to keep the existing pin while doing so.

## Examples

Dry-run with the default (maintenance) field set:

```
python3 ~/.claude/skills/tk-to-github-issue/tk_to_gh_issue.py \
    ab-1cd7 owner/infra-repo --dry-run
```

Create an issue for a ticket from another repo's store, assigned to
the caller:

```
python3 ~/.claude/skills/tk-to-github-issue/tk_to_gh_issue.py \
    app-5699f owner/app-repo \
    --tickets-root /path/to/app-repo --assignee @me
```

Target a repo with its own issue form:

```
python3 ~/.claude/skills/tk-to-github-issue/tk_to_gh_issue.py \
    ab-1cd7 owner/other-repo \
    --fields-json ./other-repo-fields.json --title-prefix '[Bug]: '
```
