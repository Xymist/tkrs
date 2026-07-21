# Changelog

## [0.10.1] - 2026-07-21

### Fixed

- `tk tree --root <id> --inverted` and `tk graph --root <id> --inverted`
  no longer collapse to almost nothing when `<id>` is an epic. `--root`
  now selects a *scope* -- the ticket plus its selection-filtered
  dependency closure, exactly what the normal-orientation `--root` walk
  reaches -- and `--inverted` only changes how that fixed scope is
  presented (leaf-first, ascending back to the root) instead of
  changing which tickets are in scope. A ticket outside the scope that
  happens to depend on one of its members is still excluded.
  Normal-orientation `--root` output is unchanged. Note the resulting
  behaviour change: `--root <leaf> --inverted` no longer means "every
  transitive dependant of `<leaf>`" -- it now means just `<leaf>`
  itself, since a leaf's own dependency closure is empty. `tk tree
  --root <id> --inverted` also renders a scope whose bottom is a
  dependency cycle instead of silently returning nothing, via a
  deterministic fallback root seeded from the cycle's own sink
  strongly-connected component -- a ticket merely upstream of the
  cycle is never wrongly promoted to a second, standalone root of its
  own; it is always nested beneath the cycle instead.

## [0.10.0] - 2026-07-21

### Added

- `tk tree --root <id>` (`-r`) restricts output to the single subtree
  rooted at that ticket (partial IDs accepted), walked in the
  requested orientation; `--status` still applies to the root itself,
  so a root that fails the filter yields empty output.
- `tk graph` prints the ticket dependency graph as a Mermaid
  `flowchart TD` to stdout, for sharing project plans with
  nontechnical stakeholders. It accepts the same `--status`,
  `--inverted`, and `--root` flags as `tk tree`, but unlike `tk tree`
  dedupes each ticket to a single node while keeping every
  selection-passing edge that reaches it, so a diamond-shaped
  dependency renders as one node with multiple incoming arrows.
  Unrestricted graphs guarantee every selection-matching ticket a
  node even when no root reaches it, so rootless dependency cycles
  still render. Output opens with a `%%{init: ...}%%` directive
  forcing straight, non-curved edges.

## [0.9.0] - 2026-07-20

### Added

- `tk tree` prints the full ticket tree, fully expanded, as plain text
  (`--status all|open|in-progress|closed`, default `open`), reusing the
  same forest-assembly logic as the `tk tui` tree pane so ordering and
  nesting stay in sync between the two.
- `tk tree --inverted` walks the reversed dependency graph: roots are
  leaf work items with no resolvable dependency, and each node nests
  its dependants, so indentation grows toward the epic(s) it feeds.

## [0.8.0] - 2026-07-20

### Added

- `tk publish github <id> <owner/repo>` renders a tk ticket as a GitHub
  issue-form submission and files it with `gh issue create`, natively
  replacing the `tk-to-github-issue` Python skill script for this repo's
  own use: `--assignee`, `--title-prefix`, `--fields-json`, `--dry-run`,
  `--body-file`, `--no-pin`, `--re-file`, `--no-priority`,
  `--priority-field`, and repeatable `--label` mirror the script's flags,
  and the default body byte-matches its output. Uses tk's own parsed
  ticket model instead of reparsing the file, and holds an OS-level
  advisory file lock (`.tickets/.publish.lock`) across the
  check-create-pin sequence, so a second `tk publish` invocation — even
  from a separate process — fails closed immediately instead of racing
  the first into double-creating an issue for the same ticket.

### Fixed

- Ticket writes are now atomic (write to a temp file, fsync, rename):
  a failed or interrupted write — out of disk space, I/O error, kill —
  can no longer truncate or corrupt the existing ticket file. This
  applies to every mutating command (`edit`, `close`, `add-note`,
  `dep`, `publish` pinning, ...).

### Removed

- The `tk_to_gh_issue.py` script inside the `tk-to-github-issue` skill:
  `tk publish github` replaces it natively. The skill remains as the
  workflow guide (readability pass, field specs, leak rules) for the
  subcommand, and the `tk-cli` skill documents the new command.

## [0.7.2] - 2026-07-20

### Added

- The `tk-to-github-issue` Claude Code skill now ships in-repo under
  `skills/tk-to-github-issue/`, converting a tk ticket into a GitHub
  issue rendered as an issue-form submission. Ported from the personal
  skills directory, made project-agnostic (no default assignee;
  `--title-prefix` and `--fields-json` adapt the body to any repo's
  issue form; the tk-id leak warning matches any id-shaped reference
  rather than a hardcoded prefix list), and validated against the
  v0.7.1 CLI: `external_ref` pinning now goes through
  `tk edit --external-ref` (direct frontmatter write only as a
  fallback) and the parser accepts the legacy `## Design` heading.
  Hardened per review: ids present in the local store hard-fail the
  publish, pinned tickets refuse re-filing without `--re-file`, and
  the frontmatter-write fallback runs only when `tk` is absent.

