# ticket

The git-backed issue tracker for AI agents. `tk` is inspired by Joe Armstrong's [Minimal Viable Program](https://joearms.github.io/published/2014-06-25-minimal-viable-program.html) with additional quality of life features for managing and querying against complex issue dependency graphs.

Tickets are markdown files with YAML frontmatter in `.tickets/`. `tk` will search upward from the current directory to find the nearest `.tickets/` (or respect `TICKETS_DIR` when set), so commands work from any subdirectory. This allows AI agents to easily search them for relevant content without dumping ten thousand character JSONL lines into their context window.

Using ticket IDs as file names also allows IDEs to quickly navigate to the ticket for you. For example, you might run `git log` in your terminal and see something like:

```text
nw-5c46: add SSE connection management
```

VS Code allows you to Ctrl+Click or Cmd+Click the ID and jump directly to the file to read the details.

## Install

**From source:**

```bash
git clone https://github.com/xymist/tkrs.git
cd tkrs && cargo install --path .
```

**From prebuilt binaries:**

Download the binary for your system from the [latest release](https://github.com/Xymist/tkrs/releases); unpack it, make it executable, and put it somewhere on your PATH

## Upgrade

Run `tk update`; if there is a new version it will download and unpack it over itself.

## Requirements

`tk` is a Rust binary with no system dependencies.

## Agent Setup

Add this line to your `CLAUDE.md` or `AGENTS.md`:

```text
This project uses a CLI ticket system for task management. Run `tk help` when you need to use it.
```

Claude Opus picks it up naturally from there. Other models may need additional guidance.

## Commands

- `tk create` — create a ticket, with optional fields
- `tk start|close|reopen|status` — set ticket status (all accept `--note`; close records `closed_at` automatically; reopen clears `closed_at` and can log a note)
- `tk dep|undep|link|unlink` — manage dependencies and links (use `tk dep cycle --include-closed` to scan closed tickets too; `tk dep --check-cycle <bool>` toggles the cycle guard, default `true`; `unlink` supports `--warn-missing`; `link` supports `--dry-run`; `undep` is idempotent and normalizes deps; `dep tree` supports `--status` and `--only-open`)
- `tk ls|list` — list tickets with filters (supports `--columns`, `--json`, and `--parent <id>` to scope to direct children of a parent; the `--json` shape always includes a `parents` array derived from reverse dependencies)
- `tk ready|blocked` — show tickets with dependency readiness (`ready` supports `--status` and `--show-deps`; `blocked` supports `--only-open`)
- `tk closed` — list recently closed tickets (supports `--limit`, `--since <RFC3339>`, `--assignee`, `--tags`)
- `tk show|edit|add-note` — inspect and edit tickets (`show` supports
  `--json` and lists derived `parents`; `edit` is non-interactive by
  default and accepts the same section flags as `create`
  (`-d/--description`, `--implementation-plan`, `--acceptance`,
  `--external-ref`, `--body-from-file`) to replace a section wholesale,
  or an empty string to clear it; pass `-i/--interactive` to launch
  `$EDITOR` instead (combine with update flags to apply them first);
  `--print` prints the ticket path; `add-note` is idempotent on headers
  and supports `--tag <label>`)
- `tk query [FILTER]` — output tickets as JSON; supports `--format ndjson|pretty` and built-in filters (`field==value` exact match, `field~substr` contains)
- `tk publish github <id> <owner/repo>` — render a ticket as a GitHub
  issue-form submission and file it with `gh issue create` (nested like
  `dep`, so other publish targets can be added later); supports
  `--assignee`, `--title-prefix` (default `"[Maintenance]: "`),
  `--fields-json` to replace the default field set, `--dry-run` to print
  the title/body/gh command without creating anything, `--body-file` to
  supply a pre-written body (title, pinning and priority still come from
  the ticket), `--no-pin`, `--re-file` to re-file an already-pinned
  ticket, `--no-priority`, `--priority-field` (default `Priority`), and
  repeatable `--label`. Best-effort sets the repo's single-select
  Priority issue field from the ticket's priority after creation. tk ids
  are never written to GitHub: the outbound title/body is checked against
  the ticket's own id and every id in the local store before anything is
  created, and an OS-level advisory file lock
  (`.tickets/.publish.lock`) is held across the whole check-create-pin
  sequence, so a second `tk publish` — even from a separate process —
  fails closed immediately instead of racing the first into
  double-creating an issue.
- `tk update` — self-update `tk` to the latest release published on GitHub, installing it over the running binary; prints `No new version available` when already current
- `tk tui` — EXPERIMENTAL - Start a TUI for browsing tickets with a little more context than just a bucket of IDs. Navigate with the keyboard (↑/↓ to move, →/← to expand/collapse, `Tab` to switch panes, `S` to cycle the status filter, `j`/`k`/PageUp/PageDown to scroll the content) or with the mouse (click a ticket to select it, click a pane to focus it, scroll wheel to scroll whichever pane the cursor is over).

## Agent skills

The repo ships Claude Code skills for agents under `skills/`:
`tk-cli` (CLI reference) and `tk-to-github-issue` (the workflow guide
for `tk publish github` — readability pass, field specs, leak rules).
To install one, symlink it into your personal skills directory so it
stays in lockstep with the checkout:

```sh
ln -s "$(pwd)/skills/tk-cli" ~/.claude/skills/tk-cli
ln -s "$(pwd)/skills/tk-to-github-issue" ~/.claude/skills/tk-to-github-issue
```

## Release workflow

Pushing a tag matching `vX.Y.Z` triggers the GitHub Actions workflow `.github/workflows/release.yml`. It first ensures a GitHub release exists for the tag, then builds the Rust binary in release mode for each supported target and publishes a versioned, per-target tarball as a release asset. The workflow can also be run manually (`workflow_dispatch`) by providing the tag to build.

Built targets and asset names:

- `tk-vX.Y.Z-x86_64-unknown-linux-musl.tar.gz` (Linux, x86_64; a fully static musl binary with no glibc dependency, so it runs on any x86_64 Linux distribution regardless of GLIBC version)
- `tk-vX.Y.Z-aarch64-apple-darwin.tar.gz` (macOS, Apple Silicon)

`tk update` self-installs by matching the running binary's target triple against these asset names, so each platform pulls its own build automatically.

## License

MIT
