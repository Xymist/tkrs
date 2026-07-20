//! A minimal ticket system with dependency tracking
//! Rewritten in Rust because why not?

use color_eyre::eyre::eyre;
use serde::ser::{SerializeSeq, Serializer};
use serde::{Deserialize, Serialize};
use serde_json::json;
use serde_json::ser::PrettyFormatter;
use std::collections::{HashMap, HashSet};
use std::fmt::Display;
use std::fs::{self as OsFs};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::Command as OsCommand;
use time::{OffsetDateTime, format_description};

use crate::cli::{QueryFormat, StatusValue};
use crate::fs::{lock_tickets, read_ticket, tickets_dir, write_ticket};

pub mod cli;
pub mod fs;
pub mod ids;
pub mod publish;
pub mod tree;
pub mod tui;

pub fn dep_add(
    id: &str,
    dep_id: &str,
    check_cycle: bool,
    tickets: &mut [Ticket],
) -> color_eyre::Result<()> {
    let ticket = tickets
        .iter_mut()
        .find(|t| t.id() == id)
        .ok_or_else(|| eyre!("ticket '{}' not found", id))?;

    let mut deps: HashSet<String> = ticket.deps().iter().cloned().collect();
    if !deps.insert(dep_id.to_string()) {
        return Err(eyre!("dependency already exists"));
    }

    let mut deps_vec: Vec<String> = deps.into_iter().collect();
    deps_vec.sort();

    let original = ticket.deps().to_vec();
    ticket.frontmatter.deps = deps_vec.clone();
    write_ticket(ticket)?;

    let cycle_guard = locate_cycles(tickets, true).map(|cycles| {
        cycles
            .iter()
            .filter(|cycle| cycle.contains(&id.to_string()) && cycle.contains(&dep_id.to_string()))
            .map(|cycle| canonicalize_cycle(cycle))
            .collect::<Vec<_>>()
    })?;

    if check_cycle && !cycle_guard.is_empty() {
        // revert on cycle detection
        let ticket = tickets
            .iter_mut()
            .find(|t| t.id() == id)
            .ok_or_else(|| eyre!("ticket '{}' not found", id))?;
        ticket.frontmatter.deps = original;
        write_ticket(ticket)?;
        return Err(eyre!(
            "cycle detected: {}",
            cycle_guard
                .iter()
                .map(|c| c.join(" -> "))
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }

    Ok(())
}

pub fn compute_updated_deps(
    ticket: &Ticket,
    mut mutator: impl FnMut(&mut Vec<String>) -> bool,
) -> (bool, Vec<String>) {
    let mut deps = ticket.deps().to_vec();
    let changed = mutator(&mut deps);
    deps.sort();
    deps.dedup();
    let changed = changed && deps != ticket.deps();
    (changed, deps)
}

pub fn resolve_dep_paths<'a>(
    id: &str,
    dep_id: &'a str,
) -> color_eyre::Result<(PathBuf, (&'a str, PathBuf))> {
    let ticket_path = resolve_ticket_path(id)?;
    let dep_path = resolve_ticket_path(dep_id)?;

    Ok((ticket_path, (dep_id, dep_path)))
}

pub fn write_ticket_links(path: &Path, links: &[String]) -> color_eyre::Result<()> {
    let mut ticket =
        read_ticket(path)?.ok_or_else(|| eyre!("ticket not found: {}", path.display()))?;
    ticket.frontmatter.links = links.to_vec();
    write_ticket(&ticket)?;
    Ok(())
}

type TicketGraph = HashMap<String, Vec<String>>;

pub fn read_ticket_graph(
    tickets: &[Ticket],
    include_closed: bool,
) -> color_eyre::Result<(Vec<Ticket>, TicketGraph)> {
    if tickets.is_empty() {
        return Ok((Vec::new(), HashMap::new()));
    }

    let filtered = filter_tickets(tickets, include_closed);

    let mut graph: HashMap<String, Vec<String>> = HashMap::new();
    let tickets_vec: Vec<Ticket> = filtered
        .map(|t| {
            graph.insert(t.id().to_string(), t.deps().to_vec());
            t.clone()
        })
        .collect();
    Ok((tickets_vec, graph))
}

pub fn filter_tickets(
    tickets: &[Ticket],
    include_closed: bool,
) -> Box<dyn Iterator<Item = &Ticket> + '_> {
    if include_closed {
        Box::new(tickets.iter())
    } else {
        Box::new(
            tickets
                .iter()
                .filter(|t| t.status() != &StatusValue::Closed),
        )
    }
}

pub fn canonicalize_cycle(cycle: &[String]) -> Vec<String> {
    let mut core: Vec<String> = if cycle.len() > 1 && cycle.first() == cycle.last() {
        cycle[..cycle.len() - 1].to_vec()
    } else {
        cycle.to_vec()
    };

    if core.is_empty() {
        return cycle.to_vec();
    }

    let (min_idx, _) = core
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| a.cmp(b))
        .unwrap();

    core.rotate_left(min_idx);
    let first = core.first().cloned().unwrap();
    core.push(first);
    core
}

