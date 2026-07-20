use clap::{ArgGroup, Args, Parser, Subcommand, ValueEnum};
use color_eyre::eyre::eyre;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::fmt::{Display, Formatter};
use std::fs::{self};
use std::io::{IsTerminal, Read};
use std::process::Command as OsCommand;
use std::{io, path::PathBuf};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::fs::write_ticket;
use crate::publish::{self, GithubPublishArgs};
use crate::tree::{TicketSelection, assemble_ticket_forest, print_forest};
use crate::{
    LinkOp, RemovalOutcome, Ticket, TicketBody, apply_query_filter, build_link_plan, dep_add,
    find_ticket, git_user_name,
    ids::{generate_id, resolve_partial_id},
    lock_tickets, parse_status, read_ticket_graph, remove_dependency, resolve_ticket_path,
    set_status_with_note, ticket_matches_filters, ticket_to_json, tickets_dir,
    validate_section_value, write_query_output, write_ticket_links,
};
use crate::{TicketFrontmatter, locate_cycles};

#[derive(Parser, Debug)]
#[command(
    name = "tk",
    bin_name = "tk",
    version,
    about = "minimal ticket system with dependency tracking",
    long_about = "minimal ticket system with dependency tracking",
    after_help = "Tickets stored as markdown files in .tickets/\nSupports partial ID matching (e.g., 'tk show 5c4' matches 'nw-5c46')"
)]
pub struct TicketCli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    #[command(about = "Start the TUI")]
    Tui,
    #[command(about = "Create ticket, prints ID")]
    Create(CreateArgs),
    #[command(about = "Set status to in_progress")]
    Start(StartArgs),
    #[command(about = "Set status to closed")]
    Close(CloseArgs),
    #[command(about = "Set status to open")]
    Reopen(ReopenArgs),
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
    #[clap(aliases = &["list"])]
    Ls(ListArgs),
    #[command(about = "List open/in-progress tickets with deps resolved")]
    Ready(FilterArgs),
    #[command(about = "List open/in-progress tickets with unresolved deps")]
    Blocked(BlockedArgs),
    #[command(about = "List recently closed tickets")]
    Closed(ClosedArgs),
    #[command(about = "Display ticket")]
    Show(ShowArgs),
    #[command(about = "Update ticket sections, or open it in $EDITOR with -i")]
    Edit(EditArgs),
    #[command(about = "Append timestamped note")]
    AddNote(AddNoteArgs),
    #[command(about = "Output tickets as JSON, optionally filtered")]
    Query(QueryArgs),
    #[command(about = "Print the full ticket tree, expanded, as plain text")]
    Tree(TreeArgs),
    #[command(about = "Publish a ticket to an external tracker")]
    Publish(PublishArgs),
    #[command(about = "Update tk to the latest release from GitHub")]
    Update(UpdateArgs),
}

#[derive(Args, Debug)]
pub struct UpdateArgs {}

#[derive(Args, Debug)]
pub struct CreateArgs {
    /// Ticket title (required)
    title: String,

    #[arg(short = 'd', long = "description", help = "Description text")]
    description: Option<String>,

    #[arg(long = "implementation-plan", help = "Implementation plan notes")]
    implementation_plan: Option<String>,

    #[arg(long = "acceptance", help = "Acceptance criteria")]
    acceptance: Option<String>,

    #[arg(
        short = 't',
        long = "type",
        value_enum,
        default_value = "task",
        help = "Type (bug|feature|task|epic|chore)"
    )]
    ticket_type: TicketType,

    #[arg(
        short = 'p',
        long = "priority",
        value_parser = clap::value_parser!(u8).range(0..=4),
        default_value_t = 2,
        help = "Priority 0-4, 0=highest"
    )]
    priority: u8,

    #[arg(
        short = 'a',
        long = "assignee",
        help = "Assignee [default: git user.name]"
    )]
    assignee: Option<String>,

    #[arg(
        long = "external-ref",
        help = "External reference (e.g., gh-123, JIRA-456)"
    )]
    external_ref: Option<String>,

    #[arg(long = "parent", help = "Parent ticket ID")]
    parent: Option<String>,

    #[arg(
        short = 'T',
        long = "tags",
        value_delimiter = ',',
        help = "Comma-separated tags (e.g., --tags ui,backend,urgent)"
    )]
    tags: Vec<String>,

    #[arg(
        long = "body-from-file",
        value_name = "PATH",
        help = "Append body content from file to the description"
    )]
    body_from_file: Option<PathBuf>,

    #[arg(
        long = "edit",
        default_value_t = false,
        help = "Launch editor to compose ticket body after creation"
    )]
    edit: bool,
}

#[derive(ValueEnum, Clone, Debug)]
#[value(rename_all = "kebab_case")]
pub enum TicketType {
    Bug,
    Feature,
    Task,
    Epic,
    Chore,
}

impl TicketType {
    pub fn as_str(&self) -> &'static str {
        match self {
            TicketType::Bug => "bug",
            TicketType::Feature => "feature",
            TicketType::Task => "task",
            TicketType::Epic => "epic",
            TicketType::Chore => "chore",
        }
    }
}

#[derive(Args, Debug)]
pub struct IdArg {
    id: String,
}

#[derive(Args, Debug)]
pub struct ReopenArgs {
    id: String,

    #[arg(
        long = "note",
        alias = "message",
        help = "Optional note to record when reopening"
    )]
    note: Option<String>,
}

