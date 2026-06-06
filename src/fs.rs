use std::{
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

use color_eyre::eyre::eyre;

use crate::{Ticket, TicketBody, TicketSection};

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
    let mut design = String::new();
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
            section = TicketSection::Design;
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
            TicketSection::Design => &mut design,
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
        description: if description.is_empty() {
            None
        } else {
            Some(description.trim().to_string())
        },
        implementation_plan: if design.is_empty() {
            None
        } else {
            Some(design.trim().to_string())
        },
        acceptance: if acceptance.is_empty() {
            None
        } else {
            Some(acceptance.trim().to_string())
        },
        notes: if notes.is_empty() {
            Vec::new()
        } else {
            notes.split("\n\n").map(|s| s.trim().to_string()).collect()
        },
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

    if let Some(desc) = &ticket.description() {
        content.push_str(desc);
        content.push_str("\n\n");
    }
    if let Some(design) = &ticket.design() {
        content.push_str("## Implementation Plan\n\n");
        content.push_str(design);
        content.push_str("\n\n");
    }
    if let Some(acceptance) = &ticket.acceptance() {
        content.push_str("## Acceptance Criteria\n\n");
        content.push_str(acceptance);
        content.push_str("\n\n");
    }
    if !ticket.notes().is_empty() {
        content.push_str("## Notes\n\n");
        content.push_str(&ticket.notes().join("\n"));
        content.push('\n');
    }

    std::fs::write(&ticket.path, content)?;

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