#[derive(Debug, Clone)]
pub enum RemovalOutcome {
    Removed,
    NoChange,
}

pub fn remove_dependency(
    ticket_id: &str,
    dep_id: &str,
    tickets: &mut [Ticket],
) -> color_eyre::Result<RemovalOutcome> {
    let ticket = tickets
        .iter_mut()
        .find(|t| t.id() == ticket_id)
        .ok_or_else(|| eyre!("ticket '{}' not found", ticket_id))?;

    let (changed, deps) = compute_updated_deps(ticket, |deps| {
        let before = deps.len();
        deps.retain(|d| d != dep_id);
        before != deps.len()
    });

    if changed {
        ticket.frontmatter.deps = deps.clone();
        write_ticket(ticket)?;
        Ok(RemovalOutcome::Removed)
    } else {
        Ok(RemovalOutcome::NoChange)
    }
}

#[derive(Clone, Copy)]
pub enum LinkOp {
    Add,
    Remove,
}

pub struct LinkChange {
    ticket: Ticket,
    before: Vec<String>,
    after: Vec<String>,
}

pub struct LinkPlan {
    updates: Vec<LinkChange>,
    missing: Vec<String>,
}

pub fn build_link_plan(
    primary_id: &str,
    target_ids: &[String],
    op: LinkOp,
) -> color_eyre::Result<LinkPlan> {
    if target_ids.is_empty() {
        return Err(eyre!("at least one target is required"));
    }

    let mut unique_targets: Vec<String> = target_ids.to_vec();
    unique_targets.sort();
    unique_targets.dedup();

    let mut tickets: HashMap<String, Ticket> = HashMap::new();
    let primary = load_ticket_by_id(primary_id)?;
    tickets.insert(primary.id().to_string(), primary);

    for id in &unique_targets {
        let ticket = load_ticket_by_id(id)?;
        tickets.insert(ticket.id().to_string(), ticket);
    }

    let mut updates: Vec<LinkChange> = Vec::new();
    let mut missing: Vec<String> = Vec::new();

    let primary_ticket = tickets
        .get(primary_id)
        .ok_or_else(|| eyre!("primary ticket missing"))?;
    let mut set: HashSet<String> = primary_ticket.links().iter().cloned().collect();
    match op {
        LinkOp::Add => {
            for id in &unique_targets {
                set.insert(id.clone());
            }
        }
        LinkOp::Remove => {
            for id in &unique_targets {
                if !set.remove(id) {
                    missing.push(format!("{}<->{}", primary_ticket.id(), id));
                }
            }
        }
    }

    let mut after: Vec<String> = set.into_iter().collect();
    after.sort();
    after.dedup();

    if after != primary_ticket.links() {
        updates.push(LinkChange {
            ticket: primary_ticket.clone(),
            before: primary_ticket.links().to_vec(),
            after,
        });
    }

    for id in &unique_targets {
        let ticket = tickets
            .get(id)
            .ok_or_else(|| eyre!("ticket '{}' missing", id))?;
        let mut set: HashSet<String> = ticket.links().iter().cloned().collect();
        match op {
            LinkOp::Add => {
                set.insert(primary_id.to_string());
            }
            LinkOp::Remove => {
                if !set.remove(primary_id) {
                    missing.push(format!("{}<->{}", primary_id, ticket.id()));
                }
            }
        }

        let mut after: Vec<String> = set.into_iter().collect();
        after.sort();
        after.dedup();

        if after != ticket.links() {
            updates.push(LinkChange {
                ticket: ticket.clone(),
                before: ticket.links().to_vec(),
                after,
            });
        }
    }

    missing.sort();
    missing.dedup();

    Ok(LinkPlan { updates, missing })
}