#[derive(Args, Debug)]
pub struct ShowArgs {
    id: String,

    #[arg(long = "json", default_value_t = false, help = "Render ticket as JSON")]
    json: bool,
}

#[derive(Args, Debug)]
#[command(group(
    ArgGroup::new("edit_mode")
        .required(true)
        .multiple(true)
        .args([
            "description",
            "implementation_plan",
            "acceptance",
            "external_ref",
            "body_from_file",
            "interactive",
            "print",
        ])
))]
pub struct EditArgs {
    id: String,

    #[arg(
        short = 'd',
        long = "description",
        help = "Replace the description section; an empty string clears it"
    )]
    description: Option<String>,

    #[arg(
        long = "implementation-plan",
        help = "Replace the implementation plan section; an empty string clears it"
    )]
    implementation_plan: Option<String>,

    #[arg(
        long = "acceptance",
        help = "Replace the acceptance criteria section; an empty string clears it"
    )]
    acceptance: Option<String>,

    #[arg(
        long = "external-ref",
        help = "Replace the external reference (e.g., gh-123, JIRA-456); an empty string clears it"
    )]
    external_ref: Option<String>,

    #[arg(
        long = "body-from-file",
        value_name = "PATH",
        help = "Append body content from file to the description; combine with --description to include both"
    )]
    body_from_file: Option<PathBuf>,

    #[arg(
        short = 'i',
        long = "interactive",
        default_value_t = false,
        help = "Launch $EDITOR to edit the ticket, after applying any update flags"
    )]
    interactive: bool,

    #[arg(
        long = "print",
        default_value_t = false,
        help = "Print the ticket path; combined with -i, prints instead of opening the editor"
    )]
    print: bool,

    #[arg(
        long = "force",
        default_value_t = false,
        help = "Force launching the editor when stdout is not a TTY; only has effect together with -i"
    )]
    force: bool,
}

#[derive(Args, Debug)]
pub struct CloseArgs {
    id: String,

    #[arg(
        long = "note",
        alias = "message",
        help = "Optional note to record when closing"
    )]
    note: Option<String>,
}

#[derive(Args, Debug)]
pub struct StatusArgs {
    id: String,

    /// New status (open|in_progress|closed), case-insensitive
    status: String,

    #[arg(
        long = "note",
        alias = "message",
        help = "Optional note to append alongside status change"
    )]
    note: Option<String>,
}

#[derive(Args, Debug)]
pub struct DepArgs {
    #[command(subcommand)]
    action: Option<DepAction>,

    /// Dependency add: dep <id> <dep-id>
    id: Option<String>,

    /// Dependency add: dep <id> <dep-id>
    dep_id: Option<String>,

    #[arg(
        long = "check-cycle",
        num_args = 0..=1,
        default_value_t = true,
        default_missing_value = "true",
        action = clap::ArgAction::Set,
        help = "Detect new dependency cycles after adding and revert if introduced"
    )]
    check_cycle: bool,
}

#[derive(Args, Debug)]
pub struct DepCycleArgs {
    #[arg(
        long = "include-closed",
        default_value_t = false,
        help = "Include closed tickets when detecting cycles"
    )]
    include_closed: bool,
}

#[derive(Subcommand, Debug)]
pub enum DepAction {
    Tree(DepTreeArgs),
    Cycle(DepCycleArgs),
}

#[derive(Args, Debug)]
pub struct LinkArgs {
    id: String,

    #[arg(required = true)]
    targets: Vec<String>,

    #[arg(
        long = "dry-run",
        default_value_t = false,
        help = "Show planned link changes without writing"
    )]
    dry_run: bool,
}

#[derive(Args, Debug)]
pub struct UnlinkArgs {
    id: String,
    target_id: String,

    #[arg(
        long = "warn-missing",
        default_value_t = false,
        help = "Print a warning instead of silent success when link is absent"
    )]
    warn_missing: bool,
}

#[derive(Args, Debug)]
pub struct ListArgs {
    #[arg(short = 's', long = "status", value_enum)]
    status: Option<StatusValue>,

    #[arg(short = 'a', long = "assignee")]
    assignee: Option<String>,

    #[arg(short = 'T', long = "tags", value_delimiter = ',')]
    tags: Vec<String>,

    #[arg(
        long = "columns",
        value_delimiter = ',',
        value_name = "FIELDS",
        help = "Comma-separated columns to display (id,status,title,deps,priority,assignee,tags)"
    )]
    columns: Vec<String>,

    #[arg(
        long = "json",
        default_value_t = false,
        help = "Output rows as JSON array"
    )]
    json: bool,

    #[arg(
        long = "parent",
        value_name = "ID",
        help = "Pass the ID or partial ID of an epic to list its child tickets."
    )]
    parent: Option<String>,
}

#[derive(Args, Debug)]
pub struct FilterArgs {
    #[arg(short = 's', long = "status", value_enum)]
    status: Option<StatusValue>,

    #[arg(short = 'a', long = "assignee")]
    assignee: Option<String>,

    #[arg(short = 'T', long = "tags", value_delimiter = ',')]
    tags: Vec<String>,

    #[arg(
        long = "show-deps",
        default_value_t = false,
        help = "Show dependency count for each ticket"
    )]
    show_deps: bool,

    #[arg(
        long = "parent",
        value_name = "ID",
        help = "Pass the ID or partial ID of an epic to filter by its child tickets."
    )]
    parent: Option<String>,
}

