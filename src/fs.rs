use std::{
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

    Ok(tickets)
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