pub fn ticket_matches_filters(
    tickets: &[Ticket],
    ticket: &Ticket,
    status: Option<StatusValue>,
    assignee: Option<&str>,
    tag: Option<&str>,
    parent: Option<&str>,
) -> bool {
    if let Some(s) = status
        && ticket.status() != &s
    {
        return false;
    }
    if let Some(a) = assignee
        && ticket.assignee().map(|v| v != a).unwrap_or(true)
    {
        return false;
    }
    if let Some(tag) = tag
        && !ticket.tags().iter().any(|t| t == tag)
    {
        return false;
    }
    if let Some(parent_id) = parent
        && !tickets
            .iter()
            .any(|t| t.id() == parent_id && t.deps().contains(&ticket.id().to_string()))
    {
        return false;
    }
    true
}

pub fn find_ticket<'a>(tickets: &'a [Ticket], input: &str) -> color_eyre::Result<&'a Ticket> {
    if let Some(t) = tickets.iter().find(|t| t.id() == input) {
        return Ok(t);
    }

    let mut matches: Vec<&Ticket> = tickets.iter().filter(|t| t.id().contains(input)).collect();
    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => Err(eyre!("Error: ticket '{input}' not found")),
        _ => Err(eyre!(
            "Error: ambiguous ID '{input}' matches multiple tickets"
        )),
    }
}

pub fn ticket_to_json(
    ticket: &Ticket,
    lookup: &HashMap<&str, &Ticket>,
    body: &TicketBody,
) -> serde_json::Value {
    let deps: Vec<serde_json::Value> = ticket
        .deps()
        .iter()
        .map(|id| {
            let resolved = lookup.get(id.as_str()).copied();
            json!({
                "id": id,
                "title": resolved.map(|t| t.title.as_str()),
                "status": resolved.map(|t| t.status()),
            })
        })
        .collect();

    let links: Vec<serde_json::Value> = ticket
        .links()
        .iter()
        .map(|id| {
            let resolved = lookup.get(id.as_str()).copied();
            json!({
                "id": id,
                "title": resolved.map(|t| t.title.as_str()),
                "status": resolved.map(|t| t.status()),
            })
        })
        .collect();

    let mut parents: Vec<serde_json::Value> = lookup
        .values()
        .filter(|t| t.deps().iter().any(|d| d == ticket.id()))
        .map(|t| {
            json!({
                "id": t.id(),
                "title": t.title.as_str(),
                "status": t.status(),
            })
        })
        .collect();
    parents.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));

    json!({
        "id": ticket.id(),
        "status": ticket.status(),
        "title": ticket.title,
        "priority": ticket.priority(),
        "assignee": ticket.assignee(),
        "tags": ticket.tags(),
        "created": ticket.created(),
        "external_ref": ticket.external_ref(),
        "parents": parents,
        "deps": deps,
        "links": links,
        "body": {
            "description": body.description,
            "implementation_plan": body.implementation_plan,
            "acceptance": body.acceptance,
            "notes": body.notes,
        }
    })
}

// Simple built-in filter language: field==value for exact match, field~substr for substring contains
// Supports nested fields with dot notation (e.g., tags==backend matches any array entry)
#[derive(Clone, Copy)]
pub enum Op {
    Eq,
    Contains,
}

