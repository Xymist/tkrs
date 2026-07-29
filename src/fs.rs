use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

use color_eyre::eyre::eyre;

use crate::{SECTION_PLACEHOLDER, Ticket, TicketBody, TicketSection};

pub static TICKETS: Mutex<Vec<Ticket>> = Mutex::new(Vec::new());

pub fn lock_tickets() -> color_eyre::Result<MutexGuard<'static, Vec<Ticket>>> {
    let mut tickets = TICKETS
        .try_lock()
        .map_err(|_| eyre!("Could not lock ticket store"))?;

    // Ensure tickets are always sorted when accessed
    tickets.sort_by(|a, b| {
        a.priority()
            .cmp(&b.priority())
            .then_with(|| a.id().cmp(b.id()))
    });

    Ok(tickets)
}

pub fn refresh_ticket_cache() -> color_eyre::Result<()> {
    let mut tickets = lock_tickets()?;
    *tickets = read_all_tickets().map_err(|e| eyre!("failed to read tickets: {e}"))?;
    Ok(())
}

pub fn read_all_tickets() -> color_eyre::Result<Vec<Ticket>> {
    let mut tickets = Vec::new();
    let dir = tickets_dir()?;
    if !dir.exists() {
        return Ok(tickets);
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        if let Some(ticket) = read_ticket(&path)? {
            tickets.push(ticket);
        }
    }

    reject_empty_ids(&tickets)?;
    reject_duplicate_ids(&tickets)?;

    Ok(tickets)
}

/// Every ticket must have a non-empty, non-whitespace-only frontmatter
/// `id:`. The forest and graph assembly's synthetic truncation marker node
/// uses the empty string as its own reserved id specifically because no
/// real ticket id is ever empty; enforcing that here, at load, is what
/// keeps the invariant real rather than merely documented. Checked before
/// [`reject_duplicate_ids`] so an empty id gets this more specific,
/// actionable message instead of being reported as a confusing "duplicate
/// ticket id ''" when more than one file has one.
fn reject_empty_ids(tickets: &[Ticket]) -> color_eyre::Result<()> {
    let mut offending: Vec<&Path> = tickets
        .iter()
        .filter(|t| t.id().trim().is_empty())
        .map(|t| t.path.as_path())
        .collect();
    if offending.is_empty() {
        return Ok(());
    }
    offending.sort();

    Err(eyre!(
        "empty ticket id in {}; fix the id: field so it is non-empty",
        join_with_and(&offending)
    ))
}

/// Every ticket id must be claimed by exactly one file: assembly (`src/tree.rs`
/// and the TUI) is deliberately tolerant of duplicate-id slices so it stays
/// usable as a library over arbitrary input, but a duplicate id in the store
/// itself is always a hand-editing mistake that makes `tree`/`graph`/`ls`
/// views diverge depending on which copy a given code path happens to
/// resolve to. Rejecting it here, once, at load time, means every command
/// fails fast with one clear error instead of silently producing a
/// different, wrong view per command.
fn reject_duplicate_ids(tickets: &[Ticket]) -> color_eyre::Result<()> {
    let mut paths_by_id: HashMap<&str, Vec<&Path>> = HashMap::new();
    for ticket in tickets {
        paths_by_id
            .entry(ticket.id())
            .or_default()
            .push(ticket.path.as_path());
    }

    let mut duplicates: Vec<(&str, Vec<&Path>)> = paths_by_id
        .into_iter()
        .filter(|(_, paths)| paths.len() > 1)
        .collect();
    if duplicates.is_empty() {
        return Ok(());
    }
    duplicates.sort_by_key(|(id, _)| *id);

    let mut message = String::new();
    for (id, mut paths) in duplicates {
        paths.sort();
        if !message.is_empty() {
            message.push('\n');
        }
        message.push_str(&format!(
            "duplicate ticket id '{id}' in {}; fix the id: field so each is unique",
            join_with_and(&paths)
        ));
    }

    Err(eyre!(message))
}

/// Joins `paths` for an error message: `"a"` for one, `"a and b"` for two,
/// `"a, b, and c"` for three or more.
fn join_with_and(paths: &[&Path]) -> String {
    let rendered: Vec<String> = paths.iter().map(|p| p.display().to_string()).collect();
    match rendered.as_slice() {
        [] => String::new(),
        [only] => only.clone(),
        [first, second] => format!("{first} and {second}"),
        [init @ .., last] => format!("{}, and {last}", init.join(", ")),
    }
}

fn section_value(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() || trimmed == SECTION_PLACEHOLDER {
        None
    } else {
        Some(trimmed.to_string())
    }
}

pub fn read_ticket(path: &Path) -> color_eyre::Result<Option<Ticket>> {
    let file_contents = std::fs::read_to_string(path)?;
    let (frontmatter, content) = file_contents
        .trim_start_matches("---")
        .split_once("---")
        .ok_or_else(|| {
            eyre!(
                "invalid ticket format in {}: missing frontmatter",
                path.display()
            )
        })?;
    let mut title = String::new();
    let mut section = TicketSection::Description;
    let mut description = String::new();
    let mut implementation_plan = String::new();
    let mut acceptance = String::new();
    let mut notes = String::new();

    for line in content.lines() {
        if title.is_empty()
            && let Some(rest) = line.strip_prefix("# ")
        {
            title = rest.to_string();
            continue;
        }

        if line.starts_with("## Implementation Plan")
            || line.starts_with("## Implementation plan")
            || line.starts_with("## Design")
        {
            section = TicketSection::ImplementationPlan;
            continue;
        } else if line.starts_with("## Acceptance Criteria") {
            section = TicketSection::Acceptance;
            continue;
        } else if line.starts_with("## Notes") {
            section = TicketSection::Notes;
            continue;
        }

        let target = match section {
            TicketSection::Description => &mut description,
            TicketSection::ImplementationPlan => &mut implementation_plan,
            TicketSection::Acceptance => &mut acceptance,
            TicketSection::Notes => &mut notes,
        };
        if !target.is_empty() || !line.trim().is_empty() {
            target.push('\n');
        }
        target.push_str(line);
    }

    let frontmatter = serde_yaml::from_str(frontmatter).map_err(|e| {
        eyre!(
            "invalid frontmatter format in {}: {}, {:?}",
            path.display(),
            e,
            frontmatter
        )
    })?;
    let body = TicketBody {
        description: section_value(&description),
        implementation_plan: section_value(&implementation_plan),
        acceptance: section_value(&acceptance),
        notes: section_value(&notes)
            .map(|n| {
                n.split("\n\n")
                    .map(|s| s.trim().to_string())
                    .collect::<Vec<String>>()
            })
            .unwrap_or_default(),
    };

    Ok(Some(Ticket {
        title: title.trim().to_string(),
        frontmatter,
        body,
        path: path.to_path_buf(),
    }))
}