## [0.7.1] - 2026-07-20

### Added

- The `tk-cli` Claude Code agent skill now ships in-repo under
  `skills/tk-cli/`, documenting the CLI (including the v0.7.0
  non-interactive `tk edit`) for agents; install it by symlinking
  into `~/.claude/skills/` (see README).

## [0.7.0] - 2026-07-20

Accumulated changes from v0.5.0 through v0.7.0.

### Added

- `tk update` self-updates the binary to the latest GitHub release (via the `self_update` crate over a pure-Rust ureq/rustls stack), installing over the running executable; prints `No new version available` when already current. The command dispatches before the ticket cache refresh, so it never reads or creates a `.tickets/` directory.
- `tk ls --parent <id>` filters the listing to direct children of the given parent (partial IDs accepted).
- `tk ls --json` and `tk show --json` now emit a `parents` array (derived from reverse dependencies); `show` also prints a `Parents:` line. Parents are no longer a stored frontmatter field.
- `tk dep --check-cycle <bool>` is now a toggleable flag (default `true`); pass `--check-cycle false` to skip the cycle guard.
- `tk create --implementation-plan <text>` populates the implementation-plan section.
- TUI right-hand pane now shows the selected ticket's status, priority, and assignee in labelled boxes with merged borders across the top, above the content body.
- TUI now supports the mouse: click a ticket row to select it, click either pane to focus it, and use the scroll wheel to scroll whichever pane the cursor is over (mouse capture is enabled while the TUI is open and disabled on exit).
- `tk edit` accepts the same section-update flags as `create`
  (`-d/--description`, `--implementation-plan`, `--acceptance`,
  `--external-ref`, `--body-from-file`) to replace a ticket's fields
  non-interactively; passing an empty string clears the field.
  `--description` and `--body-from-file` combine the same way they do
  in `create`.
- `tk create`, `tk edit`, and `tk add-note` reject description,
  implementation-plan, acceptance, and note values that contain a line
  beginning with a reserved section heading (`## Implementation Plan`,
  `## Implementation plan`, `## Design`, `## Acceptance Criteria`,
  `## Notes`), since such a line would be mistaken for a real section
  delimiter and split the value across sections on the next read.
- `tk create` and `tk edit` reject a description, implementation-plan,
  or acceptance value that is exactly `-`, since that string is the
  reserved placeholder for an empty section; pass an empty string to
  clear a section instead.
- `tk create` requires a single-line title and `tk add-note --tag` a
  single-line tag; both occupy one structural line of the ticket file,
  so an embedded newline could smuggle in a section delimiter.

### Changed

- Renamed the `## Design` ticket section to `## Implementation Plan` (JSON key `implementation_plan`); `## Design` is still recognized on read for backward compatibility.
- Ticket files are now always written from the full standard template: the lead description and every section heading (`## Implementation Plan`, `## Acceptance Criteria`, `## Notes`) are always present, with a `-` placeholder for any section left empty. `TicketBody`'s `Display` impl is now the single source of truth for the body layout, shared by the on-disk file, `tk show`, and the TUI. A lone `-` is read back as empty, so placeholders round-trip and are replaced (not appended to) by later edits.
- `tk edit` is non-interactive by default; launching `$EDITOR`/`$VISUAL`
  now requires `-i/--interactive`. Running `tk edit <id>` with no flags
  is a usage error; combine `-i` with update flags to apply them before
  opening the editor.

### Fixed

- `reopen` now clears `closed_at`, and null `Option` frontmatter fields are omitted from serialized output.
- A cleared `external_ref` is now omitted from serialized frontmatter
  instead of writing `external_ref: null`, matching the other optional
  frontmatter fields.
- `tk edit --implementation-plan`/`--acceptance` and `tk create`'s
  equivalents now trim stored values, matching `--description`'s
  existing behaviour so all sections trim consistently.
