//! Shared, presentation-independent ticket tree assembly.
//!
//! Both the TUI's tree pane and `tk tree` build the same forest: dependencies
//! nest under every ticket that depends on them and are omitted from the top
//! level, so the two stay visually consistent without duplicating the walk.

use std::collections::{HashMap, HashSet};

use clap::ValueEnum;

use crate::{Ticket, cli::StatusValue};

/// Status filter applied when assembling a ticket tree, at every level of nesting.
#[derive(ValueEnum, Copy, Clone, Debug, Default, PartialEq)]
#[value(rename_all = "kebab_case")]
pub enum TicketSelection {
    All,
    #[default]
    Open,
    InProgress,
    Closed,
}

/// A single node in an expanded ticket dependency tree, decoupled from any
/// particular rendering target (TUI widget or plain text).
#[derive(Debug, Clone, PartialEq)]
pub struct TicketNode {
    pub id: String,
    pub summary: String,
    pub children: Vec<TicketNode>,
}

/// Builds the forest of ticket trees: a ticket is a root unless some other
/// ticket lists it as a dependency, in which case it appears only nested
/// under its dependant(s). Within a single root's branch, a `visited` guard
/// prevents the same dependency id from being added twice, which also covers
/// dependency cycles.
pub fn assemble_ticket_forest(tickets: &[Ticket], selection: TicketSelection) -> Vec<TicketNode> {
    let lookup: HashMap<&str, &Ticket> = tickets.iter().map(|t| (t.id(), t)).collect();

    let dependency_ids: HashSet<&str> = tickets
        .iter()
        .flat_map(|t| t.deps().iter().map(String::as_str))
        .collect();

    tickets
        .iter()
        .filter(|t| matches_selection(t, selection))
        .filter(|t| !dependency_ids.contains(t.id()))
        .map(|ticket| {
            let mut visited = HashSet::new();
            visited.insert(ticket.id().to_string());
            TicketNode {
                id: ticket.id().to_string(),
                summary: ticket.summary(),
                children: deps_recursively(ticket, &lookup, &mut visited, selection),
            }
        })
        .collect()
}

fn deps_recursively(
    ticket: &Ticket,
    lookup: &HashMap<&str, &Ticket>,
    visited: &mut HashSet<String>,
    selection: TicketSelection,
) -> Vec<TicketNode> {
    let mut children = Vec::new();

    for dep_id in ticket.deps() {
        // Guard against cycles and repeated dependencies so a ticket is never
        // inserted more than once within the same branch.
        if !visited.insert(dep_id.clone()) {
            continue;
        }

        let Some(dep_ticket) = lookup.get(dep_id.as_str()) else {
            continue;
        };

        if !matches_selection(dep_ticket, selection) {
            continue;
        }

        children.push(TicketNode {
            id: dep_ticket.id().to_string(),
            summary: dep_ticket.summary(),
            children: deps_recursively(dep_ticket, lookup, visited, selection),
        });
    }

    children
}

fn matches_selection(ticket: &Ticket, selection: TicketSelection) -> bool {
    match selection {
        TicketSelection::All => true,
        TicketSelection::Open => ticket.status() != &StatusValue::Closed,
        TicketSelection::InProgress => ticket.status() == &StatusValue::InProgress,
        TicketSelection::Closed => ticket.status() == &StatusValue::Closed,
    }
}

/// Prints a forest to stdout as a fully expanded plain-text tree, using the
/// same box-drawing glyphs and four-char continuation indent as the `tree`
/// command. Roots are printed at column 0 with no glyph prefix.
pub fn print_forest(nodes: &[TicketNode]) {
    print!("{}", render_forest(nodes));
}

/// Renders a forest as the newline-terminated plain text `print_forest`
/// emits.
pub fn render_forest(nodes: &[TicketNode]) -> String {
    let mut out = String::new();
    for node in nodes {
        out.push_str(&node.summary);
        out.push('\n');
        render_children(&node.children, "", &mut out);
    }
    out
}