pub fn write_ticket(ticket: &Ticket) -> color_eyre::Result<()> {
    let mut content = String::new();
    content.push_str(&ticket.frontmatter.as_yaml()?);
    content.push_str(&format!("# {}\n\n", ticket.title));
    content.push_str(&ticket.body().to_string());

    // Write-then-rename so a failed or interrupted write can never truncate
    // the existing ticket; the rename is atomic within the tickets dir.
    let file_name = ticket
        .path
        .file_name()
        .ok_or_else(|| eyre!("ticket path has no file name: {}", ticket.path.display()))?
        .to_string_lossy();
    let tmp_path = ticket
        .path
        .with_file_name(format!(".{file_name}.tmp-{}", std::process::id()));
    // Preserve the destination's permission semantics across the rename:
    // refuse read-only tickets (as an in-place write would), and carry the
    // original mode onto the replacement inode.
    let existing_perms = match std::fs::metadata(&ticket.path) {
        Ok(meta) => {
            let perms = meta.permissions();
            if perms.readonly() {
                return Err(eyre!("ticket file is read-only: {}", ticket.path.display()));
            }
            Some(perms)
        }
        Err(_) => None,
    };
    let write_and_rename = (|| -> std::io::Result<()> {
        use std::io::Write as _;
        let mut file = std::fs::File::create(&tmp_path)?;
        file.write_all(content.as_bytes())?;
        file.sync_all()?;
        if let Some(perms) = existing_perms {
            std::fs::set_permissions(&tmp_path, perms)?;
        }
        std::fs::rename(&tmp_path, &ticket.path)
    })();
    if write_and_rename.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    write_and_rename?;

    Ok(())
}

pub fn tickets_dir() -> color_eyre::Result<PathBuf> {
    let path = tickets_path()?;
    std::fs::create_dir_all(&path).map_err(|e| eyre!("failed to create tickets dir: {e}"))?;
    Ok(path)
}

pub fn tickets_path() -> color_eyre::Result<PathBuf> {
    if let Ok(env_path) = std::env::var("TICKETS_DIR") {
        let path = PathBuf::from(env_path);
        if path.is_absolute() {
            return Ok(path);
        }

        if let Ok(cwd) = std::env::current_dir() {
            return Ok(cwd.join(path));
        }

        return Ok(path);
    }

    let cwd = match std::env::current_dir() {
        Ok(d) => d,
        Err(_) => return Ok(PathBuf::from(".tickets")),
    };

    // Read $HOME directly rather than via std::env::home_dir(), which falls
    // back to the passwd database when HOME is unset/empty; an unset,
    // empty, or relative $HOME is treated the same as no home directory.
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|h| h.is_absolute());
    resolve_tickets_path(&cwd, home.as_deref())
}

fn is_repo_root(dir: &Path) -> bool {
    // A git worktree or submodule stores `.git` as a file, not a directory.
    dir.join(".git").exists() || dir.join(".jj").is_dir()
}

/// Sanitizes `name` into a lowercase, dash-separated store key, or `None`
/// when nothing alphanumeric survives.
fn store_key(name: &str) -> Option<String> {
    let mut key = String::with_capacity(name.len());
    let mut last_was_dash = false;
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() {
            key.push(ch.to_ascii_lowercase());
            last_was_dash = false;
        } else if !last_was_dash {
            key.push('-');
            last_was_dash = true;
        }
    }
    let key = key.trim_matches('-');

    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

/// Reads the committed `.tk-store` override at the main repo root: genuinely
/// absent, or present but sanitizing to nothing, falls through to `Ok(None)`
/// so the caller uses the basename key instead; anything structurally
/// broken (a directory, a dangling or looping symlink, a permission error,
/// or unreadable content) is a hard error naming the marker, since silently
/// ignoring it would land tickets in the wrong, colliding store.
fn store_key_override(main: &Path) -> color_eyre::Result<Option<String>> {
    let marker = main.join(".tk-store");
    if let Err(e) = std::fs::symlink_metadata(&marker) {
        return if e.kind() == std::io::ErrorKind::NotFound {
            Ok(None)
        } else {
            Err(eyre!(
                "failed to stat store marker {}: {e}",
                marker.display()
            ))
        };
    }
    // Follow a possible symlink so a marker symlinked to a regular file
    // still works, while a dangling/looping symlink or a directory fails.
    let is_regular_file = std::fs::metadata(&marker)
        .map_err(|e| eyre!("store marker {} is unusable: {e}", marker.display()))?
        .is_file();
    if !is_regular_file {
        return Err(eyre!(
            "store marker {} is not a regular file",
            marker.display()
        ));
    }

    let contents = std::fs::read_to_string(&marker)
        .map_err(|e| eyre!("failed to read store marker {}: {e}", marker.display()))?;
    Ok(contents
        .lines()
        .next()
        .and_then(|line| store_key(line.trim())))
}

/// Lexically normalizes `path`, resolving `.` and `..` components without
/// touching the filesystem or following symlinks.
fn normalize_path(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                result.pop();
            }
            Component::CurDir => {}
            other => result.push(other.as_os_str()),
        }
    }
    result
}

/// Resolves `pointer` (a path read from a marker file) against `base` when
/// relative, then lexically normalizes the result.
fn resolve_pointer(base: &Path, pointer: &str) -> PathBuf {
    let pointer_path = Path::new(pointer);
    normalize_path(&if pointer_path.is_absolute() {
        pointer_path.to_path_buf()
    } else {
        base.join(pointer_path)
    })
}

/// Resolves a git worktree's shared repository root via its `commondir`
/// file, the pointer git itself uses to find the data every worktree
/// shares. This covers `--separate-git-dir` and bare-repo layouts that a
/// `.git/worktrees/<name>` string match would miss; a submodule's gitdir
/// has no `commondir` file, so submodules are excluded naturally. When the
/// shared root is a `.git` directory, the main root is its parent;
/// otherwise (a bare repository) the shared root is the main root itself.
/// Returns `None` when `dir` is not a linked worktree, or the gitdir or
/// commondir chain is missing, dangling, or malformed.
fn git_worktree_main_root(dir: &Path) -> Option<PathBuf> {
    let git_file = dir.join(".git");
    if !git_file.is_file() {
        return None;
    }
    let contents = std::fs::read_to_string(&git_file).ok()?;
    let gitdir = contents.lines().next()?.strip_prefix("gitdir: ")?.trim();
    if gitdir.is_empty() {
        return None;
    }
    let gitdir_path = resolve_pointer(dir, gitdir);

    let commondir_contents = std::fs::read_to_string(gitdir_path.join("commondir")).ok()?;
    let commondir = commondir_contents.lines().next()?.trim();
    if commondir.is_empty() {
        return None;
    }
    let common = resolve_pointer(&gitdir_path, commondir);

    if common.file_name().is_some_and(|name| name == ".git") {
        common.parent().map(Path::to_path_buf)
    } else {
        Some(common)
    }
}

