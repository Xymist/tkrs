use std::{
    collections::HashMap,
    path::{Path, PathBuf},
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
    let path = tickets_path();
    std::fs::create_dir_all(&path).map_err(|e| eyre!("failed to create tickets dir: {e}"))?;
    Ok(path)
}

pub fn tickets_path() -> PathBuf {
    if let Ok(env_path) = std::env::var("TICKETS_DIR") {
        let path = PathBuf::from(env_path);
        if path.is_absolute() {
            return path;
        }

        if let Ok(cwd) = std::env::current_dir() {
            return cwd.join(path);
        }

        return path;
    }

    let mut dir = match std::env::current_dir() {
        Ok(d) => d,
        Err(_) => return PathBuf::from(".tickets"),
    };

    loop {
        let candidate = dir.join(".tickets");
        if candidate.is_dir() {
            return candidate;
        }

        if !dir.pop() {
            break;
        }
    }

    PathBuf::from(".tickets")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TicketFrontmatter, cli::StatusValue};

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
}
