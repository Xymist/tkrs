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
- `tk tree` — print the full ticket tree, fully expanded, as plain
  text (same ordering and nesting as the `tk tui` tree pane, no
  colour); supports `-s/--status all|open|in-progress|closed` (default
  `open`); dependencies are omitted from the top level and nest under
  their dependants instead — a shared dependency repeats once under
  every root that depends on it, *and* once per branch within a single
  root too (a diamond dependency shows up under both branches that
  reach it, not just the first); every selection-matching ticket is
  guaranteed to appear somewhere, cycles included — a pure dependency
  cycle, or a ticket whose only dependant is filtered out (e.g. an
  open dependency of a closed ticket under the default open view),
  still renders as its own root instead of silently vanishing;
  `--inverted` walks the reversed graph instead — roots are leaf work
  items with no resolvable dependency, and each node nests the tickets
  that depend on it, so an epic reachable from several leaves appears
  once per path (the same completeness guarantee applies here too,
  e.g. an open epic whose only resolvable dependency is a closed leaf
  still renders);
  `-r/--root <id>` restricts output to a single ticket's *scope* — that
  ticket plus its selection-filtered dependency closure, exactly the
  set the normal-orientation walk reaches (partial IDs accepted;
  `--status` still applies to the root itself, so a root that fails
  the filter yields empty output); `--inverted` only changes how that
  fixed scope is presented — leaf-first, ascending back to the root —
  rather than changing which tickets are in scope, so a ticket outside
  the scope that happens to depend on one of its members never appears.
  Output repeats a ticket once per distinct dependency path reaching
  it, so on a densely diamond-shaped dependency graph the tree can
  grow rapidly; assembly is capped at 10000 rendered nodes per
  invocation (`tk tui` uses the same cap to protect redraws), and
  a `[!] output truncated at 10000 nodes ...` marker is appended as
  the last top-level entry when the cap is hit — pass `--unbounded`
  to lift it, or prefer `tk graph`, which dedupes each ticket to a
  single node and needs no cap
- `tk graph` — print the ticket graph as a Mermaid `flowchart TD` to
  stdout, for sharing project plans with nontechnical stakeholders;
  accepts the same `-s/--status`, `--inverted`, and `-r/--root` flags
  as `tk tree` and applies them identically (including `--root`'s
  scope-then-orientation semantics above), but unlike `tk tree` each
  ticket is emitted as a single deduped node while every
  selection-passing dependency edge reaching it is still recorded (so
  a diamond-shaped dependency shows up as one node with two incoming
  arrows); without `--root`, every selection-matching ticket is
  guaranteed a node even when no root reaches it, so rootless
  dependency cycles still render; edges point
  `dependant --> dependency` by default and
  `dependency --> dependant` under `--inverted`; the output opens with
  a `%%{init: ...}%%` directive forcing straight (non-curved) edges
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

The repo ships `tk-cli` (CLI reference) and `tk-to-github-issue` (the
workflow guide for `tk publish github` — readability pass, field specs,
leak rules) for both Claude Code and Codex. Claude Code skills live under
`skills/claude/`; Codex-native equivalents, including Codex UI metadata and
workflow idioms, live under `skills/codex/`.

Run the installer to symlink each pair into any existing personal skills
directory. It leaves correct links in place and refuses to overwrite conflicts,
so the installed skills stay in lockstep with the checkout.

```sh
./skills/install.sh
```

## Release workflow

Pushing a tag matching `vX.Y.Z` triggers the GitHub Actions workflow `.github/workflows/release.yml`. It first ensures a GitHub release exists for the tag, then builds the Rust binary in release mode for each supported target and publishes a versioned, per-target tarball as a release asset. The workflow can also be run manually (`workflow_dispatch`) by providing the tag to build.

Built targets and asset names:

- `tk-vX.Y.Z-x86_64-unknown-linux-musl.tar.gz` (Linux, x86_64; a fully static musl binary with no glibc dependency, so it runs on any x86_64 Linux distribution regardless of GLIBC version)
- `tk-vX.Y.Z-aarch64-apple-darwin.tar.gz` (macOS, Apple Silicon)

`tk update` self-installs by matching the running binary's target triple against these asset names, so each platform pulls its own build automatically.

## License

MIT