pub fn apply_query_filter(
    items: &[serde_json::Value],
    filter: &str,
) -> color_eyre::Result<Vec<serde_json::Value>> {
    let (path, op, needle) = if let Some(rest) = filter.split_once("==") {
        (rest.0.trim(), Op::Eq, rest.1.trim())
    } else if let Some(rest) = filter.split_once('~') {
        (rest.0.trim(), Op::Contains, rest.1.trim())
    } else {
        return Err(eyre!(
            "invalid filter; expected field==value or field~value"
        ));
    };

    if path.is_empty() || needle.is_empty() {
        return Err(eyre!("filter parts must be non-empty"));
    }

    let keys: Vec<&str> = path.split('.').collect();

    pub fn value_matches(v: &serde_json::Value, op: Op, needle: &str) -> bool {
        match v {
            serde_json::Value::String(s) => match op {
                Op::Eq => s == needle,
                Op::Contains => s.contains(needle),
            },
            serde_json::Value::Number(n) => {
                if let Some(as_u64) = n.as_u64() {
                    match op {
                        Op::Eq => needle
                            .parse::<u64>()
                            .ok()
                            .map(|i| i == as_u64)
                            .unwrap_or(false),
                        Op::Contains => false,
                    }
                } else if let Some(as_i64) = n.as_i64() {
                    match op {
                        Op::Eq => needle
                            .parse::<i64>()
                            .ok()
                            .map(|i| i == as_i64)
                            .unwrap_or(false),
                        Op::Contains => false,
                    }
                } else if let Some(as_f64) = n.as_f64() {
                    match op {
                        Op::Eq => needle
                            .parse::<f64>()
                            .ok()
                            .map(|i| (i - as_f64).abs() < f64::EPSILON)
                            .unwrap_or(false),
                        Op::Contains => false,
                    }
                } else {
                    false
                }
            }
            serde_json::Value::Bool(b) => match op {
                Op::Eq => needle
                    .parse::<bool>()
                    .ok()
                    .map(|i| i == *b)
                    .unwrap_or(false),
                Op::Contains => false,
            },
            serde_json::Value::Array(arr) => arr.iter().any(|elem| value_matches(elem, op, needle)),
            serde_json::Value::Object(map) => {
                map.values().any(|elem| value_matches(elem, op, needle))
            }
            serde_json::Value::Null => false,
        }
    }

    let mut out = Vec::new();
    'outer: for item in items {
        let mut current = item;
        for key in &keys {
            match current {
                serde_json::Value::Object(map) => {
                    if let Some(next) = map.get(*key) {
                        current = next;
                    } else {
                        continue 'outer;
                    }
                }
                serde_json::Value::Array(arr) => {
                    if arr.iter().any(|v| match v {
                        serde_json::Value::Object(obj) => obj
                            .get(*key)
                            .map(|nested| value_matches(nested, op, needle))
                            .unwrap_or(false),
                        other => value_matches(other, op, needle),
                    }) {
                        // array satisfied via nested match
                        out.push(item.clone());
                        continue 'outer;
                    }
                    continue 'outer;
                }
                _ => continue 'outer,
            }
        }

        if value_matches(current, op, needle) {
            out.push(item.clone());
        }
    }

    Ok(out)
}

