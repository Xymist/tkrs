//! A minimal ticket system with dependency tracking
//! Rewritten in Rust because why not?

use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command as OsCommand;

use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

#[derive(Parser, Debug)]
#[command(
    name = "tk",
    bin_name = "tk",
    version,
    about = "minimal ticket system with dependency tracking",
    long_about = "minimal ticket system with dependency tracking",
    after_help = "Tickets stored as markdown files in .tickets/\nSupports partial ID matching (e.g., 'tk show 5c4' matches 'nw-5c46')"
)]
struct TicketCli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    #[command(about = "Create ticket, prints ID")]
    Create(CreateArgs),
    #[command(about = "Set status to in_progress")]
    Start(IdArg),
    #[command(about = "Set status to closed")]
    Close(IdArg),
    #[command(about = "Set status to open")]
    Reopen(IdArg),
    #[command(about = "Update status (open|in_progress|closed)")]
    Status(StatusArgs),
    #[command(about = "Manage dependencies (add/tree/cycle)")]
    Dep(DepArgs),
    #[command(about = "Remove dependency")]
    Undep(DepEdgeArgs),
    #[command(about = "Link tickets together (symmetric)")]
    Link(LinkArgs),
    #[command(about = "Remove link between tickets")]
    Unlink(UnlinkArgs),
    #[command(about = "List tickets")]
    Ls(ListArgs),
    #[command(about = "List open/in-progress tickets with deps resolved")]
    Ready(FilterArgs),
    #[command(about = "List open/in-progress tickets with unresolved deps")]
    Blocked(FilterArgs),
    #[command(about = "List recently closed tickets")]
    Closed(ClosedArgs),
    #[command(about = "Display ticket")]
    Show(IdArg),
    #[command(about = "Open ticket in $EDITOR")]
    Edit(IdArg),
    #[command(about = "Append timestamped note")]
    AddNote(AddNoteArgs),
    #[command(about = "Output tickets as JSON, optionally filtered")]
    Query(QueryArgs),
    #[command(about = "Import tickets from .beads/issues.jsonl")]
    MigrateBeads,
}

#[derive(Args, Debug)]
struct CreateArgs {
    /// Optional ticket title
    title: Option<String>,

    #[arg(short = 'd', long = "description", help = "Description text")]
    description: Option<String>,

    #[arg(long = "design", help = "Design notes")]
    design: Option<String>,

    #[arg(long = "acceptance", help = "Acceptance criteria")]
    acceptance: Option<String>,

    #[arg(short = 't', long = "type", value_enum, default_value = "task", help = "Type (bug|feature|task|epic|chore)")]
    ticket_type: TicketType,

    #[arg(
        short = 'p',
        long = "priority",
        value_parser = clap::value_parser!(u8).range(0..=4),
        default_value_t = 2,
        help = "Priority 0-4, 0=highest"
    )]
    priority: u8,

    #[arg(short = 'a', long = "assignee", help = "Assignee [default: git user.name]")]
    assignee: Option<String>,

    #[arg(long = "external-ref", help = "External reference (e.g., gh-123, JIRA-456)")]
    external_ref: Option<String>,

    #[arg(long = "parent", help = "Parent ticket ID")]
    parent: Option<String>,

    #[arg(short = 'T', long = "tags", value_delimiter = ',', help = "Comma-separated tags (e.g., --tags ui,backend,urgent)")]
    tags: Vec<String>,
}

#[derive(Args, Debug)]
struct IdArg {
    id: String,
}

#[derive(Args, Debug)]
struct StatusArgs {
    id: String,

    #[arg(value_enum)]
    status: StatusValue,
}

#[derive(Args, Debug)]
struct DepArgs {
    #[command(subcommand)]
    action: Option<DepAction>,

    /// Dependency add: dep <id> <dep-id>
    id: Option<String>,

    /// Dependency add: dep <id> <dep-id>
    dep_id: Option<String>,
}

#[derive(Subcommand, Debug)]
enum DepAction {
    Tree(DepTreeArgs),
    Cycle,
}

fn cmd_dep(args: DepArgs) -> Result<(), String> {
    match args.action {
        Some(DepAction::Tree(tree_args)) => dep_tree(tree_args),
        Some(DepAction::Cycle) => dep_cycle(),
        None => dep_add(args),
    }
}

fn dep_add(args: DepArgs) -> Result<(), String> {
    let id = args
        .id
        .ok_or_else(|| "Usage: tk dep <id> <dependency-id>".to_string())?;
    let dep_id = args
        .dep_id
        .ok_or_else(|| "Usage: tk dep <id> <dependency-id>".to_string())?;

    let ticket_path = resolve_ticket_path(&id)?;
    let _dep_path = resolve_ticket_path(&dep_id)?; // validate exists

    let mut ticket = read_ticket(&ticket_path)
        .map_err(|e| format!("failed to read ticket: {e}"))?
        .ok_or_else(|| "ticket missing id".to_string())?;

    if ticket.deps.iter().any(|d| d == &dep_id) {
        println!("Dependency already exists");
        return Ok(());
    }

    ticket.deps.push(dep_id.clone());
    ticket.deps.sort();
    ticket.deps.dedup();

    write_ticket_deps(&ticket.path, &ticket.deps)?;
    println!("Added dependency: {} -> {}", ticket.id, dep_id);
    Ok(())
}