#[derive(Args, Debug)]
pub struct BlockedArgs {
    #[arg(short = 'a', long = "assignee")]
    assignee: Option<String>,

    #[arg(short = 'T', long = "tags", value_delimiter = ',')]
    tags: Vec<String>,

    #[arg(
        long = "only-open",
        default_value_t = false,
        help = "Show only blockers that are open"
    )]
    only_open: bool,

    #[arg(
        long = "parent",
        value_name = "ID",
        help = "Pass the ID or partial ID of an epic to filter by its child tickets."
    )]
    parent: Option<String>,
}

#[derive(Args, Debug)]
pub struct ClosedArgs {
    #[arg(long = "limit", default_value_t = 20)]
    limit: u32,

    #[arg(short = 'a', long = "assignee")]
    assignee: Option<String>,

    #[arg(short = 'T', long = "tags", value_delimiter = ',')]
    tags: Vec<String>,

    #[arg(
        long = "since",
        value_name = "RFC3339",
        help = "Only list tickets modified on/after this time"
    )]
    since: Option<String>,
}

#[derive(Args, Debug)]
pub struct AddNoteArgs {
    id: String,
    text: Option<String>,

    #[arg(
        long = "tag",
        value_name = "LABEL",
        help = "Optional tag label to prefix the note with"
    )]
    tag: Option<String>,
}

#[derive(Args, Debug)]
pub struct QueryArgs {
    /// Optional filter expression, e.g., tags==backend or title~api
    filter: Option<String>,

    #[arg(
        long = "format",
        value_enum,
        default_value_t = QueryFormat::Ndjson,
        help = "Output format: ndjson (default) or pretty JSON array"
    )]
    format: QueryFormat,
}

#[derive(Args, Debug)]
pub struct TreeArgs {
    #[arg(
        short = 's',
        long = "status",
        value_enum,
        default_value = "open",
        help = "Filter tickets and dependencies by status (all|open|in-progress|closed)"
    )]
    status: TicketSelection,
}

/// `tk publish <target>`: nested like `dep`, so further publish targets can
/// be added without reshaping this command.
#[derive(Args, Debug)]
pub struct PublishArgs {
    #[command(subcommand)]
    action: PublishAction,
}

#[derive(Subcommand, Debug)]
pub enum PublishAction {
    #[command(about = "Publish a ticket as a GitHub issue")]
    Github(GithubPublishArgs),
}

pub fn cmd_publish(args: PublishArgs) -> color_eyre::Result<()> {
    match args.action {
        PublishAction::Github(gh_args) => publish::cmd_publish_github(gh_args),
    }
}

#[derive(Args, Debug)]
pub struct StartArgs {
    id: String,

    #[arg(
        long = "note",
        alias = "message",
        help = "Optional note to record when starting"
    )]
    note: Option<String>,
}

#[derive(Args, Debug)]
pub struct DepEdgeArgs {
    id: String,
    dep_id: String,
}

#[derive(Args, Debug)]
pub struct DepTreeArgs {
    #[arg(
        long = "full",
        default_value_t = false,
        help = "Show all nodes (disable dedup)"
    )]
    full: bool,

    #[arg(
        short = 's',
        long = "status",
        value_enum,
        help = "Filter nodes by status (open|in_progress|closed)"
    )]
    status: Option<StatusValue>,

    #[arg(
        long = "only-open",
        default_value_t = false,
        help = "Skip closed dependencies when traversing"
    )]
    only_open: bool,

    id: String,
}

#[derive(ValueEnum, Clone, Debug)]
#[value(rename_all = "lowercase")]
pub enum QueryFormat {
    Ndjson,
    Pretty,
}

#[derive(ValueEnum, Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[value(rename_all = "snake_case")]
#[serde(rename_all = "snake_case")]
pub enum StatusValue {
    Open,
    InProgress,
    Closed,
}

impl StatusValue {
    pub fn as_str(&self) -> &'static str {
        match self {
            StatusValue::Open => "open",
            StatusValue::InProgress => "in_progress",
            StatusValue::Closed => "closed",
        }
    }
}