pub fn write_query_output(
    items: &[serde_json::Value],
    format: QueryFormat,
) -> color_eyre::Result<()> {
    match format {
        QueryFormat::Ndjson => {
            let stdout = io::stdout();
            let handle = stdout.lock();
            let mut writer = BufWriter::new(handle);
            for item in items {
                serde_json::to_writer(&mut writer, item)
                    .map_err(|e| eyre!("failed to write to stdout: {e}"))?;
                writer
                    .write_all(b"\n")
                    .map_err(|e| eyre!("failed to write to stdout: {e}"))?;
            }
            writer
                .flush()
                .map_err(|e| eyre!("failed to flush stdout: {e}"))?;
            Ok(())
        }
        QueryFormat::Pretty => {
            let mut buf = Vec::new();
            let formatter = PrettyFormatter::with_indent(b"  ");
            let mut ser = serde_json::Serializer::with_formatter(&mut buf, formatter);
            let mut seq = Serializer::serialize_seq(&mut ser, Some(items.len()))
                .map_err(|e| eyre!("failed to serialize sequence: {e}"))?;
            for item in items {
                SerializeSeq::serialize_element(&mut seq, item)
                    .map_err(|e| eyre!("failed to serialize element: {e}"))?;
            }
            SerializeSeq::end(seq).map_err(|e| eyre!("failed to end sequence: {e}"))?;
            io::stdout()
                .write_all(&buf)
                .map_err(|e| eyre!("failed to write to stdout: {e}"))?;
            io::stdout()
                .write_all(b"\n")
                .map_err(|e| eyre!("failed to write to stdout: {e}"))?;
            Ok(())
        }
    }
}

pub fn load_ticket_by_id(id: &str) -> color_eyre::Result<Ticket> {
    let path = resolve_ticket_path(id)?;
    read_ticket(&path)
        .map_err(|e| eyre!("failed to read ticket: {e}"))?
        .ok_or_else(|| eyre!("ticket missing id"))
}

pub fn parse_status(value: &str) -> color_eyre::Result<StatusValue> {
    let normalized = value.trim().to_ascii_lowercase().replace('-', "_");
    match normalized.as_str() {
        "open" => Ok(StatusValue::Open),
        "in_progress" => Ok(StatusValue::InProgress),
        "closed" => Ok(StatusValue::Closed),
        _ => Err(eyre!(
            "invalid status; must be one of: open, in_progress, closed"
        )),
    }
}

pub fn set_status_with_note(
    id: &str,
    status: StatusValue,
    note: Option<&str>,
    tickets: &mut [Ticket],
) -> color_eyre::Result<()> {
    let ticket = tickets
        .iter_mut()
        .find(|t| t.id() == id)
        .ok_or_else(|| eyre!("Error: ticket '{id}' not found"))?;

    let current_status = ticket.status();
    let status_change = current_status != &status;

    if !status_change && note.is_none() {
        return Ok(());
    }

    let tag = Some(format!("status_change: {} -> {}", current_status, status));
    ticket.update_status(status);
    if status == StatusValue::Closed {
        let now = OffsetDateTime::now_utc()
            .format(&format_description::well_known::Rfc3339)
            .unwrap_or_default();
        ticket.set_closed_at(Some(now));
    } else {
        ticket.set_closed_at(None);
    }
    ticket.add_note(
        note.unwrap_or(&format!("Status updated to {}", status)),
        tag.as_deref(),
        true,
    )?;

    crate::fs::write_ticket(ticket)?;

    Ok(())
}