fn write_ticket_deps(path: &Path, deps: &[String]) -> Result<(), String> {
    let contents = fs::read_to_string(path).map_err(|e| format!("failed to read ticket: {e}"))?;
    let mut lines = contents.lines();
    let mut output = String::new();
    let mut in_frontmatter = false;
    let mut deps_written = false;

    while let Some(line) = lines.next() {
        if line.trim() == "---" {
            if !in_frontmatter {
                in_frontmatter = true;
            } else {
                if !deps_written {
                    output.push_str(&format!("deps: [{}]\n", deps.join(", ")));
                }
                output.push_str("---\n");
                for rest in lines {
                    output.push_str(rest);
                    output.push('\n');
                }
                break;
            }
            output.push_str("---\n");
            continue;
        }

        if in_frontmatter && line.starts_with("deps:") {
            output.push_str(&format!("deps: [{}]\n", deps.join(", ")));
            deps_written = true;
        } else {
            output.push_str(line);
            output.push('\n');
        }
    }

    if !in_frontmatter {
        return Err("ticket missing frontmatter".to_string());
    }

    fs::write(path, output).map_err(|e| format!("failed to write ticket: {e}"))?;
    Ok(())
}

fn write_ticket_links(path: &Path, links: &[String]) -> Result<(), String> {
    let contents = fs::read_to_string(path).map_err(|e| format!("failed to read ticket: {e}"))?;
    let mut lines = contents.lines();
    let mut output = String::new();
    let mut in_frontmatter = false;
    let mut links_written = false;

    while let Some(line) = lines.next() {
        if line.trim() == "---" {
            if !in_frontmatter {
                in_frontmatter = true;
            } else {
                if !links_written {
                    output.push_str(&format!("links: [{}]\n", links.join(", ")));
                }
                output.push_str("---\n");
                for rest in lines {
                    output.push_str(rest);
                    output.push('\n');
                }
                break;
            }
            output.push_str("---\n");
            continue;
        }

        if in_frontmatter && line.starts_with("links:") {
            output.push_str(&format!("links: [{}]\n", links.join(", ")));
            links_written = true;
        } else {
            output.push_str(line);
            output.push('\n');
        }
    }

    if !in_frontmatter {
        return Err("ticket missing frontmatter".to_string());
    }

    fs::write(path, output).map_err(|e| format!("failed to write ticket: {e}"))?;
    Ok(())
}

fn resolve_partial_id(tickets: &[Ticket], needle: &str) -> Result<String, String> {
    let mut matches = tickets
        .iter()
        .filter(|t| t.id.contains(needle))
        .map(|t| t.id.clone())
        .collect::<Vec<_>>();
    matches.sort();
    matches.dedup();
    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => Err(format!("Error: ticket '{}' not found", needle)),
        _ => Err(format!(
            "Error: ambiguous ID '{}' matches multiple tickets",
            needle
        )),
    }
}

fn dep_tree(args: DepTreeArgs) -> Result<(), String> {
    let tickets = read_all_tickets().map_err(|e| e.to_string())?;
    if tickets.is_empty() {
        return Err("Error: ticket not found".to_string());
    }

    let lookup: HashMap<_, _> = tickets.iter().map(|t| (t.id.clone(), t)).collect();

    let root = resolve_partial_id(&tickets, &args.id)?;

    let mut visited = HashSet::new();
    let mut stack: Vec<(String, usize)> = Vec::new();
    stack.push((root.clone(), 0));

    println!(
        "{} [{}] {}",
        root,
        lookup.get(&root).map(|t| t.status.as_str()).unwrap_or(""),
        lookup.get(&root).map(|t| t.title.as_str()).unwrap_or("")
    );

    while let Some((id, depth)) = stack.pop() {
        if !args.full && !visited.insert(id.clone()) {
            continue;
        }

        let ticket = match lookup.get(&id) {
            Some(t) => t,
            None => continue,
        };

        let deps = &ticket.deps;
        let mut children = deps.clone();
        children.sort();

        for (idx, child) in children.into_iter().rev().enumerate() {
            if !args.full && visited.contains(&child) {
                continue;
            }
            let prefix = "    ".repeat(depth);
            let connector = if idx == 0 { "└── " } else { "├── " };
            let child_ticket = lookup.get(&child);
            println!(
                "{}{}{} [{}] {}",
                prefix,
                connector,
                child,
                child_ticket.map(|t| t.status.as_str()).unwrap_or(""),
                child_ticket.map(|t| t.title.as_str()).unwrap_or("")
            );
            stack.push((child, depth + 1));
        }
    }

    Ok(())
}