fn render_children(children: &[TicketNode], prefix: &str, out: &mut String) {
    let Some(last_index) = children.len().checked_sub(1) else {
        return;
    };

    for (index, child) in children.iter().enumerate() {
        let is_last = index == last_index;
        let connector = if is_last { "└── " } else { "├── " };
        out.push_str(&format!("{prefix}{connector}{}\n", child.summary));

        let child_prefix = if is_last {
            format!("{prefix}    ")
        } else {
            format!("{prefix}│   ")
        };
        render_children(&child.children, &child_prefix, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TicketBody, TicketFrontmatter};
    use std::path::PathBuf;

    fn ticket(id: &str, status: StatusValue, priority: u8, deps: &[&str]) -> Ticket {
        Ticket {
            title: format!("Title {id}"),
            frontmatter: TicketFrontmatter {
                id: id.to_string(),
                r#type: None,
                status,
                deps: deps.iter().map(|s| s.to_string()).collect(),
                links: Vec::new(),
                priority,
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
            path: PathBuf::from(format!("{id}.md")),
        }
    }

    fn root_ids(nodes: &[TicketNode]) -> Vec<&str> {
        nodes.iter().map(|n| n.id.as_str()).collect()
    }

    fn child_ids(node: &TicketNode) -> Vec<&str> {
        node.children.iter().map(|n| n.id.as_str()).collect()
    }

    fn find<'a>(nodes: &'a [TicketNode], id: &str) -> &'a TicketNode {
        nodes
            .iter()
            .find(|n| n.id == id)
            .unwrap_or_else(|| panic!("expected a root {id}"))
    }

    // Decision A -- a ticket becomes a top-level root iff:
    //   root(t) = matches_selection(t, selection) && !dependency_ids.contains(t.id())
    // (the two `.filter` calls form a short-circuit AND).
    //   c1 = matches_selection(t, selection)
    //   c2 = !dependency_ids.contains(t.id())   [t is depended-upon by no one]
    // Independence pairs (outcome = is-a-root):
    //   c1: (T,T)=root vs (F,T)=not-root
    //       -> selection_open_excludes_closed_at_top_level ("o" root vs "c" filtered, neither is a dep)
    //   c2: (T,T)=root vs (T,F)=not-root
    //       -> dependency_is_never_a_root ("a" root vs "b" which matches selection but is a dep)
    //
    // Decision B -- a dependency is added as a child iff (short-circuit `continue`s):
    //   child(dep) = visited.insert(dep) && lookup.get(dep).is_some() && matches_selection(dep, selection)
    //   c1 = visited.insert(dep)                 [dep not yet seen in this root's walk]
    //   c2 = lookup.get(dep).is_some()           [dep exists in the slice]
    //   c3 = matches_selection(dep, selection)
    // Independence pairs (outcome = dep-is-a-child):
    //   c1: (T,T,T)=child vs (F,-,-)=skipped
    //       -> repeated_dependency_within_a_root_is_collapsed_once ("d" kept under "b", pruned under "c")
    //   c2: (T,T,T)=child vs (T,F,-)=skipped
    //       -> dependency_missing_from_lookup_is_skipped ("ghost" absent from the slice)
    //   c3: (T,T,T)=child vs (T,T,F)=skipped
    //       -> selection_filters_nested_dependencies ("o"/"p" kept, closed "c" pruned)
    //
    // Decision C -- matches_selection is a multi-outcome match on `selection`,
    // with the Open arm being `status != Closed`. Discrete-outcome coverage per
    // arm (true and false rows) is provided by the selection_* top-level tests:
    //   All        -> selection_all_includes_every_status_at_top_level
    //   Open       -> selection_open_excludes_closed_at_top_level (open/in-progress true, closed false)
    //   InProgress -> selection_in_progress_includes_only_in_progress (in-progress true, others false)
    //   Closed     -> selection_closed_includes_only_closed (closed true, others false)

    #[test]
    fn dependency_is_never_a_root() {
        // A depends on B; B appears only nested under A, never at the top level.
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &["b"]),
            ticket("b", StatusValue::Open, 2, &[]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All);
        assert_eq!(root_ids(&forest), vec!["a"]);
        assert_eq!(child_ids(&forest[0]), vec!["b"]);
    }

    #[test]
    fn shared_dependency_nests_under_each_root() {
        // C is depended on by both roots A and B, so it appears once under each.
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &["c"]),
            ticket("b", StatusValue::Open, 2, &["c"]),
            ticket("c", StatusValue::Open, 2, &[]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All);
        assert_eq!(root_ids(&forest), vec!["a", "b"]);
        assert_eq!(child_ids(find(&forest, "a")), vec!["c"]);
        assert_eq!(child_ids(find(&forest, "b")), vec!["c"]);
    }

    #[test]
    fn repeated_dependency_within_a_root_is_collapsed_once() {
        // A -> [B, C]; B -> D; C -> D. The per-root visited set is consumed by
        // B's branch, so D does not reappear under C within the same root.
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &["b", "c"]),
            ticket("b", StatusValue::Open, 2, &["d"]),
            ticket("c", StatusValue::Open, 2, &["d"]),
            ticket("d", StatusValue::Open, 2, &[]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All);
        let a = find(&forest, "a");
        assert_eq!(child_ids(a), vec!["b", "c"]);
        assert_eq!(child_ids(&a.children[0]), vec!["d"]);
        assert!(
            a.children[1].children.is_empty(),
            "d must not reappear under c within root a"
        );
    }

    #[test]
    fn dependency_cycle_terminates_without_repeating() {
        // R -> A -> B -> A. The visited guard prunes the back-edge to A so the
        // walk terminates instead of looping.
        let tickets = vec![
            ticket("r", StatusValue::Open, 2, &["a"]),
            ticket("a", StatusValue::Open, 2, &["b"]),
            ticket("b", StatusValue::Open, 2, &["a"]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All);
        assert_eq!(root_ids(&forest), vec!["r"]);
        let a = &find(&forest, "r").children[0];
        assert_eq!(a.id, "a");
        assert_eq!(child_ids(a), vec!["b"]);
        assert!(
            a.children[0].children.is_empty(),
            "cycle back to a must be pruned"
        );
    }

    #[test]
    fn dependency_missing_from_lookup_is_skipped() {
        // A references a dep id absent from the slice; it is silently dropped.
        let tickets = vec![ticket("a", StatusValue::Open, 2, &["ghost"])];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All);
        assert_eq!(root_ids(&forest), vec!["a"]);
        assert!(forest[0].children.is_empty());
    }

    #[test]
    fn selection_all_includes_every_status_at_top_level() {
        // Input order is preserved (assemble does not sort), which also locks
        // behaviour 5's root ordering.
        let tickets = vec![
            ticket("o", StatusValue::Open, 2, &[]),
            ticket("p", StatusValue::InProgress, 2, &[]),
            ticket("c", StatusValue::Closed, 2, &[]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All);
        assert_eq!(root_ids(&forest), vec!["o", "p", "c"]);
    }

    #[test]
    fn selection_open_excludes_closed_at_top_level() {
        // Open = status != Closed: open and in-progress kept, closed dropped.
        let tickets = vec![
            ticket("o", StatusValue::Open, 2, &[]),
            ticket("p", StatusValue::InProgress, 2, &[]),
            ticket("c", StatusValue::Closed, 2, &[]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::Open);
        assert_eq!(root_ids(&forest), vec!["o", "p"]);
    }

    #[test]
    fn selection_in_progress_includes_only_in_progress() {
        let tickets = vec![
            ticket("o", StatusValue::Open, 2, &[]),
            ticket("p", StatusValue::InProgress, 2, &[]),
            ticket("c", StatusValue::Closed, 2, &[]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::InProgress);
        assert_eq!(root_ids(&forest), vec!["p"]);
    }

    #[test]
    fn selection_closed_includes_only_closed() {
        let tickets = vec![
            ticket("o", StatusValue::Open, 2, &[]),
            ticket("p", StatusValue::InProgress, 2, &[]),
            ticket("c", StatusValue::Closed, 2, &[]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::Closed);
        assert_eq!(root_ids(&forest), vec!["c"]);
    }

    #[test]
    fn selection_filters_nested_dependencies() {
        // Under Open selection the closed dep is pruned from A's children while
        // the open and in-progress deps remain.
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &["o", "p", "c"]),
            ticket("o", StatusValue::Open, 2, &[]),
            ticket("p", StatusValue::InProgress, 2, &[]),
            ticket("c", StatusValue::Closed, 2, &[]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::Open);
        assert_eq!(root_ids(&forest), vec!["a"]);
        assert_eq!(child_ids(&forest[0]), vec!["o", "p"]);
    }

    #[test]
    fn status_filtered_dependency_does_not_resurface_as_root() {
        // B is closed and a dep of A; under Open selection it is pruned from A's
        // children and must not appear as a top-level root either.
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &["b"]),
            ticket("b", StatusValue::Closed, 2, &[]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::Open);
        assert_eq!(root_ids(&forest), vec!["a"]);
        assert!(forest[0].children.is_empty());
    }

    #[test]
    fn root_order_follows_input_slice_order() {
        let tickets = vec![
            ticket("z", StatusValue::Open, 2, &[]),
            ticket("a", StatusValue::Open, 2, &[]),
            ticket("m", StatusValue::Open, 2, &[]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All);
        assert_eq!(root_ids(&forest), vec!["z", "a", "m"]);
    }

    #[test]
    fn child_order_follows_deps_order_not_id_order() {
        // deps stored as [c, b]; children preserve that order rather than sorting.
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &["c", "b"]),
            ticket("b", StatusValue::Open, 2, &[]),
            ticket("c", StatusValue::Open, 2, &[]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All);
        assert_eq!(child_ids(&forest[0]), vec!["c", "b"]);
    }

    #[test]
    fn node_summary_uses_priority_id_title_format() {
        let tickets = vec![ticket("abc", StatusValue::Open, 3, &[])];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All);
        assert_eq!(forest[0].summary, "[P3] abc: Title abc");
    }

    #[test]
    fn empty_slice_yields_empty_forest() {
        let forest = assemble_ticket_forest(&[], TicketSelection::All);
        assert!(forest.is_empty());
    }

    #[test]
    fn render_covers_all_connector_and_continuation_combinations() {
        // Two grandchildren under each of a non-last and a last branch, so all
        // four depth-2 prefixes appear: `│   ├── `, `│   └── `, `    ├── `,
        // and `    └── `.
        let tickets = vec![
            ticket("r", StatusValue::Open, 2, &["a", "b"]),
            ticket("a", StatusValue::Open, 2, &["a1", "a2"]),
            ticket("b", StatusValue::Open, 2, &["b1", "b2"]),
            ticket("a1", StatusValue::Open, 2, &[]),
            ticket("a2", StatusValue::Open, 2, &[]),
            ticket("b1", StatusValue::Open, 2, &[]),
            ticket("b2", StatusValue::Open, 2, &[]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All);
        assert_eq!(
            render_forest(&forest),
            "[P2] r: Title r\n\
             ├── [P2] a: Title a\n\
             │   ├── [P2] a1: Title a1\n\
             │   └── [P2] a2: Title a2\n\
             └── [P2] b: Title b\n\
             \x20   ├── [P2] b1: Title b1\n\
             \x20   └── [P2] b2: Title b2\n"
        );
    }
}