pub fn resolve_ticket_path(input: &str) -> color_eyre::Result<PathBuf> {
    let dir = tickets_dir()?;
    let exact = dir.join(format!("{input}.md"));
    if exact.exists() {
        return Ok(exact);
    }

    let mut matches = Vec::new();
    if let Ok(read_dir) = OsFs::read_dir(&dir) {
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
        0 => Err(eyre!("Error: ticket '{input}' not found")),
        _ => Err(eyre!(
            "Error: ambiguous ID '{input}' matches multiple tickets"
        )),
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ticket {
    title: String,
    #[serde(flatten)]
    frontmatter: TicketFrontmatter,
    body: TicketBody,
    path: PathBuf,
}

impl Ticket {
    pub fn summary(&self) -> String {
        format!(
            "[P{}] {}: {}",
            self.frontmatter.priority,
            self.id(),
            self.title
        )
    }

    pub fn update_status(&mut self, new_status: StatusValue) {
        self.frontmatter.status = new_status;
    }

    pub fn set_closed_at(&mut self, value: Option<String>) {
        self.frontmatter.closed_at = value;
    }

    pub fn set_external_ref(&mut self, value: Option<String>) {
        self.frontmatter.external_ref = value;
    }

    pub fn set_description(&mut self, value: Option<String>) -> color_eyre::Result<()> {
        if let Some(v) = &value {
            validate_section_value(v, "description")?;
        }
        self.body.description = value;
        Ok(())
    }

    pub fn set_implementation_plan(&mut self, value: Option<String>) -> color_eyre::Result<()> {
        if let Some(v) = &value {
            validate_section_value(v, "implementation plan")?;
        }
        self.body.implementation_plan = value;
        Ok(())
    }

    pub fn set_acceptance(&mut self, value: Option<String>) -> color_eyre::Result<()> {
        if let Some(v) = &value {
            validate_section_value(v, "acceptance criteria")?;
        }
        self.body.acceptance = value;
        Ok(())
    }

    pub fn add_note(
        &mut self,
        note_text: &str,
        tag: Option<&str>,
        append_timestamp: bool,
    ) -> color_eyre::Result<()> {
        reject_reserved_headings(note_text, "note text")?;
        if let Some(t) = tag {
            // The tag is interpolated into the note's single `- [tag]` line; an
            // embedded newline would let the remainder be parsed as a section
            // delimiter.
            if t.trim().contains('\n') {
                return Err(eyre!("note tag must be a single line"));
            }
        }

        let mut combined = String::new();
        combined.push_str("- ");

        if let Some(t) = tag {
            combined.push('[');
            combined.push_str(t.trim());
            combined.push_str("] ");
        }

        combined.push_str(note_text.trim());

        if append_timestamp {
            let timestamp = OffsetDateTime::now_utc();
            let format = format_description::parse_borrowed::<2>(
                "[year]-[month]-[day] [hour]:[minute]:[second] UTC",
            )?;
            combined.push_str(" @ ");
            combined.push_str(&timestamp.format(&format)?);
        }

        let notes = self.body.notes_mut();
        if !combined.is_empty() {
            notes.push(combined);
        }

        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketFrontmatter {
    id: String,
    r#type: Option<String>,
    status: StatusValue,
    #[serde(default)]
    deps: Vec<String>,
    #[serde(default)]
    links: Vec<String>,
    priority: u8,
    assignee: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    created: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    closed_at: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    external_ref: Option<String>,
}

impl TicketFrontmatter {
    pub fn as_yaml(&self) -> color_eyre::Result<String> {
        Ok(format!(
            "---\n{}---\n",
            serde_yaml::to_string(&self).unwrap_or_default()
        ))
    }
}

pub const SECTION_PLACEHOLDER: &str = "-";

/// Heading lines that `fs::read_ticket` matches by prefix to switch which
/// section it is accumulating into. A supplied value containing a line
/// starting with one of these is indistinguishable from a real section
/// delimiter, so it would silently split the value across sections (or move
/// it into the wrong one) the next time the ticket file is read.
const RESERVED_SECTION_HEADINGS: &[(&str, &str, &str)] = &[
    (
        "## Implementation Plan",
        "Implementation Plan",
        "--implementation-plan",
    ),
    (
        "## Implementation plan",
        "Implementation Plan",
        "--implementation-plan",
    ),
    ("## Design", "Implementation Plan", "--implementation-plan"),
    (
        "## Acceptance Criteria",
        "Acceptance Criteria",
        "--acceptance",
    ),
    ("## Notes", "Notes", "`tk add-note`"),
];

/// Rejects `value` if it contains a line beginning with a reserved section
/// heading (see [`RESERVED_SECTION_HEADINGS`]). `field` names the flag or
/// input the value came from, for the error message.
pub fn reject_reserved_headings(value: &str, field: &str) -> color_eyre::Result<()> {
    for line in value.lines() {
        if let Some((heading, section, suggestion)) = RESERVED_SECTION_HEADINGS
            .iter()
            .find(|(heading, _, _)| line.starts_with(heading))
        {
            return Err(eyre!(
                "{field} contains a line beginning with the reserved heading \
                 '{heading}'; the ticket parser treats that as the start of \
                 the {section} section and would corrupt the ticket on the \
                 next read. Use {suggestion} to set the {section} section \
                 directly instead of embedding the heading text."
            ));
        }
    }
    Ok(())
}

/// Rejects `value` if, once trimmed, it is exactly [`SECTION_PLACEHOLDER`],
/// which `fs::read_ticket` reads back as an unset section rather than as
/// literal content.
pub fn reject_placeholder_value(value: &str, field: &str) -> color_eyre::Result<()> {
    if value.trim() == SECTION_PLACEHOLDER {
        return Err(eyre!(
            "{field} cannot be '{SECTION_PLACEHOLDER}': that value is reserved \
             as the empty-section placeholder; pass an empty string to clear \
             the section instead"
        ));
    }
    Ok(())
}

/// Validates a value destined for a ticket body section (description,
/// implementation plan, or acceptance criteria): rejects reserved section
/// headings and the literal placeholder value. See [`reject_reserved_headings`]
/// and [`reject_placeholder_value`].
pub fn validate_section_value(value: &str, field: &str) -> color_eyre::Result<()> {
    reject_reserved_headings(value, field)?;
    reject_placeholder_value(value, field)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TicketBody {
    description: Option<String>,
    implementation_plan: Option<String>,
    acceptance: Option<String>,
    #[serde(default)]
    notes: Vec<String>,
}

impl TicketBody {
    pub fn notes(&self) -> &Vec<String> {
        &self.notes
    }

    pub fn notes_mut(&mut self) -> &mut Vec<String> {
        &mut self.notes
    }
}

impl Display for TicketBody {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let description = self.description.as_deref().unwrap_or(SECTION_PLACEHOLDER);
        let implementation_plan = self
            .implementation_plan
            .as_deref()
            .unwrap_or(SECTION_PLACEHOLDER);
        let acceptance = self.acceptance.as_deref().unwrap_or(SECTION_PLACEHOLDER);
        let notes = if self.notes.is_empty() {
            SECTION_PLACEHOLDER.to_string()
        } else {
            self.notes.join("\n\n")
        };

        write!(
            f,
            "{description}\n\n## Implementation Plan\n\n{implementation_plan}\n\n## Acceptance Criteria\n\n{acceptance}\n\n## Notes\n\n{notes}\n"
        )
    }
}

#[derive(Clone, Copy)]
pub enum TicketSection {
    Description,
    ImplementationPlan,
    Acceptance,
    Notes,
}

impl Ticket {
    pub fn id(&self) -> &str {
        &self.frontmatter.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn status(&self) -> &StatusValue {
        &self.frontmatter.status
    }

    pub fn deps(&self) -> &[String] {
        &self.frontmatter.deps
    }

    pub fn links(&self) -> &[String] {
        &self.frontmatter.links
    }

    pub fn tags(&self) -> &[String] {
        &self.frontmatter.tags
    }

    pub fn created(&self) -> Option<&str> {
        self.frontmatter.created.as_deref()
    }

    pub fn external_ref(&self) -> Option<&str> {
        self.frontmatter.external_ref.as_deref()
    }

    pub fn priority(&self) -> u8 {
        self.frontmatter.priority
    }

    pub fn assignee(&self) -> Option<&str> {
        self.frontmatter.assignee.as_deref()
    }

    pub fn to_json(&self) -> serde_json::Value {
        json!({
            "id": self.id(),
            "status": self.status(),
            "title": self.title,
            "deps": self.deps(),
            "links": self.links(),
            "priority": self.priority(),
            "assignee": self.assignee(),
            "tags": self.tags(),
            "created": self.created(),
            "external_ref": self.external_ref(),
        })
    }

    pub fn description(&self) -> Option<&str> {
        self.body.description.as_deref()
    }

    pub fn implementation_plan(&self) -> Option<&str> {
        self.body.implementation_plan.as_deref()
    }

    pub fn acceptance(&self) -> Option<&str> {
        self.body.acceptance.as_deref()
    }

    pub fn notes(&self) -> &Vec<String> {
        self.body.notes()
    }

    pub fn body(&self) -> &TicketBody {
        &self.body
    }
}

pub fn parse_array_line(raw: &str) -> Vec<String> {
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

pub fn git_user_name() -> Option<String> {
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

pub fn dfs(
    node: &str,
    graph: &HashMap<String, Vec<String>>,
    state: &mut HashMap<String, u8>,
    stack: &mut Vec<String>,
    cycles: &mut Vec<Vec<String>>,
    seen: &mut HashSet<String>,
) {
    state.insert(node.to_string(), 1);
    stack.push(node.to_string());

    if let Some(neighbors) = graph.get(node) {
        for neigh in neighbors {
            if !graph.contains_key(neigh) {
                continue;
            }
            match state.get(neigh).copied().unwrap_or(0) {
                0 => dfs(neigh, graph, state, stack, cycles, seen),
                1 => {
                    if let Some(pos) = stack.iter().position(|p| p == neigh) {
                        let mut cycle = stack[pos..].to_vec();
                        cycle.push(neigh.clone());
                        let canonical = canonicalize_cycle(&cycle);
                        let key = canonical[..canonical.len() - 1].join("->");
                        if seen.insert(key) {
                            cycles.push(canonical);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    state.insert(node.to_string(), 2);
    stack.pop();
}

pub fn locate_cycles(
    tickets: &[Ticket],
    include_closed: bool,
) -> color_eyre::Result<Vec<Vec<String>>> {
    let (_, graph) = read_ticket_graph(tickets, include_closed)?;
    if graph.is_empty() {
        println!("No dependency cycles found");
        return Ok(Vec::new());
    }

    let mut state: HashMap<String, u8> = HashMap::new();
    let mut cycles: Vec<Vec<String>> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for node in graph.keys() {
        if state.get(node) == Some(&2) {
            continue;
        }
        dfs(
            node,
            &graph,
            &mut state,
            &mut Vec::new(),
            &mut cycles,
            &mut seen,
        );
    }

    Ok(cycles)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticket() -> Ticket {
        Ticket {
            title: "Test".to_string(),
            frontmatter: TicketFrontmatter {
                id: "t-test".to_string(),
                status: StatusValue::Open,
                deps: Vec::new(),
                links: Vec::new(),
                created: None,
                r#type: None,
                priority: 2,
                assignee: None,
                external_ref: None,
                tags: Vec::new(),
                closed_at: None,
            },
            body: TicketBody {
                description: None,
                implementation_plan: None,
                acceptance: None,
                notes: Vec::new(),
            },
            path: std::path::PathBuf::from("t-test.md"),
        }
    }

    #[test]
    fn setters_reject_reserved_heading_values() {
        let mut t = ticket();
        assert!(t.set_description(Some("x\n## Notes\ny".into())).is_err());
        assert!(
            t.set_implementation_plan(Some("## Acceptance Criteria".into()))
                .is_err()
        );
        assert!(t.set_acceptance(Some("## Design x".into())).is_err());
        assert!(t.body.description.is_none());
        assert!(t.body.implementation_plan.is_none());
        assert!(t.body.acceptance.is_none());
    }

    #[test]
    fn setters_reject_placeholder_values() {
        let mut t = ticket();
        assert!(t.set_description(Some(" - ".into())).is_err());
        assert!(t.set_acceptance(Some("-".into())).is_err());
        assert!(t.body.description.is_none());
        assert!(t.body.acceptance.is_none());
    }

    #[test]
    fn setters_accept_safe_values() {
        let mut t = ticket();
        t.set_description(Some("hello".into())).unwrap();
        t.set_acceptance(Some("### Notes lookalike".into()))
            .unwrap();
        assert_eq!(t.body.description.as_deref(), Some("hello"));
        assert_eq!(t.body.acceptance.as_deref(), Some("### Notes lookalike"));
    }
}