fn dep_cycle() -> Result<(), String> {
    let tickets = read_all_tickets().map_err(|e| e.to_string())?;
    if tickets.is_empty() {
        println!("No dependency cycles found");
        return Ok(());
    }

    let open_only: Vec<_> = tickets
        .into_iter()
        .filter(|t| t.status != "closed")
        .collect();

    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    for t in &open_only {
        graph.insert(t.id.clone(), t.deps.clone());
    }

    let mut state: HashMap<String, u8> = HashMap::new();
    let mut cycles: Vec<Vec<String>> = Vec::new();

    for node in graph.keys() {
        if state.get(node) == Some(&2) {
            continue;
        }
        let mut stack: Vec<(String, usize)> = Vec::new();
        let mut path: Vec<String> = Vec::new();
        stack.push((node.clone(), 0));

        while let Some((n, idx)) = stack.pop() {
            match state.get(&n).copied().unwrap_or(0) {
                0 => {
                    state.insert(n.clone(), 1); // visiting
                    stack.push((n.clone(), 0xff));
                    path.push(n.clone());
                    if let Some(neighbors) = graph.get(&n) {
                        for neigh in neighbors.iter().rev() {
                            stack.push((neigh.clone(), path.len()));
                        }
                    }
                }
                1 => {
                    // finishing node if idx == 0xff marker
                    state.insert(n.clone(), 2);
                    path.pop();
                }
                _ => {
                    // already done
                }
            }

            if idx != 0xff
                && let Some(pos) = path.iter().position(|p| p == &n)
            {
                let mut cycle = path[pos..].to_vec();
                cycle.push(n.clone());
                cycles.push(cycle);
            }
        }
    }

    if cycles.is_empty() {
        println!("No dependency cycles found");
        return Ok(());
    }

    for (i, cycle) in cycles.iter().enumerate() {
        println!("Cycle {}: {}", i + 1, cycle.join(" -> "));
    }

    Ok(())
}

fn cmd_undep(args: DepEdgeArgs) -> Result<(), String> {
    let path = resolve_ticket_path(&args.id)?;
    let mut ticket = read_ticket(&path)
        .map_err(|e| format!("failed to read ticket: {e}"))?
        .ok_or_else(|| "ticket missing id".to_string())?;

    let before = ticket.deps.len();
    ticket.deps.retain(|d| d != &args.dep_id);
    if before == ticket.deps.len() {
        // no-op if absent
        return Ok(());
    }

    write_ticket_deps(&ticket.path, &ticket.deps)?;
    println!("Removed dependency: {} !-> {}", ticket.id, args.dep_id);
    Ok(())
}

fn cmd_link(args: LinkArgs) -> Result<(), String> {
    let mut primary = load_ticket_by_id(&args.id)?;
    let mut targets = Vec::new();
    for t in &args.targets {
        targets.push(load_ticket_by_id(t)?);
    }

    // Update primary links
    for t in &targets {
        if !primary.links.contains(&t.id) {
            primary.links.push(t.id.clone());
        }
    }
    primary.links.sort();
    primary.links.dedup();
    write_ticket_links(&primary.path, &primary.links)?;

    // Update target links symmetrically
    for mut t in targets {
        if !t.links.contains(&primary.id) {
            t.links.push(primary.id.clone());
            t.links.sort();
            t.links.dedup();
            write_ticket_links(&t.path, &t.links)?;
        }
    }

    println!("Linked {} <-> {}", primary.id, args.targets.join(", "));
    Ok(())
}

fn cmd_unlink(args: UnlinkArgs) -> Result<(), String> {
    let mut primary = load_ticket_by_id(&args.id)?;
    let mut target = load_ticket_by_id(&args.target_id)?;

    let mut changed = false;
    let len_before = primary.links.len();
    primary.links.retain(|l| l != &target.id);
    if primary.links.len() != len_before {
        changed = true;
        write_ticket_links(&primary.path, &primary.links)?;
    }

    let len_before_t = target.links.len();
    target.links.retain(|l| l != &primary.id);
    if target.links.len() != len_before_t {
        changed = true;
        write_ticket_links(&target.path, &target.links)?;
    }

    if changed {
        println!("Unlinked {} <-> {}", primary.id, target.id);
    }
    Ok(())
}

fn ticket_matches_filters(
    ticket: &Ticket,
    status: Option<&str>,
    assignee: Option<&str>,
    tag: Option<&str>,
) -> bool {
    if let Some(s) = status
        && ticket.status != s
    {
        return false;
    }
    if let Some(a) = assignee
        && ticket.assignee().map(|v| v != a).unwrap_or(true)
    {
        return false;
    }
    if let Some(tag) = tag
        && !ticket.tags.iter().any(|t| t == tag)
    {
        return false;
    }
    true
}

fn cmd_ls(args: ListArgs) -> Result<(), String> {
    let tickets = read_all_tickets().map_err(|e| e.to_string())?;
    if tickets.is_empty() {
        return Ok(());
    }

    let mut tickets = tickets;
    tickets.sort_by(|a, b| a.id.cmp(&b.id));

    for ticket in &tickets {
        if !ticket_matches_filters(
            ticket,
            args.status.as_ref().map(StatusValue::as_str),
            args.assignee.as_deref(),
            args.tags.first().map(|s| s.as_str()),
        ) {
            continue;
        }

        let deps_display = if ticket.deps.is_empty() {
            String::from("[]")
        } else {
            format!("[{}]", ticket.deps.join(", "))
        };
        let dep_suffix = if ticket.deps.is_empty() {
            String::new()
        } else {
            format!(" <- {}", deps_display)
        };
        println!(
            "{:<8} [{}] - {}{}",
            ticket.id, ticket.status, ticket.title, dep_suffix
        );
    }

    Ok(())
}