/// Resolves the main repo root for a jj workspace whose `.jj/repo` is the
/// pointer-file form (`jj workspace add`), rather than the default
/// workspace's `.jj/repo` directory. Returns `None` when `dir` is not a
/// secondary workspace, or the referenced repo directory doesn't exist.
fn jj_workspace_main_root(dir: &Path) -> Option<PathBuf> {
    let repo_pointer = dir.join(".jj").join("repo");
    if !repo_pointer.is_file() {
        return None;
    }
    let contents = std::fs::read_to_string(&repo_pointer).ok()?;
    let referenced = contents.lines().next()?.trim();
    if referenced.is_empty() {
        return None;
    }
    let resolved = resolve_pointer(&dir.join(".jj"), referenced);
    if !resolved.is_dir() {
        return None;
    }

    resolved.parent()?.parent().map(Path::to_path_buf)
}

/// Resolves the root of the main repo that `dir` belongs to, so a linked
/// git worktree or a secondary jj workspace shares its ticket store with
/// the repo it was created from. Returns `dir` itself when neither
/// indirection applies.
fn main_repo_root(dir: &Path) -> PathBuf {
    git_worktree_main_root(dir)
        .or_else(|| jj_workspace_main_root(dir))
        .unwrap_or_else(|| dir.to_path_buf())
}