impl Display for StatusValue {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

pub fn cmd_dep(args: DepArgs) -> color_eyre::Result<()> {
    let mut tickets = lock_tickets()?;
    match args.action {
        Some(DepAction::Tree(tree_args)) => dep_tree(tree_args, &tickets),
        Some(DepAction::Cycle(cycle_args)) => dep_cycle(cycle_args, &tickets),
        None => {
            let id = args
                .id
                .ok_or_else(|| eyre!("Usage: tk dep <id> <dependency-id>"))?;
            let dep_id = args
                .dep_id
                .ok_or_else(|| eyre!("Usage: tk dep <id> <dependency-id>"))?;
            let check_cycle = args.check_cycle;
            let target_id = resolve_partial_id(&tickets, &id)?;
            let dep_target_id = resolve_partial_id(&tickets, &dep_id)?;
            dep_add(&target_id, &dep_target_id, check_cycle, &mut tickets)?;
            println!("Added dependency: {} -> {}", target_id, dep_target_id);
            Ok(())
        }
    }
}

pub fn cmd_undep(args: DepEdgeArgs) -> color_eyre::Result<()> {
    let mut tickets = lock_tickets()?;
    let ticket_id = resolve_partial_id(&tickets, &args.id)?;
    let dep_id = resolve_partial_id(&tickets, &args.dep_id)?;

    if let RemovalOutcome::Removed = remove_dependency(&ticket_id, &dep_id, &mut tickets)? {
        println!("Removed dependency: {} !-> {}", ticket_id, dep_id);
    } else {
        println!("Dependency not present: {} !-> {}", ticket_id, dep_id);
    }

    Ok(())
}

pub fn cmd_start(args: StartArgs) -> color_eyre::Result<()> {
    let mut tickets = lock_tickets()?;
    let id = resolve_partial_id(&tickets, &args.id)?;
    set_status_with_note(
        &id,
        StatusValue::InProgress,
        args.note.as_deref(),
        &mut tickets,
    )
}

pub fn cmd_close(args: CloseArgs) -> color_eyre::Result<()> {
    let mut tickets = lock_tickets()?;
    let id = resolve_partial_id(&tickets, &args.id)?;
    set_status_with_note(&id, StatusValue::Closed, args.note.as_deref(), &mut tickets)
}

pub fn cmd_reopen(args: ReopenArgs) -> color_eyre::Result<()> {
    let mut tickets = lock_tickets()?;
    let id = resolve_partial_id(&tickets, &args.id)?;
    set_status_with_note(&id, StatusValue::Open, args.note.as_deref(), &mut tickets)
}

pub fn cmd_status(args: StatusArgs) -> color_eyre::Result<()> {
    let mut tickets = lock_tickets()?;
    let status = parse_status(&args.status)?;
    let id = resolve_partial_id(&tickets, &args.id)?;
    set_status_with_note(&id, status, args.note.as_deref(), &mut tickets)
}

/// Combines an inline description with file-sourced body content: both trimmed
/// and joined by a blank line when both are given, either one alone trimmed on
/// its own, and `None` when neither is given.
fn merge_description(description: Option<&str>, file_body: Option<&str>) -> Option<String> {
    match (description, file_body) {
        (Some(desc), Some(file)) => Some(format!("{}\n\n{}", desc.trim(), file.trim())),
        (Some(desc), None) => Some(desc.trim().to_string()),
        (None, Some(file)) => Some(file.trim().to_string()),
        (None, None) => None,
    }
}

/// Interprets a flag value passed to `edit` for replace-or-clear semantics: `None`
/// means the flag was not passed and the field is left untouched, `Some(None)`
/// clears the field, and `Some(Some(value))` replaces it with the trimmed value.
fn section_update(value: Option<&str>) -> Option<Option<String>> {
    value.map(|v| {
        let trimmed = v.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn apply_edit_updates(args: &EditArgs) -> color_eyre::Result<()> {
    let mut tickets = lock_tickets()?;
    let id = resolve_partial_id(&tickets, &args.id)?;

    let file_body = match args.body_from_file.as_ref() {
        Some(path) => Some(
            fs::read_to_string(path)
                .map_err(|e| eyre!("failed to read body file {}: {e}", path.display()))?,
        ),
        None => None,
    };
    let merged = merge_description(args.description.as_deref(), file_body.as_deref());
    let description = merged.filter(|s| !s.trim().is_empty());
    if let Some(value) = description.as_deref() {
        validate_section_value(value, "--description/--body-from-file")?;
    }

    let implementation_plan = section_update(args.implementation_plan.as_deref());
    if let Some(Some(value)) = &implementation_plan {
        validate_section_value(value, "--implementation-plan")?;
    }

    let acceptance = section_update(args.acceptance.as_deref());
    if let Some(Some(value)) = &acceptance {
        validate_section_value(value, "--acceptance")?;
    }

    let external_ref = section_update(args.external_ref.as_deref());

    let ticket = tickets
        .iter_mut()
        .find(|t| t.id() == id)
        .ok_or_else(|| eyre!("Error: ticket '{}' not found", id))?;

    if args.description.is_some() || args.body_from_file.is_some() {
        ticket.set_description(description)?;
    }

    if let Some(value) = implementation_plan {
        ticket.set_implementation_plan(value)?;
    }

    if let Some(value) = acceptance {
        ticket.set_acceptance(value)?;
    }

    if let Some(value) = external_ref {
        ticket.set_external_ref(value);
    }

    write_ticket(ticket)?;

    println!("Updated {id}");

    Ok(())
}

pub fn cmd_edit(args: EditArgs) -> color_eyre::Result<()> {
    let has_updates = args.description.is_some()
        || args.implementation_plan.is_some()
        || args.acceptance.is_some()
        || args.external_ref.is_some()
        || args.body_from_file.is_some();

    if has_updates {
        apply_edit_updates(&args)?;
    }

    if !args.interactive {
        if args.print {
            let path = resolve_ticket_path(&args.id)?;
            println!("{}", path.display());
        }
        return Ok(());
    }

    let path = resolve_ticket_path(&args.id)?;

    let stdout_is_tty = io::stdout().is_terminal();
    if args.print || (!stdout_is_tty && !args.force) {
        println!("{}", path.display());
        return Ok(());
    }

    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vi".to_string());

    let status = OsCommand::new(editor)
        .arg(&path)
        .status()
        .map_err(|e| eyre!("failed to launch editor: {e}"))?;
    if !status.success() {
        return Err(eyre!("editor exited with error"));
    }
    Ok(())
}

pub fn cmd_update(_args: UpdateArgs) -> color_eyre::Result<()> {
    let status = self_update::backends::github::Update::configure()
        .repo_owner("Xymist")
        .repo_name("tkrs")
        .bin_name("tk")
        .current_version(env!("CARGO_PKG_VERSION"))
        .show_download_progress(true)
        .show_output(false)
        .no_confirm(true)
        .build()?
        .update()?;

    match status {
        self_update::Status::UpToDate(_) => println!("No new version available"),
        self_update::Status::Updated(version) => println!("Updated to version {version}"),
    }

    Ok(())
}

pub fn dep_tree(args: DepTreeArgs, tickets: &[Ticket]) -> color_eyre::Result<()> {
    let (tickets, graph) = read_ticket_graph(tickets, true)?;
    if tickets.is_empty() {
        return Err(eyre!("Error: ticket not found"));
    }

    let lookup: HashMap<&str, _> = tickets.iter().map(|t| (t.id(), t)).collect();

    let root_id = resolve_partial_id(&tickets, &args.id)?;
    let root = root_id.as_str();

    let mut visited = HashSet::new();
    let mut stack: Vec<(String, usize)> = Vec::new();
    stack.push((root.to_string(), 0));

    if args.status.is_none() || lookup.get(root).map(|t| t.status()) == args.status.as_ref() {
        println!(
            "{} [{}] {}",
            root,
            lookup.get(root).map(|t| t.status().as_str()).unwrap_or(""),
            lookup.get(root).map(|t| t.title.as_str()).unwrap_or("")
        );
    }

    while let Some((id, depth)) = stack.pop() {
        if !args.full && !visited.insert(id.clone()) {
            continue;
        }

        let ticket = match lookup.get(id.as_str()) {
            Some(t) => t,
            None => continue,
        };

        let deps = graph.get(ticket.id()).cloned().unwrap_or_default();
        let mut children = deps;

        children.sort();

        for (idx, child) in children.into_iter().rev().enumerate() {
            if !args.full && visited.contains(child.as_str()) {
                continue;
            }
            let prefix = "    ".repeat(depth);
            let connector = if idx == 0 { "└── " } else { "├── " };
            let child_ticket = lookup.get(child.as_str());
            if let Some(t) = child_ticket {
                if args.only_open && t.status() == &StatusValue::Closed {
                    continue;
                }
                if let Some(status) = args.status.as_ref()
                    && t.status() != status
                {
                    continue;
                }
            } else if args.only_open {
                continue;
            }

            println!(
                "{}{}{} [{}] {}",
                prefix,
                connector,
                child,
                child_ticket.map(|t| t.status().as_str()).unwrap_or(""),
                child_ticket.map(|t| t.title.as_str()).unwrap_or("")
            );
            stack.push((child, depth + 1));
        }
    }

    Ok(())
}

pub fn dep_cycle(args: DepCycleArgs, tickets: &[Ticket]) -> color_eyre::Result<()> {
    let mut cycles = locate_cycles(tickets, args.include_closed)?;
    if cycles.is_empty() {
        println!("No dependency cycles found");
        return Ok(());
    }

    cycles.sort_by(|a, b| a[..a.len() - 1].cmp(&b[..b.len() - 1]));

    println!("Dependency cycles:");
    for cycle in cycles {
        println!("{}", cycle.join(" -> "));
    }

    Ok(())
}

pub fn cmd_link(args: LinkArgs) -> color_eyre::Result<()> {
    let plan = build_link_plan(&args.id, &args.targets, LinkOp::Add)?;

    if plan.updates.is_empty() {
        println!("Links already up to date");
        return Ok(());
    }

    if args.dry_run {
        for change in &plan.updates {
            println!(
                "{}: [{}] -> [{}]",
                change.ticket.id(),
                change.before.join(", "),
                change.after.join(", ")
            );
        }
        return Ok(());
    }

    for change in &plan.updates {
        write_ticket_links(&change.ticket.path, &change.after)?;
    }

    let targets = args.targets.join(", ");
    println!("Linked {} <-> {}", args.id, targets);
    Ok(())
}

pub fn cmd_unlink(args: UnlinkArgs) -> color_eyre::Result<()> {
    let plan = build_link_plan(
        &args.id,
        std::slice::from_ref(&args.target_id),
        LinkOp::Remove,
    )?;

    if plan.updates.is_empty() {
        if args.warn_missing {
            println!("Warning: link {} <-> {} not found", args.id, args.target_id);
        }
        return Ok(());
    }

    for change in &plan.updates {
        write_ticket_links(&change.ticket.path, &change.after)?;
    }

    println!("Unlinked {} <-> {}", args.id, args.target_id);
    if args.warn_missing && !plan.missing.is_empty() {
        println!("Warning: link {} <-> {} not found", args.id, args.target_id);
    }
    Ok(())
}

pub fn cmd_ls(args: ListArgs) -> color_eyre::Result<()> {
    let tickets = lock_tickets()?;
    let parent = if let Some(p) = args.parent {
        Some(resolve_partial_id(&tickets, &p)?)
    } else {
        None
    };

    let filtered: Vec<&Ticket> = tickets
        .iter()
        .filter(|ticket| {
            ticket_matches_filters(
                &tickets,
                ticket,
                args.status,
                args.assignee.as_deref(),
                args.tags.first().map(|s| s.as_str()),
                parent.as_deref(),
            )
        })
        .collect();

    if args.json {
        let rows: Vec<serde_json::Value> = filtered
            .iter()
            .map(|t| {
                let mut parents: Vec<String> = tickets
                    .iter()
                    .filter(|p| p.deps().iter().any(|d| d == t.id()))
                    .map(|p| p.id().to_string())
                    .collect();
                parents.sort();
                json!({
                    "id": t.id(),
                    "status": t.status(),
                    "title": t.title,
                    "deps": t.deps(),
                    "priority": t.priority(),
                    "assignee": t.assignee(),
                    "tags": t.tags(),
                    "parents": parents
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&rows)
                .map_err(|e| eyre!("failed to serialize JSON: {e}"))?
        );
        return Ok(());
    }

    let selected_columns = if args.columns.is_empty() {
        vec!["id", "status", "title", "deps"]
    } else {
        args.columns.iter().map(|s| s.as_str()).collect()
    };

    for ticket in filtered {
        let mut parts: Vec<String> = Vec::new();
        for col in &selected_columns {
            match *col {
                "id" => parts.push(ticket.id().to_string()),
                "status" => parts.push(format!("[{}]", ticket.status())),
                "title" => parts.push(ticket.title.clone()),
                "deps" => parts.push(format!("[{}]", ticket.deps().join(", "))),
                "priority" => parts.push(format!("P{}", ticket.priority())),
                "assignee" => {
                    if let Some(a) = ticket.assignee() {
                        parts.push(a.to_string());
                    }
                }
                "tags" => {
                    if !ticket.tags().is_empty() {
                        parts.push(format!("[{}]", ticket.tags().join(", ")));
                    }
                }
                other => {
                    return Err(eyre!("Unknown column '{other}'"));
                }
            }
        }
        println!("{}", parts.join(" "));
    }

    Ok(())
}

pub fn cmd_ready(args: FilterArgs) -> color_eyre::Result<()> {
    let tickets = lock_tickets()?;
    if tickets.is_empty() {
        return Ok(());
    }

    let parent = if let Some(p) = args.parent {
        Some(resolve_partial_id(&tickets, &p)?)
    } else {
        None
    };

    let lookup: HashMap<_, _> = tickets.iter().map(|t| (t.id(), t)).collect();
    let status_filter = args.status;
    let ready: Vec<&Ticket> = tickets
        .iter()
        .filter(|ticket| {
            let status_match = if let Some(s) = status_filter {
                ticket.status() == &s
            } else {
                ticket.status() == &StatusValue::Open || ticket.status() == &StatusValue::InProgress
            };
            status_match
                && ticket_matches_filters(
                    &tickets,
                    ticket,
                    None,
                    args.assignee.as_deref(),
                    args.tags.first().map(|s| s.as_str()),
                    parent.as_deref(),
                )
        })
        .filter(|ticket| {
            ticket.deps().iter().all(|dep| {
                lookup.get(dep.as_str()).map(|t| t.status()) == Some(&StatusValue::Closed)
            })
        })
        .collect();

    for ticket in ready {
        if args.show_deps {
            println!(
                "{:<8} [P{}][{}] - {} (deps: {})",
                ticket.id(),
                ticket.priority(),
                ticket.status(),
                ticket.title,
                ticket.deps().len()
            );
        } else {
            println!(
                "{:<8} [P{}][{}] - {}",
                ticket.id(),
                ticket.priority(),
                ticket.status(),
                ticket.title
            );
        }
    }

    Ok(())
}

pub fn cmd_blocked(args: BlockedArgs) -> color_eyre::Result<()> {
    let tickets = lock_tickets()?;
    if tickets.is_empty() {
        return Ok(());
    }

    let parent = if let Some(p) = args.parent {
        Some(resolve_partial_id(&tickets, &p)?)
    } else {
        None
    };
    let lookup: HashMap<_, _> = tickets.iter().map(|t| (t.id(), t)).collect();
    let mut blocked: Vec<(&Ticket, Vec<String>)> = Vec::new();
    for ticket in tickets.iter() {
        if ticket.status() != &StatusValue::Open && ticket.status() != &StatusValue::InProgress {
            continue;
        }
        if !ticket_matches_filters(
            &tickets,
            ticket,
            None,
            args.assignee.as_deref(),
            args.tags.first().map(|s| s.as_str()),
            parent.as_deref(),
        ) {
            continue;
        }

        let mut blockers: Vec<String> = Vec::new();
        for dep in ticket.deps() {
            if let Some(t) = lookup.get(dep.as_str()) {
                if t.status() != &StatusValue::Closed
                    && (!args.only_open || t.status() == &StatusValue::Open)
                {
                    blockers.push(dep.clone());
                }
            } else {
                blockers.push(dep.clone());
            }
        }

        if !blockers.is_empty() {
            blockers.sort();
            blocked.push((ticket, blockers));
        }
    }

    for (ticket, blockers) in blocked {
        println!(
            "{:<8} [P{}][{}] - {} <- [{}]",
            ticket.id(),
            ticket.priority(),
            ticket.status(),
            ticket.title,
            blockers.join(", ")
        );
    }

    Ok(())
}

pub fn cmd_closed(args: ClosedArgs) -> color_eyre::Result<()> {
    let mut tickets = lock_tickets()?;
    if tickets.is_empty() {
        return Ok(());
    }

    let since = if let Some(since_str) = args.since.as_deref() {
        match OffsetDateTime::parse(since_str, &Rfc3339) {
            Ok(dt) => Some(dt),
            Err(_) => return Err(eyre!("invalid --since; expected RFC3339 timestamp")),
        }
    } else {
        None
    };

    let mut enriched: Vec<(Ticket, Option<OffsetDateTime>)> = Vec::new();
    for t in tickets.drain(..) {
        let mtime = fs::metadata(&t.path)
            .and_then(|m| m.modified())
            .ok()
            .map(OffsetDateTime::from);
        enriched.push((t, mtime));
    }

    enriched.sort_by(|(a, ma), (b, mb)| match (ma, mb) {
        (Some(ta), Some(tb)) => tb.cmp(ta).then_with(|| a.id().cmp(b.id())),
        (None, Some(_)) => std::cmp::Ordering::Greater,
        (Some(_), None) => std::cmp::Ordering::Less,
        (None, None) => a.id().cmp(b.id()),
    });

    let mut shown = 0u32;
    for (ticket, mtime) in enriched {
        if shown >= args.limit {
            break;
        }
        if ticket.status() != &StatusValue::Closed {
            continue;
        }
        if let Some(assignee) = args.assignee.as_deref()
            && ticket.assignee().map(|a| a != assignee).unwrap_or(true)
        {
            continue;
        }
        if let Some(tag) = args.tags.first()
            && !ticket.tags().iter().any(|t| t == tag)
        {
            continue;
        }
        if let (Some(since_dt), Some(modified)) = (since, mtime) {
            if modified < since_dt {
                continue;
            }
        } else if since.is_some() && mtime.is_none() {
            // missing mtime, treat as older than since
            continue;
        }

        println!(
            "{:<8} [{}] - {}",
            ticket.id(),
            ticket.status(),
            ticket.title
        );
        shown += 1;
    }

    Ok(())
}

pub fn cmd_show(args: ShowArgs) -> color_eyre::Result<()> {
    let tickets = lock_tickets()?;
    let lookup: HashMap<_, _> = tickets.iter().map(|t| (t.id(), t)).collect();
    let ticket = find_ticket(&tickets, &args.id)?;

    let body = ticket.body();

    if args.json {
        let payload = ticket_to_json(ticket, &lookup, body);
        println!(
            "{}",
            serde_json::to_string_pretty(&payload)
                .map_err(|e| eyre!("failed to serialize ticket: {e}"))?
        );
        return Ok(());
    }

    print_ticket(ticket, &lookup, body);
    Ok(())
}

pub fn print_ticket(ticket: &Ticket, lookup: &HashMap<&str, &Ticket>, body: &TicketBody) {
    println!("{} [{}] - {}", ticket.id(), ticket.status(), ticket.title);
    println!("Priority: P{}", ticket.priority());

    if let Some(created) = &ticket.created() {
        println!("Created: {created}");
    }
    if let Some(assignee) = ticket.assignee() {
        println!("Assignee: {assignee}");
    }
    if let Some(external) = &ticket.external_ref() {
        println!("External: {external}");
    }

    let mut parents: Vec<String> = lookup
        .values()
        .filter(|p| p.deps().iter().any(|d| d == ticket.id()))
        .map(|p| format!("{} ({})", p.id(), p.title))
        .collect();
    parents.sort();
    if !parents.is_empty() {
        println!("Parents: {}", parents.join(", "));
    }

    if ticket.deps().is_empty() {
        println!("Deps: -");
    } else {
        let deps: Vec<String> = ticket
            .deps()
            .iter()
            .map(|id| {
                let title = lookup
                    .get(id.as_str())
                    .map(|t| t.title.as_str())
                    .unwrap_or("?");
                format!("{id} ({title})")
            })
            .collect();
        println!("Deps: {}", deps.join(", "));
    }

    if ticket.links().is_empty() {
        println!("Links: -");
    } else {
        let links: Vec<String> = ticket
            .links()
            .iter()
            .map(|id| {
                let title = lookup
                    .get(id.as_str())
                    .map(|t| t.title.as_str())
                    .unwrap_or("?");
                format!("{id} ({title})")
            })
            .collect();
        println!("Links: {}", links.join(", "));
    }

    if ticket.tags().is_empty() {
        println!("Tags: -");
    } else {
        println!("Tags: [{}]", ticket.tags().join(", "));
    }

    println!("{}", body);
}

pub fn cmd_query(args: QueryArgs) -> color_eyre::Result<()> {
    let tickets = lock_tickets()?;
    let mut items: Vec<serde_json::Value> = tickets.iter().map(|t| t.to_json()).collect();
    items.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));

    if let Some(filter) = args.filter.as_deref() {
        let filtered = apply_query_filter(&items, filter)?;
        write_query_output(&filtered, args.format)
    } else {
        write_query_output(&items, args.format)
    }
}

pub fn cmd_tree(args: TreeArgs) -> color_eyre::Result<()> {
    let tickets = lock_tickets()?;
    let forest = assemble_ticket_forest(&tickets, args.status);
    print_forest(&forest);
    Ok(())
}

pub fn cmd_add_note(args: AddNoteArgs) -> color_eyre::Result<()> {
    let mut tickets = lock_tickets()?;
    let id = resolve_partial_id(&tickets, &args.id)?;

    let ticket = tickets
        .iter_mut()
        .find(|t| t.id() == id)
        .ok_or_else(|| eyre!("Error: ticket '{}' not found", id))?;

    let note_text = if let Some(text) = args.text {
        text
    } else {
        let mut buffer = String::new();
        io::stdin()
            .read_to_string(&mut buffer)
            .map_err(|e| eyre!("failed to read stdin: {e}"))?;
        if buffer.trim().is_empty() {
            return Err(eyre!("Error: no note provided"));
        }
        buffer
    };

    ticket.add_note(&note_text, args.tag.as_deref(), true)?;

    write_ticket(ticket)?;

    println!(
        "Note added to {}",
        ticket
            .path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
    );
    Ok(())
}

pub fn cmd_create(args: CreateArgs) -> color_eyre::Result<()> {
    let mut tickets = lock_tickets()?;
    let tickets_dir = tickets_dir()?;

    let title = args.title.trim();
    if title.is_empty() {
        return Err(eyre!("Title is required"));
    }
    // The title occupies the single `# ` line of the ticket file; an embedded
    // newline would let later lines be parsed as section delimiters.
    if title.contains('\n') {
        return Err(eyre!("Title must be a single line"));
    }
    let title = title.to_string();
    let assignee = args.assignee.or_else(git_user_name);
    let parent = if let Some(parent_raw) = args.parent.as_deref() {
        Some(resolve_partial_id(&tickets, parent_raw).map_err(|e| eyre!("{e}"))?)
    } else {
        None
    };
    let id = generate_id().map_err(|e| eyre!("failed to generate id: {e}"))?;
    let file_path = tickets_dir.join(format!("{id}.md"));
    let now = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .map_err(|e| eyre!("failed to format timestamp: {e}"))?;

    // Validate tags: disallow brackets/whitespace to avoid malformed frontmatter
    for tag in &args.tags {
        if tag.contains(['[', ']', ' ']) {
            return Err(eyre!(
                "invalid tag '{tag}': must not contain brackets or spaces"
            ));
        }
    }

    let file_body = if let Some(path) = args.body_from_file.as_ref() {
        Some(
            fs::read_to_string(path)
                .map_err(|e| eyre!("failed to read body file {}: {e}", path.display()))?,
        )
    } else {
        None
    };

    let description = merge_description(args.description.as_deref(), file_body.as_deref());
    if let Some(value) = description.as_deref() {
        validate_section_value(value, "--description/--body-from-file")?;
    }

    let implementation_plan = args.implementation_plan.as_deref().map(|s| s.trim());
    if let Some(value) = implementation_plan {
        validate_section_value(value, "--implementation-plan")?;
    }

    let acceptance = args.acceptance.as_deref().map(|s| s.trim());
    if let Some(value) = acceptance {
        validate_section_value(value, "--acceptance")?;
    }

    let ticket = Ticket {
        title: title.clone(),
        frontmatter: TicketFrontmatter {
            id: id.clone(),
            status: StatusValue::Open,
            deps: Vec::new(),
            links: Vec::new(),
            created: Some(now.clone()),
            r#type: Some(args.ticket_type.as_str().to_string()),
            priority: args.priority,
            assignee: assignee.clone(),
            external_ref: args.external_ref.clone(),
            tags: args.tags.clone(),
            closed_at: None,
        },
        body: TicketBody {
            description,
            implementation_plan: implementation_plan.map(str::to_string),
            acceptance: acceptance.map(str::to_string),
            notes: Vec::new(),
        },
        path: file_path.clone(),
    };

    write_ticket(&ticket)?;

    if let Some(parent_id) = parent {
        dep_add(&parent_id, &id, true, &mut tickets)?;
    };

    if let Some(edit) = args.edit.then(|| EditArgs {
        id: id.clone(),
        description: None,
        implementation_plan: None,
        acceptance: None,
        external_ref: None,
        body_from_file: None,
        interactive: true,
        print: false,
        force: false,
    }) {
        cmd_edit(edit)?;
    } else {
        println!("{id}");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{merge_description, section_update};

    // MC/DC for merge_description's match on (description, file_body):
    //   c1 = description.is_some()
    //   c2 = file_body.is_some()
    // All four combinations are distinct outcomes, so every combination is
    // exercised. Independence pairs (holding the other condition constant):
    //   c1: (Some, None)=Some vs (None, None)=None  (c2 held None)
    //   c2: (None, Some)=Some vs (None, None)=None  (c1 held None)
    // The (Some, Some) merge case is covered explicitly for completeness.
    #[test]
    fn merge_description_joins_both_trimmed() {
        assert_eq!(
            merge_description(Some("  Intro  "), Some("  Body  ")),
            Some("Intro\n\nBody".to_string())
        );
    }

    #[test]
    fn merge_description_description_only_isolates_c1() {
        assert_eq!(
            merge_description(Some("  Intro  "), None),
            Some("Intro".to_string())
        );
    }

    #[test]
    fn merge_description_file_only_isolates_c2() {
        assert_eq!(
            merge_description(None, Some("  Body  ")),
            Some("Body".to_string())
        );
    }

    #[test]
    fn merge_description_neither_is_none() {
        assert_eq!(merge_description(None, None), None);
    }

    #[test]
    fn section_update_absent_flag_leaves_untouched() {
        assert_eq!(section_update(None), None);
    }

    #[test]
    fn section_update_empty_value_clears() {
        assert_eq!(section_update(Some("   ")), Some(None));
        assert_eq!(section_update(Some("")), Some(None));
    }

    #[test]
    fn section_update_nonempty_value_replaces() {
        assert_eq!(
            section_update(Some("Replacement")),
            Some(Some("Replacement".to_string()))
        );
    }
}