fn cmd_ready(args: FilterArgs) -> Result<(), String> {
    let tickets = read_all_tickets().map_err(|e| e.to_string())?;
    if tickets.is_empty() {
        return Ok(());
    }

    let lookup: HashMap<_, _> = tickets.iter().map(|t| (t.id.clone(), t)).collect();

    let mut ready: Vec<&Ticket> = Vec::new();
    for ticket in &tickets {
        if ticket.status != "open" && ticket.status != "in_progress" {
            continue;
        }
        if !ticket_matches_filters(
            ticket,
            None,
            args.assignee.as_deref(),
            args.tags.first().map(|s| s.as_str()),
        ) {
            continue;
        }

        let mut all_closed = true;
        for dep in &ticket.deps {
            if let Some(t) = lookup.get(dep) {
                if t.status != "closed" {
                    all_closed = false;
                    break;
                }
            } else {
                // missing dep -> treat as not closed
                all_closed = false;
                break;
            }
        }

        if all_closed {
            ready.push(ticket);
        }
    }

    ready.sort_by(|a, b| {
        a.priority_value()
            .cmp(&b.priority_value())
            .then_with(|| a.id.cmp(&b.id))
    });

    for ticket in ready {
        println!(
            "{:<8} [P{}][{}] - {}",
            ticket.id,
            ticket.priority().unwrap_or(2),
            ticket.status,
            ticket.title
        );
    }

    Ok(())
}

fn cmd_blocked(args: FilterArgs) -> Result<(), String> {
    let tickets = read_all_tickets().map_err(|e| e.to_string())?;
    if tickets.is_empty() {
        return Ok(());
    }

    let lookup: HashMap<_, _> = tickets.iter().map(|t| (t.id.clone(), t)).collect();

    let mut blocked: Vec<(&Ticket, Vec<String>)> = Vec::new();
    for ticket in &tickets {
        if ticket.status != "open" && ticket.status != "in_progress" {
            continue;
        }
        if !ticket_matches_filters(
            ticket,
            None,
            args.assignee.as_deref(),
            args.tags.first().map(|s| s.as_str()),
        ) {
            continue;
        }

        let mut blockers: Vec<String> = Vec::new();
        for dep in &ticket.deps {
            if let Some(t) = lookup.get(dep) {
                if t.status != "closed" {
                    blockers.push(dep.clone());
                }
            } else {
                blockers.push(dep.clone());
            }
        }

        if !blockers.is_empty() {
            blocked.push((ticket, blockers));
        }
    }

    blocked.sort_by(|(a, _), (b, _)| {
        a.priority_value()
            .cmp(&b.priority_value())
            .then_with(|| a.id.cmp(&b.id))
    });

    for (ticket, blockers) in blocked {
        println!(
            "{:<8} [P{}][{}] - {} <- [{}]",
            ticket.id,
            ticket.priority().unwrap_or(2),
            ticket.status,
            ticket.title,
            blockers.join(", ")
        );
    }

    Ok(())
}

fn cmd_closed(args: ClosedArgs) -> Result<(), String> {
    let mut tickets = read_all_tickets().map_err(|e| e.to_string())?;
    if tickets.is_empty() {
        return Ok(());
    }

    // Sort by mtime descending using metadata; fallback to id order on errors
    tickets.sort_by(|a, b| {
        let ma = fs::metadata(&a.path).and_then(|m| m.modified());
        let mb = fs::metadata(&b.path).and_then(|m| m.modified());
        match (ma, mb) {
            (Ok(ta), Ok(tb)) => tb.cmp(&ta),
            _ => a.id.cmp(&b.id),
        }
    });

    let mut shown = 0u32;
    for ticket in tickets {
        if shown >= args.limit {
            break;
        }
        if ticket.status != "closed" && ticket.status != "done" {
            continue;
        }
        if let Some(assignee) = args.assignee.as_deref()
            && ticket.assignee().map(|a| a != assignee).unwrap_or(true)
        {
            continue;
        }
        if let Some(tag) = args.tags.first()
            && !ticket.tags.iter().any(|t| t == tag)
        {
            continue;
        }

        println!("{:<8} [{}] - {}", ticket.id, ticket.status, ticket.title);
        shown += 1;
    }

    Ok(())
}

fn cmd_show(args: IdArg) -> Result<(), String> {
    let path = resolve_ticket_path(&args.id)?;
    let content = fs::read_to_string(&path).map_err(|e| format!("failed to read ticket: {e}"))?;
    print!("{content}");
    Ok(())
}

fn cmd_edit(args: IdArg) -> Result<(), String> {
    let path = resolve_ticket_path(&args.id)?;
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let status = OsCommand::new(editor)
        .arg(&path)
        .status()
        .map_err(|e| format!("failed to launch editor: {e}"))?;
    if !status.success() {
        return Err("editor exited with error".to_string());
    }
    Ok(())
}

#[derive(Deserialize)]
struct BeadsIssue {
    id: String,
    title: Option<String>,
    status: Option<String>,
    dependencies: Option<Vec<BeadsDep>>, // type, depends_on_id
    created_at: Option<String>,
    issue_type: Option<String>,
    priority: Option<u8>,
    assignee: Option<String>,
    external_ref: Option<String>,
    description: Option<String>,
    design: Option<String>,
    acceptance_criteria: Option<String>,
    notes: Option<String>,
}

#[derive(Deserialize)]
struct BeadsDep {
    #[serde(rename = "type")]
    dep_type: String,
    depends_on_id: Option<String>,
}