- TUI no longer shows dependency tickets at the top level; tickets referenced as a dependency (at any nesting depth) now appear only nested under their parent.
- `tk show` no longer prints stray trailing whitespace after the `## Notes` section.
- Multiple notes no longer collapse into a single note when a ticket is re-read (the persisted note separator now matches the parser).

## [0.4.0] - 2026-01-27

### Added

- `list` command alias for `ls`
- t-2a9f: Validate create inputs, templates, and body-from-file (2026-01-27)
- t-6a4d: Closed listing with `--since` and deterministic ordering (2026-01-27)
- t-191e: Shared dep graph builder and `dep cycle --include-closed` (2026-01-27)
- t-403d: Start command via shared status helper with optional notes (2026-01-27)
- t-4206: Streamed migrate-beads import with validation and note preservation (2026-01-27)
- t-48f2: Close command writes `closed_at` and optional notes in one pass (2026-01-27)
- t-532c: `ls` stable priority/id ordering with `--columns` and `--json` (2026-01-27)
- t-690b: Idempotent unlink with shared link-set helper and warn-missing (2026-01-27)
- t-699a: Blocked uses cached metadata, `--only-open`, and sorted blockers (2026-01-27)
- t-b8f5: Fix title parsing so sections resolve correctly (2026-01-27)
- t-3b25: Require non-empty titles on ticket creation (2026-01-27)
- t-8f1d: Symmetric link management with in-memory sets and optional dry-run (2026-01-27)
- t-b55c: Streamed query output with `--format ndjson|pretty` and proper escaping (2026-01-27)
- t-b911: Safer dep adds with ambiguity checks and optional cycle guard rollback (2026-01-27)
- t-bb55: Respect VISUAL/EDITOR precedence for `edit`, add `--print`, and non-tty fallback (2026-01-27)
- t-bc30: Improve status validation, shared note appends, and idempotent updates (2026-01-27)
- t-c387: Add `tk show --json` with resolved parent/dependency/link metadata and body sections (2026-01-27)
- t-cea5: `ready` reuses cached metadata, adds `--status` and `--show-deps`, and sorts by priority/id (2026-01-27)
- t-ceb5: `undep` uses shared dep mutation, is idempotent, and normalizes empty deps (2026-01-27)
- t-dcc8: `add-note` avoids duplicate headers, normalizes newlines, and supports tagged notes (2026-01-27)
- t-eb47: `dep tree` adds status/only-open filters and uses cached graph with stable sorting (2026-01-27)
- t-f431: `reopen` uses shared status helper, clears closed_at, and supports optional notes (2026-01-27)
- tic-2797: GitHub Actions release workflow builds and publishes tagged binaries, packaging `tk` tarballs (2026-01-27)
- tic-9094: Release workflow switches to `gh release` and `$GITHUB_OUTPUT` (2026-01-27)

### Changed

- Walk parent directories to find `.tickets/` directory (or `TICKETS_DIR`), enabling commands from any subdirectory

### Fixed

- `dep` command now resolves partial IDs for the dependency argument
- t-75e3: `undep` resolves partial IDs and errors when dependency missing (2026-01-27)
- `unlink` command now resolves partial IDs for both arguments
- t-569e: `create --parent` resolves partial IDs and errors on missing/ambiguous (2026-01-27)
- t-9e4d: `generate_id` uses 3-char prefix for single-segment dirs (2026-01-27)

## [0.3.0] - 2026-01-18

### Added

- Support `TICKETS_DIR` environment variable for custom tickets directory location
- `dep cycle` command to detect dependency cycles in open tickets
- `add-note` command for appending timestamped notes to tickets
- `-a, --assignee` filter flag for `ls`, `ready`, `blocked`, and `closed` commands
- `--tags` flag for `create` command to add comma-separated tags
- `-T, --tag` filter flag for `ls`, `ready`, `blocked`, and `closed` commands

## [0.2.0] - 2026-01-04

### Added

- `--parent` flag for `create` command to set parent ticket
- `link`/`unlink` commands for symmetric ticket relationships
- `show` command displays parent title and linked tickets
- `migrate-beads` now imports parent-child and related dependencies

## [0.1.1] - 2026-01-02

### Fixed

- `edit` command no longer hangs when run in non-TTY environments

## [0.1.0] - 2026-01-02

Initial release.
