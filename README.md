# ticket

The git-backed issue tracker for AI agents. Rooted in the Unix Philosophy, `tk` is inspired by Joe Armstrong's [Minimal Viable Program](https://joearms.github.io/published/2014-06-25-minimal-viable-program.html) with additional quality of life features for managing and querying against complex issue dependency graphs.

`tk` was written as a full replacement for [beads](https://github.com/steveyegge/beads). It shares many similar commands but without the need for keeping a SQLite file in sync or a rogue background daemon mangling your changes. It ships with a `migrate-beads` command to make this a smooth transition.

Tickets are markdown files with YAML frontmatter in `.tickets/`. `tk` will search upward from the current directory to find the nearest `.tickets/` (or respect `TICKETS_DIR` when set), so commands work from any subdirectory. This allows AI agents to easily search them for relevant content without dumping ten thousand character JSONL lines into their context window.

Using ticket IDs as file names also allows IDEs to quickly navigate to the ticket for you. For example, you might run `git log` in your terminal and see something like:

```text
nw-5c46: add SSE connection management
```

VS Code allows you to Ctrl+Click or Cmd+Click the ID and jump directly to the file to read the details.

## Install

**Homebrew (macOS/Linux):**

```bash
brew tap wedow/tools
brew install ticket
```

**Arch Linux (AUR):**

```bash
yay -S ticket  # or paru, etc.
```

**From source (auto-updates on git pull):**

```bash
git clone https://github.com/wedow/ticket.git
cd ticket && ln -s "$PWD/ticket" ~/.local/bin/tk
```

**Or** just copy `ticket` to somewhere in your PATH.

## Requirements

`tk` is a Rust binary with no system dependencies.

## Agent Setup

Add this line to your `CLAUDE.md` or `AGENTS.md`:

```text
This project uses a CLI ticket system for task management. Run `tk help` when you need to use it.
```

Claude Opus picks it up naturally from there. Other models may need additional guidance.

## Commands

- `tk create` — create a ticket with optional fields
- `tk start|close|reopen|status` — set ticket status (all accept `--note`; close records `closed_at` automatically; reopen clears `closed_at` and can log a note)
- `tk dep|undep|link|unlink` — manage dependencies and links (use `tk dep cycle --include-closed` to scan closed tickets too; `unlink` supports `--warn-missing`; `link` supports `--dry-run`; `undep` is idempotent and normalizes deps; `dep tree` supports `--status` and `--only-open`)
- `tk ls` — list tickets with filters (supports `--columns` and `--json`)
- `tk ready|blocked` — show tickets with dependency readiness (`ready` supports `--status` and `--show-deps`; `blocked` supports `--only-open`)
- `tk closed` — list recently closed tickets (supports `--limit`, `--since <RFC3339>`, `--assignee`, `--tags`)
- `tk show|edit|add-note` — inspect and edit tickets (`show` supports `--json`; `add-note` is idempotent on headers and supports `--tag <label>`)
- `tk query [FILTER]` — output tickets as JSON; supports `--format ndjson|pretty` and built-in filters (`field==value` exact match, `field~substr` contains)
- `tk migrate-beads` — import beads issues from `.beads/issues.jsonl`

## License

MIT