fn cmd_migrate_beads() -> Result<(), String> {
    let jsonl_path = PathBuf::from(".beads/issues.jsonl");
    if !jsonl_path.exists() {
        return Err("Error: .beads/issues.jsonl not found".to_string());
    }

    let file = File::open(&jsonl_path).map_err(|e| format!("failed to open beads file: {e}"))?;
    let reader = io::BufReader::new(file);

    fs::create_dir_all(tickets_dir()).map_err(|e| format!("failed to create tickets dir: {e}"))?;

    let mut count = 0usize;
    for line in reader.lines() {
        let line = line.map_err(|e| format!("failed to read beads line: {e}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let issue: BeadsIssue =
            serde_json::from_str(&line).map_err(|e| format!("failed to parse beads issue: {e}"))?;

        let deps = issue
            .dependencies
            .as_ref()
            .map(|deps| {
                deps.iter()
                    .filter(|d| d.dep_type == "blocks")
                    .filter_map(|d| d.depends_on_id.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let links = issue
            .dependencies
            .as_ref()
            .map(|deps| {
                deps.iter()
                    .filter(|d| d.dep_type == "related")
                    .filter_map(|d| d.depends_on_id.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let parent = issue.dependencies.as_ref().and_then(|deps| {
            deps.iter()
                .find(|d| d.dep_type == "parent-child")
                .and_then(|d| d.depends_on_id.clone())
        });

        let id = issue.id;
        let path = tickets_dir().join(format!("{id}.md"));
        let mut file = File::create(&path).map_err(|e| format!("failed to write ticket: {e}"))?;

        writeln!(file, "---").map_err(map_io)?;
        writeln!(file, "id: {id}").map_err(map_io)?;
        writeln!(
            file,
            "status: {}",
            issue.status.as_deref().unwrap_or("open")
        )
        .map_err(map_io)?;
        writeln!(file, "deps: [{}]", deps.join(", ")).map_err(map_io)?;
        writeln!(file, "links: [{}]", links.join(", ")).map_err(map_io)?;
        writeln!(
            file,
            "created: {}",
            issue.created_at.as_deref().unwrap_or("")
        )
        .map_err(map_io)?;
        writeln!(
            file,
            "type: {}",
            issue.issue_type.as_deref().unwrap_or("task")
        )
        .map_err(map_io)?;
        writeln!(file, "priority: {}", issue.priority.unwrap_or(2)).map_err(map_io)?;
        if let Some(assignee) = issue.assignee.as_deref()
            && !assignee.is_empty()
        {
            writeln!(file, "assignee: {assignee}").map_err(map_io)?;
        }
        if let Some(ext) = issue.external_ref.as_deref()
            && !ext.is_empty()
        {
            writeln!(file, "external-ref: {ext}").map_err(map_io)?;
        }
        if let Some(p) = parent.as_deref() {
            writeln!(file, "parent: {p}").map_err(map_io)?;
        }
        writeln!(file, "---").map_err(map_io)?;
        writeln!(file, "# {}", issue.title.as_deref().unwrap_or("Untitled")).map_err(map_io)?;
        writeln!(file).map_err(map_io)?;

        if let Some(desc) = issue.description.as_deref()
            && !desc.is_empty()
        {
            writeln!(file, "{}\n", desc).map_err(map_io)?;
        }
        if let Some(design) = issue.design.as_deref()
            && !design.is_empty()
        {
            writeln!(file, "## Design\n\n{}\n", design).map_err(map_io)?;
        }
        if let Some(acc) = issue.acceptance_criteria.as_deref()
            && !acc.is_empty()
        {
            writeln!(file, "## Acceptance Criteria\n\n{}\n", acc).map_err(map_io)?;
        }
        if let Some(notes) = issue.notes.as_deref()
            && !notes.is_empty() {
                writeln!(file, "## Notes\n\n{}\n", notes).map_err(map_io)?;
            }

        count += 1;
    }

    println!("Migrated {} tickets from beads", count);
    Ok(())
}

fn cmd_query(args: QueryArgs) -> Result<(), String> {
    let tickets = read_all_tickets().map_err(|e| e.to_string())?;
    let filter = args.filter;

    let mut items: Vec<serde_json::Value> = tickets.iter().map(|t| t.to_json()).collect();
    items.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));

    if let Some(filt) = filter {
        let data = serde_json::to_string(&items).map_err(|e| e.to_string())?;
        let mut child = OsCommand::new("jq")
            .arg(filt)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .map_err(|_| "jq not available for filtering".to_string())?;
        if let Some(stdin) = child.stdin.as_mut() {
            stdin.write_all(data.as_bytes()).map_err(map_io)?;
        }
        let out = child.wait_with_output().map_err(map_io)?;
        if !out.status.success() {
            return Err("jq filter failed".to_string());
        }
        print!("{}", String::from_utf8_lossy(&out.stdout));
    } else {
        for item in items {
            println!(
                "{}",
                serde_json::to_string(&item).map_err(|e| e.to_string())?
            );
        }
    }
    Ok(())
}

fn cmd_add_note(args: AddNoteArgs) -> Result<(), String> {
    let path = resolve_ticket_path(&args.id)?;

    let note_text = if let Some(text) = args.text {
        text
    } else {
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .map_err(|e| format!("failed to read stdin: {e}"))?;
        if buffer.trim().is_empty() {
            return Err("Error: no note provided".to_string());
        }
        buffer
    };

    let timestamp = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|e| e.to_string())?;

    let mut file = OpenOptions::new()
        .append(true)
        .open(&path)
        .map_err(|e| format!("failed to open ticket: {e}"))?;

    let contents = fs::read_to_string(&path).map_err(|e| format!("failed to read ticket: {e}"))?;
    if !contents.contains("## Notes") {
        writeln!(file, "\n## Notes").map_err(map_io)?;
    }
    writeln!(file, "\n**{}**\n\n{}", timestamp, note_text.trim_end()).map_err(map_io)?;
    println!(
        "Note added to {}",
        path.file_stem().unwrap_or_default().to_string_lossy()
    );
    Ok(())
}

fn load_ticket_by_id(id: &str) -> Result<Ticket, String> {
    let path = resolve_ticket_path(id)?;
    read_ticket(&path)
        .map_err(|e| format!("failed to read ticket: {e}"))?
        .ok_or_else(|| "ticket missing id".to_string())
}

#[derive(Args, Debug)]
struct DepEdgeArgs {
    id: String,
    dep_id: String,
}

#[derive(Args, Debug)]
struct DepTreeArgs {
    #[arg(long = "full", default_value_t = false, help = "Show all nodes (disable dedup)")]
    full: bool,
    id: String,
}

#[derive(Args, Debug)]
struct LinkArgs {
    id: String,

    #[arg(required = true)]
    targets: Vec<String>,
}

#[derive(Args, Debug)]
struct UnlinkArgs {
    id: String,
    target_id: String,
}

#[derive(Args, Debug)]
struct ListArgs {
    #[arg(short = 's', long = "status", value_enum)]
    status: Option<StatusValue>,

    #[arg(short = 'a', long = "assignee")]
    assignee: Option<String>,

    #[arg(short = 'T', long = "tags", value_delimiter = ',')]
    tags: Vec<String>,
}

#[derive(Args, Debug)]
struct FilterArgs {
    #[arg(short = 'a', long = "assignee")]
    assignee: Option<String>,

    #[arg(short = 'T', long = "tags", value_delimiter = ',')]
    tags: Vec<String>,
}

#[derive(Args, Debug)]
struct ClosedArgs {
    #[arg(long = "limit", default_value_t = 20)]
    limit: u32,

    #[arg(short = 'a', long = "assignee")]
    assignee: Option<String>,

    #[arg(short = 'T', long = "tags", value_delimiter = ',')]
    tags: Vec<String>,
}

#[derive(Args, Debug)]
struct AddNoteArgs {
    id: String,
    text: Option<String>,
}

#[derive(Args, Debug)]
struct QueryArgs {
    filter: Option<String>,
}

#[derive(ValueEnum, Clone, Debug)]
#[value(rename_all = "snake_case")]
enum StatusValue {
    Open,
    InProgress,
    Closed,
}

impl StatusValue {
    fn as_str(&self) -> &'static str {
        match self {
            StatusValue::Open => "open",
            StatusValue::InProgress => "in_progress",
            StatusValue::Closed => "closed",
        }
    }
}

#[derive(ValueEnum, Clone, Debug)]
#[value(rename_all = "kebab_case")]
enum TicketType {
    Bug,
    Feature,
    Task,
    Epic,
    Chore,
}

impl TicketType {
    fn as_str(&self) -> &'static str {
        match self {
            TicketType::Bug => "bug",
            TicketType::Feature => "feature",
            TicketType::Task => "task",
            TicketType::Epic => "epic",
            TicketType::Chore => "chore",
        }
    }
}

fn cmd_create(args: CreateArgs) -> Result<(), String> {
    let tickets_dir = tickets_dir();
    fs::create_dir_all(&tickets_dir).map_err(|e| format!("failed to create tickets dir: {e}"))?;

    let title = args.title.unwrap_or_else(|| "Untitled".to_string());
    let assignee = args.assignee.or_else(git_user_name);
    let id = generate_id().map_err(|e| format!("failed to generate id: {e}"))?;
    let file_path = tickets_dir.join(format!("{id}.md"));
    let now = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|e| e.to_string())?;

    let mut content = String::new();
    content.push_str("---\n");
    content.push_str(&format!("id: {id}\n"));
    content.push_str("status: open\n");
    content.push_str("deps: []\n");
    content.push_str("links: []\n");
    content.push_str(&format!("created: {now}\n"));
    content.push_str(&format!("type: {}\n", args.ticket_type.as_str()));
    content.push_str(&format!("priority: {}\n", args.priority));
    if let Some(assignee) = assignee {
        content.push_str(&format!("assignee: {assignee}\n"));
    }
    if let Some(external) = args.external_ref {
        content.push_str(&format!("external-ref: {external}\n"));
    }
    if let Some(parent) = args.parent {
        content.push_str(&format!("parent: {parent}\n"));
    }
    if !args.tags.is_empty() {
        let joined = args.tags.join(", ");
        content.push_str(&format!("tags: [{joined}]\n"));
    }
    content.push_str("---\n");
    content.push_str(&format!("# {title}\n\n"));

    if let Some(desc) = args.description {
        content.push_str(&desc);
        content.push_str("\n\n");
    }

    if let Some(design) = args.design {
        content.push_str("## Design\n\n");
        content.push_str(&design);
        content.push_str("\n\n");
    }

    if let Some(acc) = args.acceptance {
        content.push_str("## Acceptance Criteria\n\n");
        content.push_str(&acc);
        content.push_str("\n\n");
    }

    fs::write(&file_path, content)
        .map_err(|e| format!("failed to write ticket file: {e}"))?;

    println!("{id}");
    Ok(())
}

fn cmd_set_status(id: String, status: StatusValue) -> Result<(), String> {
    let path = resolve_ticket_path(&id)?;
    let contents = fs::read_to_string(&path).map_err(|e| format!("failed to read ticket: {e}"))?;

    let mut lines = contents.lines();
    let mut output = String::new();
    let mut in_frontmatter = false;
    let mut status_written = false;

    while let Some(line) = lines.next() {
        if line.trim() == "---" {
            if !in_frontmatter {
                in_frontmatter = true;
            } else {
                if !status_written {
                    output.push_str(&format!("status: {}\n", status.as_str()));
                }
                output.push_str("---\n");
                // write remaining lines and break
                for rest in lines {
                    output.push_str(rest);
                    output.push('\n');
                }
                break;
            }
            output.push_str("---\n");
            continue;
        }

        if in_frontmatter && line.starts_with("status:") {
            output.push_str(&format!("status: {}\n", status.as_str()));
            status_written = true;
        } else {
            output.push_str(line);
            output.push('\n');
        }
    }

    if !in_frontmatter {
        return Err("ticket missing frontmatter".to_string());
    }

    fs::write(&path, output).map_err(|e| format!("failed to write ticket: {e}"))?;
    Ok(())
}

fn resolve_ticket_path(input: &str) -> Result<PathBuf, String> {
    let dir = tickets_dir();
    let exact = dir.join(format!("{input}.md"));
    if exact.exists() {
        return Ok(exact);
    }

    let mut matches = Vec::new();
    if let Ok(read_dir) = fs::read_dir(&dir) {
        for entry in read_dir.flatten() {
            let path = entry.path();
            if let Some(name) = path.file_name().and_then(|n| n.to_str())
                && name.contains(input)
                && name.ends_with(".md")
            {
                matches.push(path.clone());
                if matches.len() > 1 {
                    break;
                }
            }
        }
    }

    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => Err(format!("Error: ticket '{input}' not found")),
        _ => Err(format!(
            "Error: ambiguous ID '{input}' matches multiple tickets"
        )),
    }
}

fn read_all_tickets() -> io::Result<Vec<Ticket>> {
    let mut tickets = Vec::new();
    let dir = tickets_dir();
    if !dir.exists() {
        return Ok(tickets);
    }

    for entry in fs::read_dir(dir)? {
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

#[derive(Debug, Clone)]
struct Ticket {
    id: String,
    status: String,
    title: String,
    deps: Vec<String>,
    links: Vec<String>,
    priority: Option<u8>,
    assignee: Option<String>,
    tags: Vec<String>,
    created: Option<String>,
    parent: Option<String>,
    external_ref: Option<String>,
    #[allow(dead_code)]
    description: Option<String>,
    #[allow(dead_code)]
    design: Option<String>,
    #[allow(dead_code)]
    acceptance: Option<String>,
    #[allow(dead_code)]
    notes: Option<String>,
    path: PathBuf,
}

impl Ticket {
    fn priority(&self) -> Option<u8> {
        self.priority
    }

    fn assignee(&self) -> Option<&str> {
        self.assignee.as_deref()
    }

    fn priority_value(&self) -> u8 {
        self.priority.unwrap_or(2)
    }

    fn to_json(&self) -> serde_json::Value {
        json!({
            "id": self.id,
            "status": self.status,
            "title": self.title,
            "deps": self.deps,
            "links": self.links,
            "priority": self.priority.unwrap_or(2),
            "assignee": self.assignee,
            "tags": self.tags,
            "created": self.created,
            "parent": self.parent,
            "external_ref": self.external_ref,
        })
    }
}

fn read_ticket(path: &Path) -> io::Result<Option<Ticket>> {
    let content = fs::read_to_string(path)?;
    let lines = content.lines();
    let mut in_frontmatter = false;
    let mut id = String::new();
    let mut status = String::new();
    let mut deps: Vec<String> = Vec::new();
    let mut links: Vec<String> = Vec::new();
    let mut priority: Option<u8> = None;
    let mut assignee: Option<String> = None;
    let mut tags: Vec<String> = Vec::new();
    let mut created: Option<String> = None;
    let mut parent: Option<String> = None;
    let mut external_ref: Option<String> = None;
    let mut description: Option<String> = None;
    let mut design: Option<String> = None;
    let mut acceptance: Option<String> = None;
    let mut notes: Option<String> = None;
    let mut title = String::new();

    for line in lines {
        if line.trim() == "---" {
            if !in_frontmatter {
                in_frontmatter = true;
                continue;
            } else {
                break;
            }
        }
        if in_frontmatter {
            if let Some(rest) = line.strip_prefix("id:") {
                id = rest.trim().to_string();
            } else if let Some(rest) = line.strip_prefix("status:") {
                status = rest.trim().to_string();
            } else if let Some(rest) = line.strip_prefix("deps:") {
                deps = parse_array_line(rest.trim());
            } else if let Some(rest) = line.strip_prefix("links:") {
                links = parse_array_line(rest.trim());
            } else if let Some(rest) = line.strip_prefix("priority:") {
                priority = rest.trim().parse().ok();
            } else if let Some(rest) = line.strip_prefix("assignee:") {
                let trimmed = rest.trim();
                if !trimmed.is_empty() {
                    assignee = Some(trimmed.to_string());
                }
            } else if let Some(rest) = line.strip_prefix("tags:") {
                tags = parse_array_line(rest.trim());
            } else if let Some(rest) = line.strip_prefix("created:") {
                let trimmed = rest.trim();
                if !trimmed.is_empty() {
                    created = Some(trimmed.to_string());
                }
            } else if let Some(rest) = line.strip_prefix("parent:") {
                let trimmed = rest.trim();
                if !trimmed.is_empty() {
                    parent = Some(trimmed.to_string());
                }
            } else if let Some(rest) = line.strip_prefix("external-ref:") {
                let trimmed = rest.trim();
                if !trimmed.is_empty() {
                    external_ref = Some(trimmed.to_string());
                }
            }
        } else if title.is_empty()
            && let Some(rest) = line.strip_prefix("# ")
        {
            title = rest.to_string();
        } else if line.starts_with("## Design") {
            design = Some(String::new());
        } else if line.starts_with("## Acceptance Criteria") {
            acceptance = Some(String::new());
        } else if line.starts_with("## Notes") {
            notes = Some(String::new());
        } else if design.is_some() && notes.is_none() && !line.starts_with("## ") {
            design = Some(match design.take() {
                Some(mut s) => {
                    s.push_str(line);
                    s.push('\n');
                    s
                }
                None => String::new(),
            });
        } else if acceptance.is_some() && notes.is_none() && !line.starts_with("## ") {
            acceptance = Some(match acceptance.take() {
                Some(mut s) => {
                    s.push_str(line);
                    s.push('\n');
                    s
                }
                None => String::new(),
            });
        } else if notes.is_some() && !line.starts_with("## ") {
            notes = Some(match notes.take() {
                Some(mut s) => {
                    s.push_str(line);
                    s.push('\n');
                    s
                }
                None => String::new(),
            });
        } else if notes.is_none() && acceptance.is_none() && design.is_none() && !line.is_empty() {
            // First body section before any headings -> description
            description = Some(match description.take() {
                Some(mut s) => {
                    s.push_str(line);
                    s.push('\n');
                    s
                }
                None => {
                    let mut s = String::new();
                    s.push_str(line);
                    s.push('\n');
                    s
                }
            });
        }
    }

    if id.is_empty() {
        return Ok(None);
    }

    Ok(Some(Ticket {
        id,
        status,
        title,
        deps,
        links,
        priority,
        assignee,
        tags,
        created,
        parent,
        external_ref,
        description,
        design,
        acceptance,
        notes,
        path: path.to_path_buf(),
    }))
}

fn parse_array_line(raw: &str) -> Vec<String> {
    let trimmed = raw.trim();
    let without_brackets = trimmed.trim_start_matches('[').trim_end_matches(']');
    if without_brackets.trim().is_empty() {
        return Vec::new();
    }
    without_brackets
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn git_user_name() -> Option<String> {
    let output = OsCommand::new("git")
        .args(["config", "user.name"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn generate_id() -> Result<String, io::Error> {
    let dir = std::env::current_dir()?;
    let dir_name = dir.file_name().unwrap_or_default().to_string_lossy();
    let mut prefix = dir_name
        .split(&['-', '_'][..])
        .filter_map(|s| s.chars().next())
        .collect::<String>();
    if prefix.is_empty() {
        prefix = dir_name.chars().take(3).collect();
    }

    let mut hasher = Sha256::new();
    let entropy = format!(
        "{}{}",
        std::process::id(),
        OffsetDateTime::now_utc().unix_timestamp()
    );
    hasher.update(entropy.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    let suffix: String = hash.chars().take(4).collect();

    Ok(format!("{prefix}-{suffix}"))
}

fn tickets_dir() -> PathBuf {
    PathBuf::from(".tickets")
}

fn map_io(err: io::Error) -> String {
    err.to_string()
}

fn main() {
    let cli = TicketCli::parse();

    match cli.command {
        Command::Create(args) => cmd_create(args).unwrap_or_else(report_err),
        Command::Start(args) => {
            cmd_set_status(args.id, StatusValue::InProgress).unwrap_or_else(report_err)
        }
        Command::Close(args) => {
            cmd_set_status(args.id, StatusValue::Closed).unwrap_or_else(report_err)
        }
        Command::Reopen(args) => {
            cmd_set_status(args.id, StatusValue::Open).unwrap_or_else(report_err)
        }
        Command::Status(args) => cmd_set_status(args.id, args.status).unwrap_or_else(report_err),
        Command::Dep(args) => cmd_dep(args).unwrap_or_else(report_err),
        Command::Undep(args) => cmd_undep(args).unwrap_or_else(report_err),
        Command::Link(args) => cmd_link(args).unwrap_or_else(report_err),
        Command::Unlink(args) => cmd_unlink(args).unwrap_or_else(report_err),
        Command::Ls(args) => cmd_ls(args).unwrap_or_else(report_err),
        Command::Ready(args) => cmd_ready(args).unwrap_or_else(report_err),
        Command::Blocked(args) => cmd_blocked(args).unwrap_or_else(report_err),
        Command::Closed(args) => cmd_closed(args).unwrap_or_else(report_err),
        Command::Show(args) => cmd_show(args).unwrap_or_else(report_err),
        Command::Edit(args) => cmd_edit(args).unwrap_or_else(report_err),
        Command::AddNote(args) => cmd_add_note(args).unwrap_or_else(report_err),
        Command::Query(args) => cmd_query(args).unwrap_or_else(report_err),
        Command::MigrateBeads => cmd_migrate_beads().unwrap_or_else(report_err),
    }
}

fn report_err(err: impl std::fmt::Display) {
    eprintln!("{err}");
}
