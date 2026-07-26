---
name: tk-to-github-issue
description: Publish a local `tk` ticket as a polished GitHub issue with `tk publish github`, preserving issue-form fields and pinning the created issue as the ticket's external reference. Use when Codex is asked to file, open, raise, publish, or push a tk ticket to GitHub. Covers dry-run review, human-readable reformatting, custom issue-form fields, local ticket-ID leak prevention, idempotency, and priority mapping.
---

# Publish a `tk` ticket to GitHub

Use `tk publish github`; do not compose the issue separately with
`gh issue create`. The command owns creation, validation, priority mapping, and
`external_ref` pinning. Codex owns the required readability pass.

Publishing changes external state. Proceed when the user asked to create or
publish the issue. If the request is only to inspect, draft, or preview it,
stop after the dry run and return the rendered result or scratch-file path.

The ticket store is discovered upward from the current directory. To publish
from another checkout, run inside that checkout or set `TICKETS_DIR` to its
`.tickets/` directory. The `<owner/repo>` argument selects only the GitHub
target; it does not select the local ticket store.

## Follow the publish workflow

1. Inspect the source ticket:

   ```sh
   tk show <id>
   ```

2. Inspect the target repository's issue form when it has one. With authenticated
   GitHub access, read `.github/ISSUE_TEMPLATE/*.yml` and copy each `label:`
   byte-for-byte into a fields JSON file.

3. Always render before creating:

   ```sh
   tk publish github <id> <owner/repo> [applicable options] --dry-run
   ```

   Include `--fields-json` here when step 2 produced a custom field set, and
   preserve the same rendering options for the final publish.

4. Extract only the issue body. Exclude dry-run chrome such as
   `# would create`, `TITLE:`, the shown `gh` command, and priority notes.

5. Reformat ticket-derived prose for human readers while preserving every form
   heading, fixed value, qualifier, and constraint. Save the complete polished
   body to a scratch file outside the repository when practical.

6. Review the title and body for local ticket IDs. Resolve any cross-reference
   to a pinned `gh-<number>` reference or describe the work without its local
   ID.

7. Publish with the polished body:

   ```sh
   tk publish github <id> <owner/repo> \
     --body-file /tmp/polished-body.md
   ```

8. Verify the command reported the created issue and, unless `--no-pin` was
   requested, pinned `external_ref: gh-<number>` on the local ticket:

   ```sh
   tk show <id> --json
   ```

Do not claim success if issue creation was not confirmed, or if required local
pinning was not confirmed.

## Perform the readability pass

Preserve the issue-form structure:

- Keep every `### <field label>` heading byte-identical and in the same order.
- Leave fixed-value fields exactly as rendered.
- Preserve meaning; reformat without inventing claims or dropping constraints.
- Put file paths, symbols, code expressions, config keys, and constants in
  backticks.
- Convert bare URLs to descriptive Markdown links.
- Use `#NNNN` for issues or pull requests in the same repository.
- Split wall-of-text prose into one-idea paragraphs.
- Turn inline enumerations and multi-part fixes into real lists.
- Use short bold run-in headings for distinct defects when useful.

Skip the rewrite only when the dry-run body is already short, clear, and
properly formatted. In that case, rerun the same command without `--dry-run`.

## Match custom issue forms

The default fields target a conventional maintenance form. For another form,
write a JSON array:

```json
[
  {"label": "What needs to change?", "source": "describe"},
  {"label": "Definition of done", "source": "acceptance"},
  {"label": "Severity", "value": "Unknown"}
]
```

Then render and publish with:

```sh
tk publish github <id> <owner/repo> \
  --fields-json /tmp/fields.json \
  --dry-run
```

`source: describe` uses the ticket introduction and implementation plan,
falling back to notes when needed. `source: acceptance` uses acceptance
criteria. GitHub recognizes an issue-form submission only when the headings
match the form labels exactly.

## Use supported options

Run `tk publish github --help` for the installed binary's authoritative list.
Common options:

```text
--assignee <name>          Assign the issue; @me means the caller
--title-prefix <text>      Default: [Maintenance]:
--fields-json <path>       Replace the default form field set
--dry-run                  Render without creating
--body-file <path>         Use a reviewed body
--no-pin                   Do not update the local external_ref
--re-file                  Permit an already-pinned ticket to be filed again
--no-priority              Skip Priority issue-field mapping
--priority-field <name>    Default: Priority
--label <label>            Add a label; repeatable
```

Priority mapping is best-effort:

| tk priority | GitHub option |
| --- | --- |
| P0 | Urgent |
| P1 | High |
| P2 | Medium |
| P3 | Low |

A missing field, option, ticket priority, or permission does not undo a
successfully created issue. Report that partial outcome precisely.

## Enforce local-ID privacy and idempotency

Local `tk` IDs must not leak into a GitHub title, body, branch, pull request, or
commit. The publish command rejects the source ticket ID and any ID found in
the local store, and warns on other ID-shaped strings.

By default, a ticket with an existing `external_ref` is refused to prevent a
duplicate issue and accidental repinning. Use `--re-file` only when the user
explicitly intends to create another issue. Add `--no-pin` when preserving the
existing local reference is also required.

Publishes are serialized per ticket store. If another publish holds
`.tickets/.publish.lock`, report the conflict and retry only when the user's
request calls for waiting or monitoring.