/// Resolves the ticket store for `cwd` in precedence order: a repo-local
/// `.tickets/` found while walking up to the repo root; the main repo's
/// local `.tickets/` when `dir` is a worktree or workspace of one; the
/// `.tk-store` marker key at the main root; the main root's basename key;
/// and finally a fresh `.tickets/` at the main repo root (so a linked
/// worktree and its main checkout still converge on one store even when no
/// home-fallback key is available). `home` itself is never treated as a
/// repo-local store, since `~/.tickets` is the container every
/// home-fallback store lives under, not a store in its own right.
fn resolve_tickets_path(cwd: &Path, home: Option<&Path>) -> color_eyre::Result<PathBuf> {
    let mut dir = cwd;

    loop {
        let candidate = dir.join(".tickets");
        if home != Some(dir) && candidate.is_dir() {
            return Ok(candidate);
        }

        if is_repo_root(dir) {
            let main = main_repo_root(dir);
            if main != dir {
                let main_store = main.join(".tickets");
                if main_store.is_dir() {
                    return Ok(main_store);
                }
            }

            let Some(home) = home else {
                return Ok(main.join(".tickets"));
            };
            let key = store_key_override(&main)?
                .or_else(|| store_key(&main.file_name().unwrap_or_default().to_string_lossy()));
            let Some(key) = key else {
                return Ok(main.join(".tickets"));
            };
            return Ok(home.join(".tickets").join(key));
        }

        match dir.parent() {
            Some(parent) => dir = parent,
            None => break,
        }
    }

    Ok(PathBuf::from(".tickets"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TicketFrontmatter, cli::StatusValue};
    use assert_fs::{TempDir, prelude::*};

    fn ticket_at(id: &str, path: &str) -> Ticket {
        Ticket {
            title: format!("Title {id}"),
            frontmatter: TicketFrontmatter {
                id: id.to_string(),
                r#type: None,
                status: StatusValue::Open,
                deps: Vec::new(),
                links: Vec::new(),
                priority: 2,
                assignee: None,
                tags: Vec::new(),
                created: None,
                closed_at: None,
                external_ref: None,
            },
            body: TicketBody {
                description: None,
                implementation_plan: None,
                acceptance: None,
                notes: Vec::new(),
            },
            path: PathBuf::from(path),
        }
    }

    #[test]
    fn reject_duplicate_ids_allows_a_store_with_unique_ids() {
        let tickets = vec![ticket_at("a", "a.md"), ticket_at("b", "b.md")];
        assert!(reject_duplicate_ids(&tickets).is_ok());
    }

    #[test]
    fn reject_duplicate_ids_names_the_id_and_both_paths() {
        let tickets = vec![
            ticket_at("x-1234", ".tickets/a.md"),
            ticket_at("x-1234", ".tickets/b.md"),
        ];
        let err = reject_duplicate_ids(&tickets).unwrap_err().to_string();
        assert!(err.contains("x-1234"));
        assert!(err.contains(".tickets/a.md"));
        assert!(err.contains(".tickets/b.md"));
    }

    #[test]
    fn reject_duplicate_ids_joins_three_paths_with_commas_and_and() {
        let tickets = vec![
            ticket_at("x-1234", ".tickets/b.md"),
            ticket_at("x-1234", ".tickets/a.md"),
            ticket_at("x-1234", ".tickets/c.md"),
        ];
        let err = reject_duplicate_ids(&tickets).unwrap_err().to_string();
        assert!(
            err.contains(".tickets/a.md, .tickets/b.md, and .tickets/c.md"),
            "three paths must be sorted and comma-joined with a final 'and': {err}"
        );
    }

    #[test]
    fn reject_duplicate_ids_lists_every_duplicated_id_in_one_error() {
        let tickets = vec![
            ticket_at("x-1111", "a.md"),
            ticket_at("x-1111", "b.md"),
            ticket_at("y-2222", "c.md"),
            ticket_at("y-2222", "d.md"),
        ];
        let err = reject_duplicate_ids(&tickets).unwrap_err().to_string();
        assert!(err.contains("x-1111"));
        assert!(err.contains("y-2222"));
        assert!(err.contains("a.md"));
        assert!(err.contains("b.md"));
        assert!(err.contains("c.md"));
        assert!(err.contains("d.md"));
    }

    #[test]
    fn reject_empty_ids_allows_a_store_with_non_empty_ids() {
        let tickets = vec![ticket_at("a", "a.md"), ticket_at("b", "b.md")];
        assert!(reject_empty_ids(&tickets).is_ok());
    }

    #[test]
    fn reject_empty_ids_names_the_file_for_an_empty_id() {
        let tickets = vec![ticket_at("", ".tickets/empty.md")];
        let err = reject_empty_ids(&tickets).unwrap_err().to_string();
        assert!(err.contains(".tickets/empty.md"));
        assert!(err.contains("empty"));
    }

    #[test]
    fn reject_empty_ids_treats_whitespace_only_as_empty() {
        let tickets = vec![ticket_at("   ", ".tickets/blank.md")];
        let err = reject_empty_ids(&tickets).unwrap_err().to_string();
        assert!(err.contains(".tickets/blank.md"));
    }

    #[test]
    fn reject_empty_ids_lists_every_offending_file_in_one_error() {
        let tickets = vec![
            ticket_at("", "b.md"),
            ticket_at("", "a.md"),
            ticket_at("x-1234", "c.md"),
        ];
        let err = reject_empty_ids(&tickets).unwrap_err().to_string();
        assert!(err.contains("a.md and b.md"), "sorted, both listed: {err}");
        assert!(
            !err.contains("c.md"),
            "the non-empty id must not be flagged"
        );
    }

    const WORKTREE_GIT_FILE: &str = "gitdir: /elsewhere/.git/worktrees/repo\n";

    fn make_dirs(temp: &TempDir, paths: &[&str]) {
        for path in paths {
            temp.child(path).create_dir_all().unwrap();
        }
    }

    /// Builds a linked git worktree the way git does: a `.git` pointer file in
    /// the checkout, and the per-worktree gitdir holding the `commondir` file
    /// that points back at the shared `.git` directory.
    fn make_git_worktree(temp: &TempDir, main: &str, checkout: &str) {
        let gitdir = temp.child(format!("{main}/.git/worktrees/{checkout}"));
        gitdir.create_dir_all().unwrap();
        gitdir.child("commondir").write_str("../..\n").unwrap();
        temp.child(format!("{checkout}/.git"))
            .write_str(&format!("gitdir: {}\n", gitdir.path().display()))
            .unwrap();
    }

    /// The spelling `jj` uses for a secondary workspace's `.jj/repo` file: the
    /// bare path of the main workspace's repo dir, with no line terminator.
    fn jj_workspace_pointer(main: &Path) -> String {
        format!("{}/.jj/repo", main.display())
    }

    fn home_store(home: &TempDir, key: &str) -> PathBuf {
        home.path().join(".tickets").join(key)
    }

    /// Strips every permission bit from `path`. Reports `false` when the file
    /// stays readable anyway, which is what happens under a uid that bypasses
    /// permission checks; callers bail out rather than assert.
    #[cfg(unix)]
    fn make_unreadable(path: &Path) -> bool {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o000)).unwrap();
        std::fs::read_to_string(path).is_err()
    }

    #[cfg(not(unix))]
    fn make_unreadable(_path: &Path) -> bool {
        false
    }

    // MC/DC for: dir.join(".git").exists() || dir.join(".jj").is_dir()
    //   c1 = a `.git` entry exists (file or directory)
    //   c2 = `.jj` exists and is a directory
    // Independence pairs, named by the test functions in this module:
    //   c1: t1_a_git_directory_alone(T,-)=true
    //       vs t2_neither_marker(F,F)=false (c2 masked by short-circuit in t1)
    //   c2: t3_a_jj_directory_alone(F,T)=true vs t2_neither_marker(F,F)=false
    // t4_both_markers(T,T)=true covers both disjuncts true; the two file-form
    // cases pin which filesystem predicate each condition uses.
    mod is_repo_root {
        use super::*;

        #[test]
        fn t1_a_git_directory_alone_is_a_repo_root() {
            let temp = TempDir::new().unwrap();
            make_dirs(&temp, &[".git"]);

            assert!(!temp.child(".jj").path().exists());
            assert!(is_repo_root(temp.path()));
        }

        #[test]
        fn t2_neither_marker_is_not_a_repo_root() {
            let temp = TempDir::new().unwrap();
            make_dirs(&temp, &["src"]);

            assert!(!is_repo_root(temp.path()));
        }

        #[test]
        fn t3_a_jj_directory_alone_is_a_repo_root() {
            let temp = TempDir::new().unwrap();
            make_dirs(&temp, &[".jj"]);

            assert!(!temp.child(".git").path().exists());
            assert!(is_repo_root(temp.path()));
        }

        #[test]
        fn t4_both_markers_is_a_repo_root() {
            let temp = TempDir::new().unwrap();
            temp.child(".git").write_str(WORKTREE_GIT_FILE).unwrap();
            make_dirs(&temp, &[".jj"]);

            assert!(is_repo_root(temp.path()));
        }

        #[test]
        fn a_git_file_from_a_worktree_is_a_repo_root() {
            let temp = TempDir::new().unwrap();
            temp.child(".git").write_str(WORKTREE_GIT_FILE).unwrap();

            assert!(is_repo_root(temp.path()));
        }

        #[test]
        fn a_jj_file_without_git_is_not_a_repo_root() {
            let temp = TempDir::new().unwrap();
            temp.child(".jj").write_str("not a jj repo\n").unwrap();

            assert!(!is_repo_root(temp.path()));
        }
    }

    // MC/DC for the walk's local-store check:
    //   `home != Some(dir) && candidate.is_dir()`
    //   c1 = dir is not the home directory itself
    //   c2 = dir/.tickets exists and is a directory
    // Independence pairs, named by the test functions in this module:
    //   c1: returns_a_store_in_the_cwd(T,T)=returns it
    //       vs never_returns_the_home_container_store(F,-)=skips it
    //       (c2 masked by short-circuit)
    //   c2: returns_a_store_in_the_cwd(T,T)=returns it
    //       vs falls_back_to_home_for_a_git_repo_without_a_store(T,F)=skips it
    //
    // MC/DC for the repo-root arm's shared-store check, spelled as nested ifs
    // over `main != dir` then `main/.tickets is a dir`:
    //   c3 = the repo root is a worktree/workspace of a different main root
    //   c4 = that main root has a local .tickets directory
    //   c3: uses_the_main_repos_local_store_from_a_worktree(T,T)=main store
    //       vs falls_back_to_home_for_a_git_repo_without_a_store(F,-)=falls
    //       through (c4 masked: the inner if is not reached)
    //   c4: uses_the_main_repos_local_store_from_a_worktree(T,T)=main store
    //       vs falls_back_to_home_for_a_git_file_worktree(T,F)=falls through
    mod resolve_tickets_path {
        use super::*;

        fn resolve(cwd: &Path, home: Option<&Path>) -> PathBuf {
            resolve_tickets_path(cwd, home).unwrap()
        }

        #[test]
        fn returns_a_store_in_the_cwd() {
            let temp = TempDir::new().unwrap();
            let home = TempDir::new().unwrap();
            make_dirs(&temp, &["repo/.tickets"]);

            assert_eq!(
                resolve(temp.child("repo").path(), Some(home.path())),
                temp.child("repo/.tickets").path()
            );
        }

        #[test]
        fn prefers_an_ancestor_store_over_the_home_fallback() {
            let temp = TempDir::new().unwrap();
            let home = TempDir::new().unwrap();
            make_dirs(&temp, &["repo/.git", "repo/.tickets", "repo/sub/dir"]);

            let resolved = resolve(temp.child("repo/sub/dir").path(), Some(home.path()));

            assert_eq!(resolved, temp.child("repo/.tickets").path());
            assert!(!resolved.starts_with(home.path()));
        }

        #[test]
        fn never_returns_the_home_container_store() {
            let temp = TempDir::new().unwrap();
            make_dirs(&temp, &[".tickets", "plain/sub"]);

            let resolved = resolve(temp.child("plain/sub").path(), Some(temp.path()));

            assert_ne!(
                resolved,
                temp.child(".tickets").path(),
                "~/.tickets is the container for home-fallback stores, not a store"
            );
        }

        #[test]
        fn falls_back_to_home_for_a_git_repo_without_a_store() {
            let temp = TempDir::new().unwrap();
            let home = TempDir::new().unwrap();
            make_dirs(&temp, &["repo/.git", "repo/nested/deep"]);

            assert_eq!(
                resolve(temp.child("repo/nested/deep").path(), Some(home.path())),
                home_store(&home, "repo")
            );
        }

        #[test]
        fn falls_back_to_home_for_a_git_file_worktree() {
            let temp = TempDir::new().unwrap();
            let home = TempDir::new().unwrap();
            make_git_worktree(&temp, "main", "wt");
            make_dirs(&temp, &["wt/nested"]);

            let resolved = resolve(temp.child("wt/nested").path(), Some(home.path()));

            assert!(!temp.child("main/.tickets").path().exists());
            assert_eq!(resolved, home_store(&home, "main"));
            assert_ne!(resolved, home_store(&home, "wt"));
        }

        #[test]
        fn uses_the_main_repos_local_store_from_a_worktree() {
            let temp = TempDir::new().unwrap();
            let home = TempDir::new().unwrap();
            make_git_worktree(&temp, "main", "wt");
            make_dirs(&temp, &["main/.tickets", "wt/nested"]);

            assert_eq!(
                resolve(temp.child("wt/nested").path(), Some(home.path())),
                temp.child("main/.tickets").path()
            );
        }

        #[test]
        fn keys_a_dangling_worktree_pointer_by_its_own_basename() {
            let temp = TempDir::new().unwrap();
            let home = TempDir::new().unwrap();
            make_dirs(&temp, &["repo/nested"]);
            temp.child("repo/.git")
                .write_str(WORKTREE_GIT_FILE)
                .unwrap();

            assert_eq!(
                resolve(temp.child("repo/nested").path(), Some(home.path())),
                home_store(&home, "repo"),
                "an unresolvable pointer must not key the store by its target"
            );
        }

        #[test]
        fn falls_back_to_home_for_a_jj_only_repo() {
            let temp = TempDir::new().unwrap();
            let home = TempDir::new().unwrap();
            make_dirs(&temp, &["repo/.jj", "repo/nested"]);

            assert_eq!(
                resolve(temp.child("repo/nested").path(), Some(home.path())),
                home_store(&home, "repo")
            );
        }

        #[test]
        fn falls_back_to_home_for_a_repo_with_both_markers() {
            let temp = TempDir::new().unwrap();
            let home = TempDir::new().unwrap();
            make_git_worktree(&temp, "main", "wt");
            make_dirs(&temp, &["wt/.jj", "wt/nested"]);

            assert_eq!(
                resolve(temp.child("wt/nested").path(), Some(home.path())),
                home_store(&home, "main")
            );
        }

        #[test]
        fn shares_the_store_between_a_git_worktree_and_its_main_repo() {
            let temp = TempDir::new().unwrap();
            let home = TempDir::new().unwrap();
            make_git_worktree(&temp, "main", "wt");

            let from_worktree = resolve(temp.child("wt").path(), Some(home.path()));
            let from_main = resolve(temp.child("main").path(), Some(home.path()));

            assert_eq!(from_worktree, from_main);
            assert_eq!(from_main, home_store(&home, "main"));
        }

        #[test]
        fn shares_the_store_between_a_jj_workspace_and_its_main_repo() {
            let temp = TempDir::new().unwrap();
            let home = TempDir::new().unwrap();
            let main = temp.child("main");
            make_dirs(&temp, &["main/.jj/repo", "ws/.jj"]);
            temp.child("ws/.jj/repo")
                .write_str(&jj_workspace_pointer(main.path()))
                .unwrap();

            let from_workspace = resolve(temp.child("ws").path(), Some(home.path()));
            let from_main = resolve(main.path(), Some(home.path()));

            assert_eq!(from_workspace, from_main);
            assert_eq!(from_main, home_store(&home, "main"));
        }

        /// Keys deliberately encode only the basename so a synced `~/.tickets`
        /// resolves to the same store on machines that check the repo out at
        /// different paths; `.tk-store` is the escape hatch for the collision
        /// this admits.
        #[test]
        fn shares_a_key_between_same_named_repos_without_a_marker() {
            let temp = TempDir::new().unwrap();
            let home = TempDir::new().unwrap();
            make_dirs(&temp, &["a/proj/.git", "b/proj/.git"]);

            let first = resolve(temp.child("a/proj").path(), Some(home.path()));
            let second = resolve(temp.child("b/proj").path(), Some(home.path()));

            assert_eq!(first, second);
            assert_eq!(first, home_store(&home, "proj"));
        }

        #[test]
        fn keys_colliding_repos_apart_with_a_tk_store_marker() {
            let temp = TempDir::new().unwrap();
            let home = TempDir::new().unwrap();
            make_dirs(&temp, &["a/proj/.git", "b/proj/.git"]);
            temp.child("b/proj/.tk-store")
                .write_str("Proj Beta\n")
                .unwrap();

            let first = resolve(temp.child("a/proj").path(), Some(home.path()));
            let second = resolve(temp.child("b/proj").path(), Some(home.path()));

            assert_ne!(first, second);
            assert_eq!(first, home_store(&home, "proj"));
            assert_eq!(second, home_store(&home, "proj-beta"));
        }

        #[test]
        fn gives_a_worktree_the_main_repos_marker_key() {
            let temp = TempDir::new().unwrap();
            let home = TempDir::new().unwrap();
            make_git_worktree(&temp, "main", "wt");
            temp.child("main/.tk-store")
                .write_str("Shared Store\n")
                .unwrap();

            assert_eq!(
                resolve(temp.child("wt").path(), Some(home.path())),
                home_store(&home, "shared-store")
            );
        }

        #[test]
        fn ignores_a_marker_committed_in_a_worktree_checkout() {
            let temp = TempDir::new().unwrap();
            let home = TempDir::new().unwrap();
            make_git_worktree(&temp, "main", "wt");
            temp.child("wt/.tk-store").write_str("checkout\n").unwrap();

            assert_eq!(
                resolve(temp.child("wt").path(), Some(home.path())),
                home_store(&home, "main"),
                "only the main root's marker may name the shared store"
            );
        }

        #[test]
        fn falls_back_to_the_basename_for_a_blank_marker() {
            let temp = TempDir::new().unwrap();
            let home = TempDir::new().unwrap();
            make_dirs(&temp, &["repo/.git"]);
            temp.child("repo/.tk-store").write_str("   \n").unwrap();

            assert_eq!(
                resolve(temp.child("repo").path(), Some(home.path())),
                home_store(&home, "repo")
            );
        }

        #[test]
        fn propagates_an_unreadable_marker_as_an_error() {
            let temp = TempDir::new().unwrap();
            let home = TempDir::new().unwrap();
            make_dirs(&temp, &["repo/.git"]);
            let marker = temp.child("repo/.tk-store");
            marker.write_str("blocked\n").unwrap();
            if !make_unreadable(marker.path()) {
                return;
            }

            let err = resolve_tickets_path(temp.child("repo").path(), Some(home.path()))
                .unwrap_err()
                .to_string();

            assert!(
                err.contains(".tk-store"),
                "error must name the marker: {err}"
            );
        }

        #[test]
        fn ignores_a_store_above_the_repo_root() {
            let temp = TempDir::new().unwrap();
            let home = TempDir::new().unwrap();
            make_dirs(&temp, &[".tickets", "repo/.git", "repo/sub"]);

            let resolved = resolve(temp.child("repo/sub").path(), Some(home.path()));

            assert_eq!(resolved, home_store(&home, "repo"));
            assert_ne!(resolved, temp.child(".tickets").path());
        }

        #[test]
        fn uses_the_repo_root_when_home_is_unknown() {
            let temp = TempDir::new().unwrap();
            make_dirs(&temp, &["repo/.git", "repo/sub"]);

            assert_eq!(
                resolve(temp.child("repo/sub").path(), None),
                temp.child("repo/.tickets").path()
            );
        }

        #[test]
        fn uses_the_repo_root_when_the_basename_has_no_key() {
            let temp = TempDir::new().unwrap();
            let home = TempDir::new().unwrap();
            make_dirs(&temp, &["___/.git"]);

            assert_eq!(
                resolve(temp.child("___").path(), Some(home.path())),
                temp.child("___/.tickets").path()
            );
        }

        #[test]
        fn converges_on_the_main_root_when_home_is_unknown() {
            let temp = TempDir::new().unwrap();
            make_git_worktree(&temp, "main", "wt");

            let resolved = resolve(temp.child("wt").path(), None);

            assert_eq!(resolved, temp.child("main/.tickets").path());
            assert_ne!(
                resolved,
                temp.child("wt/.tickets").path(),
                "a worktree must not start a store of its own"
            );
        }

        #[test]
        fn converges_on_the_main_root_when_the_basename_has_no_key() {
            let temp = TempDir::new().unwrap();
            let home = TempDir::new().unwrap();
            make_git_worktree(&temp, "___", "wt");

            let resolved = resolve(temp.child("wt").path(), Some(home.path()));

            assert_eq!(resolved, temp.child("___/.tickets").path());
            assert_ne!(resolved, temp.child("wt/.tickets").path());
        }
    }

    mod store_key {
        use super::*;

        #[test]
        fn lowercases_and_substitutes_non_alphanumerics() {
            assert_eq!(
                store_key("My Repo.v2"),
                Some("my-repo-v2".to_string()),
                "digits survive, case folds, `.` and space become dashes"
            );
        }

        #[test]
        fn collapses_runs_of_substituted_characters() {
            assert_eq!(store_key("My   Repo!!x"), Some("my-repo-x".to_string()));
        }

        #[test]
        fn trims_leading_and_trailing_dashes() {
            assert_eq!(store_key("_hidden_"), Some("hidden".to_string()));
        }

        #[test]
        fn substitutes_non_ascii_characters() {
            assert_eq!(store_key("naïve"), Some("na-ve".to_string()));
        }

        #[test]
        fn returns_none_for_an_empty_name() {
            assert_eq!(store_key(""), None);
        }

        #[test]
        fn returns_none_for_whitespace_only() {
            assert_eq!(store_key("   "), None);
        }

        #[test]
        fn returns_none_when_no_character_is_alphanumeric() {
            assert_eq!(store_key("___"), None);
        }
    }

    mod store_key_override {
        use super::*;

        #[test]
        fn a_marker_at_the_main_root_names_the_store() {
            let temp = TempDir::new().unwrap();
            let main = temp.child("main");
            make_dirs(&temp, &["main"]);
            main.child(".tk-store").write_str("Shared Store\n").unwrap();

            assert_eq!(
                store_key_override(main.path()).unwrap(),
                Some("shared-store".to_string())
            );
        }

        #[test]
        fn a_missing_marker_has_no_override() {
            let temp = TempDir::new().unwrap();
            make_dirs(&temp, &["repo"]);

            assert_eq!(store_key_override(temp.child("repo").path()).unwrap(), None);
        }

        #[test]
        fn a_checkout_level_marker_is_not_consulted() {
            let temp = TempDir::new().unwrap();
            make_dirs(&temp, &["main"]);
            temp.child("wt/.tk-store").write_str("checkout\n").unwrap();

            assert_eq!(store_key_override(temp.child("main").path()).unwrap(), None);
        }

        #[test]
        fn surrounding_whitespace_is_trimmed() {
            let temp = TempDir::new().unwrap();
            let repo = temp.child("repo");
            repo.child(".tk-store").write_str("  spaced  \n").unwrap();

            assert_eq!(
                store_key_override(repo.path()).unwrap(),
                Some("spaced".to_string())
            );
        }

        #[test]
        fn only_the_first_line_is_used() {
            let temp = TempDir::new().unwrap();
            let repo = temp.child("repo");
            repo.child(".tk-store")
                .write_str("first\nsecond\n")
                .unwrap();

            assert_eq!(
                store_key_override(repo.path()).unwrap(),
                Some("first".to_string())
            );
        }

        #[test]
        fn the_marker_content_is_sanitized() {
            let temp = TempDir::new().unwrap();
            let repo = temp.child("repo");
            repo.child(".tk-store").write_str("My Store!\n").unwrap();

            assert_eq!(
                store_key_override(repo.path()).unwrap(),
                Some("my-store".to_string())
            );
        }

        #[test]
        fn an_empty_marker_has_no_override() {
            let temp = TempDir::new().unwrap();
            let repo = temp.child("repo");
            repo.child(".tk-store").write_str("").unwrap();

            assert_eq!(store_key_override(repo.path()).unwrap(), None);
        }

        #[test]
        fn a_whitespace_only_marker_has_no_override() {
            let temp = TempDir::new().unwrap();
            let repo = temp.child("repo");
            repo.child(".tk-store").write_str(" \t \n").unwrap();

            assert_eq!(store_key_override(repo.path()).unwrap(), None);
        }

        #[test]
        fn a_tk_store_directory_is_an_error() {
            let temp = TempDir::new().unwrap();
            make_dirs(&temp, &["repo/.tk-store"]);

            let err = store_key_override(temp.child("repo").path())
                .unwrap_err()
                .to_string();

            assert!(
                err.contains(".tk-store"),
                "error must name the marker: {err}"
            );
        }

        #[cfg(unix)]
        #[test]
        fn a_symlink_to_a_readable_file_is_followed() {
            let temp = TempDir::new().unwrap();
            let repo = temp.child("repo");
            let target = repo.child("store-name");
            target.write_str("Linked Store\n").unwrap();
            std::os::unix::fs::symlink(target.path(), repo.child(".tk-store").path()).unwrap();

            assert_eq!(
                store_key_override(repo.path()).unwrap(),
                Some("linked-store".to_string())
            );
        }

        #[cfg(unix)]
        #[test]
        fn a_dangling_symlink_is_an_error() {
            let temp = TempDir::new().unwrap();
            let repo = temp.child("repo");
            make_dirs(&temp, &["repo"]);
            let missing = repo.child("gone");
            std::os::unix::fs::symlink(missing.path(), repo.child(".tk-store").path()).unwrap();

            assert!(!missing.path().exists());
            let err = store_key_override(repo.path()).unwrap_err().to_string();

            assert!(
                err.contains(".tk-store"),
                "error must name the marker: {err}"
            );
        }

        #[test]
        fn an_unreadable_marker_is_an_error() {
            let temp = TempDir::new().unwrap();
            let repo = temp.child("repo");
            let marker = repo.child(".tk-store");
            marker.write_str("blocked\n").unwrap();
            if !make_unreadable(marker.path()) {
                return;
            }

            let err = store_key_override(repo.path()).unwrap_err().to_string();

            assert!(
                err.contains(".tk-store"),
                "error must name the marker: {err}"
            );
        }

        #[test]
        fn a_non_utf8_marker_is_an_error() {
            let temp = TempDir::new().unwrap();
            let repo = temp.child("repo");
            repo.child(".tk-store")
                .write_binary(&[0xff, 0xfe, b'\n'])
                .unwrap();

            let err = store_key_override(repo.path()).unwrap_err().to_string();

            assert!(
                err.contains(".tk-store"),
                "error must name the marker: {err}"
            );
        }
    }

    mod normalize_path {
        use super::*;

        #[test]
        fn leaves_a_clean_absolute_path_unchanged() {
            assert_eq!(normalize_path(Path::new("/a/b/c")), PathBuf::from("/a/b/c"));
        }

        #[test]
        fn elides_current_dir_components() {
            assert_eq!(
                normalize_path(Path::new("/a/./b/./c")),
                PathBuf::from("/a/b/c")
            );
        }

        #[test]
        fn resolves_parent_dir_components() {
            assert_eq!(
                normalize_path(Path::new("/a/b/../c")),
                PathBuf::from("/a/c")
            );
        }

        #[test]
        fn parent_dir_past_a_relative_start_yields_an_empty_path() {
            assert_eq!(normalize_path(Path::new("../..")), PathBuf::new());
        }

        #[test]
        fn parent_dir_cannot_climb_above_the_filesystem_root() {
            assert_eq!(normalize_path(Path::new("/../../a")), PathBuf::from("/a"));
        }
    }

    // MC/DC for the shared-root classification:
    //   `common.file_name().is_some_and(|name| name == ".git")`
    //   c1 = the resolved common dir has a final component
    //   c2 = that component is `.git`
    // Independence pairs, named by the test functions in this module:
    //   c1: an_absolute_gitdir_resolves_to_the_main_repo_root(T,T)=parent
    //       vs a_commondir_at_the_filesystem_root_is_its_own_main_root(F,-)
    //       =the common dir itself (c2 masked: never evaluated)
    //   c2: an_absolute_gitdir_resolves_to_the_main_repo_root(T,T)=parent
    //       vs a_bare_repository_is_its_own_main_root(T,F)=the common dir
    //       itself
    mod git_worktree_main_root {
        use super::*;

        #[test]
        fn an_absolute_gitdir_resolves_to_the_main_repo_root() {
            let temp = TempDir::new().unwrap();
            make_git_worktree(&temp, "main", "wt");

            assert_eq!(
                git_worktree_main_root(temp.child("wt").path()),
                Some(temp.child("main").path().to_path_buf())
            );
        }

        #[test]
        fn a_relative_gitdir_resolves_against_the_worktree_dir() {
            let temp = TempDir::new().unwrap();
            make_dirs(&temp, &["main/.git/worktrees/wt", "wt"]);
            temp.child("main/.git/worktrees/wt/commondir")
                .write_str("../..\n")
                .unwrap();
            temp.child("wt/.git")
                .write_str("gitdir: ../main/.git/worktrees/wt\n")
                .unwrap();

            assert_eq!(
                git_worktree_main_root(temp.child("wt").path()),
                Some(temp.child("main").path().to_path_buf())
            );
        }

        #[test]
        fn a_bare_repository_is_its_own_main_root() {
            let temp = TempDir::new().unwrap();
            let bare = temp.child("proj.git");
            make_dirs(&temp, &["proj.git/worktrees/wt", "wt"]);
            temp.child("proj.git/worktrees/wt/commondir")
                .write_str("../..\n")
                .unwrap();
            temp.child("wt/.git")
                .write_str(&format!("gitdir: {}/worktrees/wt\n", bare.path().display()))
                .unwrap();

            assert_eq!(
                git_worktree_main_root(temp.child("wt").path()),
                Some(bare.path().to_path_buf()),
                "a bare repo has no `.git` component to strip"
            );
        }

        #[test]
        fn a_commondir_at_the_filesystem_root_is_its_own_main_root() {
            let temp = TempDir::new().unwrap();
            let gitdir = temp.child("gitdir");
            gitdir.create_dir_all().unwrap();
            gitdir.child("commondir").write_str("/\n").unwrap();
            temp.child("wt/.git")
                .write_str(&format!("gitdir: {}\n", gitdir.path().display()))
                .unwrap();

            assert_eq!(
                git_worktree_main_root(temp.child("wt").path()),
                Some(PathBuf::from("/"))
            );
        }

        #[test]
        fn a_submodule_gitdir_without_a_commondir_is_not_a_worktree() {
            let temp = TempDir::new().unwrap();
            let main = temp.child("main");
            make_dirs(&temp, &["main/.git/modules/sub", "main/sub"]);
            temp.child("main/sub/.git")
                .write_str(&format!(
                    "gitdir: {}/.git/modules/sub\n",
                    main.path().display()
                ))
                .unwrap();

            assert_eq!(git_worktree_main_root(temp.child("main/sub").path()), None);
        }

        #[test]
        fn a_gitdir_without_a_commondir_file_is_not_a_worktree() {
            let temp = TempDir::new().unwrap();
            make_dirs(&temp, &["main/.git/worktrees/wt", "wt"]);
            temp.child("wt/.git")
                .write_str(&format!(
                    "gitdir: {}/.git/worktrees/wt\n",
                    temp.child("main").path().display()
                ))
                .unwrap();

            assert_eq!(git_worktree_main_root(temp.child("wt").path()), None);
        }

        #[test]
        fn an_empty_commondir_file_is_not_a_worktree() {
            let temp = TempDir::new().unwrap();
            make_dirs(&temp, &["main/.git/worktrees/wt", "wt"]);
            temp.child("main/.git/worktrees/wt/commondir")
                .write_str("\n")
                .unwrap();
            temp.child("wt/.git")
                .write_str(&format!(
                    "gitdir: {}/.git/worktrees/wt\n",
                    temp.child("main").path().display()
                ))
                .unwrap();

            assert_eq!(git_worktree_main_root(temp.child("wt").path()), None);
        }

        #[test]
        fn a_dangling_gitdir_is_not_a_worktree() {
            let temp = TempDir::new().unwrap();
            temp.child("wt/.git").write_str(WORKTREE_GIT_FILE).unwrap();

            assert_eq!(git_worktree_main_root(temp.child("wt").path()), None);
        }

        #[test]
        fn a_git_directory_is_not_a_worktree_pointer() {
            let temp = TempDir::new().unwrap();
            make_dirs(&temp, &["repo/.git"]);

            assert_eq!(git_worktree_main_root(temp.child("repo").path()), None);
        }

        #[test]
        fn a_missing_git_entry_is_not_a_worktree_pointer() {
            let temp = TempDir::new().unwrap();
            make_dirs(&temp, &["plain"]);

            assert_eq!(git_worktree_main_root(temp.child("plain").path()), None);
        }

        #[test]
        fn an_empty_git_file_is_not_a_worktree_pointer() {
            let temp = TempDir::new().unwrap();
            temp.child("wt/.git").write_str("").unwrap();

            assert_eq!(git_worktree_main_root(temp.child("wt").path()), None);
        }

        #[test]
        fn a_git_file_without_the_gitdir_prefix_is_not_a_worktree_pointer() {
            let temp = TempDir::new().unwrap();
            temp.child("wt/.git").write_str("not a pointer\n").unwrap();

            assert_eq!(git_worktree_main_root(temp.child("wt").path()), None);
        }

        #[test]
        fn an_empty_gitdir_value_is_not_a_worktree_pointer() {
            let temp = TempDir::new().unwrap();
            temp.child("wt/.git").write_str("gitdir: \n").unwrap();

            assert_eq!(git_worktree_main_root(temp.child("wt").path()), None);
        }
    }

    mod jj_workspace_main_root {
        use super::*;

        #[test]
        fn an_absolute_repo_pointer_resolves_to_the_main_workspace_root() {
            let temp = TempDir::new().unwrap();
            let main = temp.child("main");
            make_dirs(&temp, &["main/.jj/repo", "ws/.jj"]);
            temp.child("ws/.jj/repo")
                .write_str(&jj_workspace_pointer(main.path()))
                .unwrap();

            assert_eq!(
                jj_workspace_main_root(temp.child("ws").path()),
                Some(main.path().to_path_buf())
            );
        }

        #[test]
        fn a_relative_repo_pointer_resolves_against_the_jj_dir() {
            let temp = TempDir::new().unwrap();
            make_dirs(&temp, &["main/.jj/repo", "ws/.jj"]);
            temp.child("ws/.jj/repo")
                .write_str("../../main/.jj/repo")
                .unwrap();

            assert_eq!(
                jj_workspace_main_root(temp.child("ws").path()),
                Some(temp.child("main").path().to_path_buf())
            );
        }

        #[test]
        fn a_repo_directory_is_the_main_workspace_and_has_no_pointer() {
            let temp = TempDir::new().unwrap();
            make_dirs(&temp, &["main/.jj/repo"]);

            assert_eq!(jj_workspace_main_root(temp.child("main").path()), None);
        }

        #[test]
        fn a_missing_jj_dir_has_no_pointer() {
            let temp = TempDir::new().unwrap();
            make_dirs(&temp, &["plain"]);

            assert_eq!(jj_workspace_main_root(temp.child("plain").path()), None);
        }

        #[test]
        fn an_empty_repo_pointer_is_ignored() {
            let temp = TempDir::new().unwrap();
            temp.child("ws/.jj/repo").write_str("\n").unwrap();

            assert_eq!(jj_workspace_main_root(temp.child("ws").path()), None);
        }

        #[test]
        fn a_dangling_repo_pointer_is_ignored() {
            let temp = TempDir::new().unwrap();
            let missing = temp.child("gone/.jj/repo");
            temp.child("ws/.jj/repo")
                .write_str(&missing.path().display().to_string())
                .unwrap();

            assert!(!missing.path().exists());
            assert_eq!(jj_workspace_main_root(temp.child("ws").path()), None);
        }

        #[test]
        fn a_repo_pointer_at_the_filesystem_root_is_ignored() {
            let temp = TempDir::new().unwrap();
            temp.child("ws/.jj/repo").write_str("/").unwrap();

            assert_eq!(jj_workspace_main_root(temp.child("ws").path()), None);
        }
    }

    mod main_repo_root {
        use super::*;

        #[test]
        fn a_git_worktree_pointer_wins_over_a_jj_workspace_pointer() {
            let temp = TempDir::new().unwrap();
            let git_main = temp.child("git-main");
            let jj_main = temp.child("jj-main");
            make_git_worktree(&temp, "git-main", "wt");
            make_dirs(&temp, &["jj-main/.jj/repo", "wt/.jj"]);
            temp.child("wt/.jj/repo")
                .write_str(&jj_workspace_pointer(jj_main.path()))
                .unwrap();

            assert_eq!(
                main_repo_root(temp.child("wt").path()),
                git_main.path().to_path_buf()
            );
        }

        #[test]
        fn a_jj_workspace_pointer_is_used_when_there_is_no_git_pointer() {
            let temp = TempDir::new().unwrap();
            let jj_main = temp.child("jj-main");
            make_dirs(&temp, &["jj-main/.jj/repo", "ws/.jj"]);
            temp.child("ws/.jj/repo")
                .write_str(&jj_workspace_pointer(jj_main.path()))
                .unwrap();

            assert_eq!(
                main_repo_root(temp.child("ws").path()),
                jj_main.path().to_path_buf()
            );
        }

        #[test]
        fn a_plain_repo_root_is_its_own_main_root() {
            let temp = TempDir::new().unwrap();
            make_dirs(&temp, &["repo/.git"]);

            assert_eq!(
                main_repo_root(temp.child("repo").path()),
                temp.child("repo").path().to_path_buf()
            );
        }
    }
}
