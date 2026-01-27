# Changelog

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
