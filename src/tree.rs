//! Shared, presentation-independent ticket tree assembly.
//!
//! Both the TUI's tree pane and `tk tree` build the same forest: dependencies
//! nest under every ticket that depends on them and are omitted from the top
//! level, so the two stay visually consistent without duplicating the walk.
//! `tk tree` can additionally walk the reversed graph (see [`Orientation`]);
//! the TUI always uses the normal orientation.

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

/// Which direction a ticket forest is assembled in.
///
/// `Normal` nests each ticket's dependencies under it, so a root is a
/// ticket no other ticket depends on. `Inverted` nests each ticket's
/// dependants under it instead, so a root is a ticket with no resolvable
/// dependency of its own (a leaf work item), and indentation grows toward
/// the epics that depend on it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Orientation {
    Normal,
    Inverted,
}

/// Builds the forest of ticket trees for the given `orientation`. Root
/// eligibility and the parent/child edge direction are both taken from the
/// unfiltered dependency graph before `selection` is applied:
///
/// - `Normal`: a ticket is a root unless some other ticket lists it as a
///   dependency; children of a node are its resolvable dependencies.
/// - `Inverted`: a ticket is a root if it has no resolvable dependency of
///   its own (its `deps()` is empty, or none of its dep ids resolve to a
///   ticket in the slice); children of a node are the tickets that depend
///   on it.
///
/// Within a single root's branch, a `visited` guard prevents the same
/// ticket id from being added twice, which also covers dependency cycles.
pub fn assemble_ticket_forest(
    tickets: &[Ticket],
    selection: TicketSelection,
    orientation: Orientation,
) -> Vec<TicketNode> {
    let lookup: HashMap<&str, &Ticket> = tickets.iter().map(|t| (t.id(), t)).collect();

    let dependency_ids: HashSet<&str> = tickets
        .iter()
        .flat_map(|t| t.deps().iter().map(String::as_str))
        .collect();

    let source = match orientation {
        Orientation::Normal => ChildSource::Deps(&lookup),
        Orientation::Inverted => ChildSource::Dependants(build_inverted_adjacency(tickets)),
    };

    tickets
        .iter()
        .filter(|t| matches_selection(t, selection))
        .filter(|t| is_root(t, orientation, &dependency_ids, &lookup))
        .map(|ticket| {
            let mut visited = HashSet::new();
            visited.insert(ticket.id().to_string());
            TicketNode {
                id: ticket.id().to_string(),
                summary: ticket.summary(),
                children: children_recursively(ticket, &source, &mut visited, selection),
            }
        })
        .collect()
}

/// Where a node's children come from during the walk. Normal children are
/// read from the ticket's own `deps()` rather than an id-keyed map so that
/// tickets sharing a duplicate id each keep their own dependency list.
enum ChildSource<'a> {
    Deps(&'a HashMap<&'a str, &'a Ticket>),
    Dependants(HashMap<&'a str, Vec<&'a Ticket>>),
}

impl<'a> ChildSource<'a> {
    /// Resolvable children of `ticket`, in `deps()` order (`Deps`) or input
    /// slice order (`Dependants`).
    fn children_of(&self, ticket: &'a Ticket) -> Vec<&'a Ticket> {
        match self {
            ChildSource::Deps(lookup) => ticket
                .deps()
                .iter()
                .filter_map(|dep_id| lookup.get(dep_id.as_str()).copied())
                .collect(),
            ChildSource::Dependants(adjacency) => {
                adjacency.get(ticket.id()).cloned().unwrap_or_default()
            }
        }
    }
}

/// Adjacency for the inverted orientation: each ticket maps to its
/// dependants, in input slice order (a dependant is pushed onto every
/// dependency it names, in the order it is visited).
fn build_inverted_adjacency(tickets: &[Ticket]) -> HashMap<&str, Vec<&Ticket>> {
    let mut adjacency: HashMap<&str, Vec<&Ticket>> = HashMap::new();
    for ticket in tickets {
        for dep_id in ticket.deps() {
            adjacency.entry(dep_id.as_str()).or_default().push(ticket);
        }
    }
    adjacency
}

fn is_root(
    ticket: &Ticket,
    orientation: Orientation,
    dependency_ids: &HashSet<&str>,
    lookup: &HashMap<&str, &Ticket>,
) -> bool {
    match orientation {
        Orientation::Normal => !dependency_ids.contains(ticket.id()),
        Orientation::Inverted => !ticket
            .deps()
            .iter()
            .any(|dep_id| lookup.contains_key(dep_id.as_str())),
    }
}

fn children_recursively<'a>(
    ticket: &'a Ticket,
    source: &ChildSource<'a>,
    visited: &mut HashSet<String>,
    selection: TicketSelection,
) -> Vec<TicketNode> {
    let mut children = Vec::new();

    for child in source.children_of(ticket) {
        // Guard against cycles and repeated edges so a ticket is never
        // inserted more than once within the same branch.
        if !visited.insert(child.id().to_string()) {
            continue;
        }

        if !matches_selection(child, selection) {
            continue;
        }

        children.push(TicketNode {
            id: child.id().to_string(),
            summary: child.summary(),
            children: children_recursively(child, source, visited, selection),
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

    // Decision A -- a ticket becomes a top-level root iff (the two `.filter`
    // calls form a short-circuit AND):
    //   root(t) = matches_selection(t, selection) && is_root(t, orientation, ..)
    //   c1 = matches_selection(t, selection)
    //   c2 = is_root(t, orientation, ..)         [orientation-dispatched, below]
    // `is_root` switches on `orientation`: the Normal arm is
    // `!dependency_ids.contains(t.id())` (t is depended-upon by no one); the
    // Inverted arm is its own decision, enumerated as Decision D.
    // Independence pairs (outcome = is-a-root), taken in the Normal arm:
    //   c1: (T,T)=root vs (F,T)=not-root
    //       -> selection_open_excludes_closed_at_top_level ("o" root vs "c" filtered, neither is a dep)
    //   c2: (T,T)=root vs (T,F)=not-root
    //       -> dependency_is_never_a_root ("a" root vs "b" which matches selection but is a dep)
    //
    // Decision B -- inside `children_recursively`, a candidate (already resolved
    // to a `&Ticket` by `ChildSource::children_of`) is added as a child iff
    // (short-circuit `continue`s):
    //   child(c) = visited.insert(c) && matches_selection(c, selection)
    //   c1 = visited.insert(c)                   [c not yet seen in this root's walk]
    //   c2 = matches_selection(c, selection)
    // Independence pairs (outcome = candidate-is-a-child):
    //   c1: (T,T)=child vs (F,-)=skipped
    //       -> repeated_dependency_within_a_root_is_collapsed_once ("d" kept under "b", pruned under "c")
    //          (inverted analogue: inverted_shared_dependant_collapses_within_a_root)
    //   c2: (T,T)=child vs (T,F)=skipped
    //       -> selection_filters_nested_dependencies ("o"/"p" kept, closed "c" pruned)
    //          (inverted analogue: inverted_open_selection_prunes_closed_dependant)
    // The separate single-condition decision in `ChildSource::children_of`'s
    // `Deps` arm (a dep id resolves via `lookup`) drops unknown dep ids before
    // the walk; branch coverage: dependency_missing_from_lookup_is_skipped.
    //
    // Decision D -- inverted-orientation root eligibility (`is_root`'s Inverted arm):
    //   root_inv(t) = !t.deps().iter().any(|d| lookup.contains_key(d))
    // The `.any` is an OR-fold over each dep's `lookup.contains_key(d)`; a root is
    // a ticket every one of whose deps is unresolvable (or which has no deps).
    //   c = lookup.contains_key(d)               [some dep d of t resolves in the slice]
    // Independence pair (outcome = is-a-root):
    //   c: all deps unresolvable (any=F) => root  vs  a resolvable dep present (any=T) => not-root
    //      -> inverted_all_ghost_deps_is_a_root vs inverted_resolvable_dep_is_not_a_root
    // The vacuous empty-deps case (any over [] = F => root) is covered by
    // inverted_leaf_with_no_deps_is_a_root.
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
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Normal);
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
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Normal);
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
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Normal);
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
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Normal);
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
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Normal);
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
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Normal);
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
        let forest = assemble_ticket_forest(&tickets, TicketSelection::Open, Orientation::Normal);
        assert_eq!(root_ids(&forest), vec!["o", "p"]);
    }

    #[test]
    fn selection_in_progress_includes_only_in_progress() {
        let tickets = vec![
            ticket("o", StatusValue::Open, 2, &[]),
            ticket("p", StatusValue::InProgress, 2, &[]),
            ticket("c", StatusValue::Closed, 2, &[]),
        ];
        let forest =
            assemble_ticket_forest(&tickets, TicketSelection::InProgress, Orientation::Normal);
        assert_eq!(root_ids(&forest), vec!["p"]);
    }

    #[test]
    fn selection_closed_includes_only_closed() {
        let tickets = vec![
            ticket("o", StatusValue::Open, 2, &[]),
            ticket("p", StatusValue::InProgress, 2, &[]),
            ticket("c", StatusValue::Closed, 2, &[]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::Closed, Orientation::Normal);
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
        let forest = assemble_ticket_forest(&tickets, TicketSelection::Open, Orientation::Normal);
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
        let forest = assemble_ticket_forest(&tickets, TicketSelection::Open, Orientation::Normal);
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
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Normal);
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
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Normal);
        assert_eq!(child_ids(&forest[0]), vec!["c", "b"]);
    }

    #[test]
    fn node_summary_uses_priority_id_title_format() {
        let tickets = vec![ticket("abc", StatusValue::Open, 3, &[])];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Normal);
        assert_eq!(forest[0].summary, "[P3] abc: Title abc");
    }

    #[test]
    fn empty_slice_yields_empty_forest() {
        let forest = assemble_ticket_forest(&[], TicketSelection::All, Orientation::Normal);
        assert!(forest.is_empty());
    }

    #[test]
    fn duplicate_ids_keep_their_own_dependencies() {
        // Two tickets sharing an id (possible in a hand-edited store) must each
        // walk their own deps() rather than aliasing through an id-keyed map.
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &["x"]),
            ticket("a", StatusValue::Open, 2, &["y"]),
            ticket("x", StatusValue::Open, 2, &[]),
            ticket("y", StatusValue::Open, 2, &[]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Normal);
        assert_eq!(root_ids(&forest), vec!["a", "a"]);
        assert_eq!(child_ids(&forest[0]), vec!["x"]);
        assert_eq!(child_ids(&forest[1]), vec!["y"]);
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
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Normal);
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

    // --- Inverted orientation ---------------------------------------------

    #[test]
    fn inverted_leaf_with_no_deps_is_a_root() {
        // Decision D, vacuous arm: `.any` over an empty deps list is false, so a
        // ticket with no dependency of its own is a root.
        let tickets = vec![ticket("l", StatusValue::Open, 2, &[])];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Inverted);
        assert_eq!(root_ids(&forest), vec!["l"]);
        assert!(forest[0].children.is_empty());
    }

    #[test]
    fn inverted_all_ghost_deps_is_a_root() {
        // Decision D, c=F: no dep resolves in the slice, so `.any` is false and
        // the ticket is still a root. Independence pair with
        // inverted_resolvable_dep_is_not_a_root.
        let tickets = vec![ticket("a", StatusValue::Open, 2, &["ghost1", "ghost2"])];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Inverted);
        assert_eq!(root_ids(&forest), vec!["a"]);
        assert!(forest[0].children.is_empty());
    }

    #[test]
    fn inverted_resolvable_dep_is_not_a_root() {
        // Decision D, c=T: one dep resolves (`b`), so `.any` is true and `a` is
        // not a root -- it nests under `b` as a dependant instead.
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &["ghost", "b"]),
            ticket("b", StatusValue::Open, 2, &[]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Inverted);
        assert_eq!(root_ids(&forest), vec!["b"]);
        assert_eq!(child_ids(&forest[0]), vec!["a"]);
    }

    #[test]
    fn inverted_children_are_dependants_in_input_slice_order() {
        // `a` is depended on by `c` then `b` (slice order c, b); its dependant
        // children preserve that input-slice order rather than id order.
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &[]),
            ticket("c", StatusValue::Open, 2, &["a"]),
            ticket("b", StatusValue::Open, 2, &["a"]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Inverted);
        assert_eq!(root_ids(&forest), vec!["a"]);
        assert_eq!(child_ids(&forest[0]), vec!["c", "b"]);
    }

    #[test]
    fn inverted_single_leaf_nests_all_dependants() {
        // Leaf `l` is depended on by both `x` and `y`; inverted, it is the sole
        // root and both dependants appear as its children.
        let tickets = vec![
            ticket("l", StatusValue::Open, 2, &[]),
            ticket("x", StatusValue::Open, 2, &["l"]),
            ticket("y", StatusValue::Open, 2, &["l"]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Inverted);
        assert_eq!(root_ids(&forest), vec!["l"]);
        assert_eq!(child_ids(find(&forest, "l")), vec!["x", "y"]);
    }

    #[test]
    fn inverted_shared_leaf_nests_epic_under_each_root() {
        // Epic `e` depends on leaves `l1` and `l2`; inverted, each leaf is its
        // own root and `e` nests once under each (mirror of
        // shared_dependency_nests_under_each_root).
        let tickets = vec![
            ticket("l1", StatusValue::Open, 2, &[]),
            ticket("l2", StatusValue::Open, 2, &[]),
            ticket("e", StatusValue::Open, 2, &["l1", "l2"]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Inverted);
        assert_eq!(root_ids(&forest), vec!["l1", "l2"]);
        assert_eq!(child_ids(find(&forest, "l1")), vec!["e"]);
        assert_eq!(child_ids(find(&forest, "l2")), vec!["e"]);
    }

    #[test]
    fn inverted_shared_dependant_collapses_within_a_root() {
        // Decision B, c1 (inverted): `e` depends on both `m1` and `m2`, which
        // both depend on `l`. Within root `l` the per-root visited set is
        // consumed by m1's branch, so `e` does not reappear under m2.
        let tickets = vec![
            ticket("l", StatusValue::Open, 2, &[]),
            ticket("m1", StatusValue::Open, 2, &["l"]),
            ticket("m2", StatusValue::Open, 2, &["l"]),
            ticket("e", StatusValue::Open, 2, &["m1", "m2"]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Inverted);
        let l = find(&forest, "l");
        assert_eq!(child_ids(l), vec!["m1", "m2"]);
        assert_eq!(child_ids(&l.children[0]), vec!["e"]);
        assert!(
            l.children[1].children.is_empty(),
            "e must not reappear under m2 within root l"
        );
    }

    #[test]
    fn inverted_dependency_cycle_terminates_without_repeating() {
        // Leaf `l`; `a` depends on [l, b]; `b` depends on `a`. Inverted the walk
        // is l -> a (dependant) -> b (dependant) -> a, and the back-edge to `a`
        // is pruned by the visited guard.
        let tickets = vec![
            ticket("l", StatusValue::Open, 2, &[]),
            ticket("a", StatusValue::Open, 2, &["l", "b"]),
            ticket("b", StatusValue::Open, 2, &["a"]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Inverted);
        assert_eq!(root_ids(&forest), vec!["l"]);
        let a = &find(&forest, "l").children[0];
        assert_eq!(a.id, "a");
        assert_eq!(child_ids(a), vec!["b"]);
        assert!(
            a.children[0].children.is_empty(),
            "cycle back to a must be pruned"
        );
    }

    #[test]
    fn inverted_open_selection_prunes_closed_dependant() {
        // Decision B, c2 (inverted): under Open selection a closed dependant is
        // pruned from its leaf's children.
        let tickets = vec![
            ticket("l", StatusValue::Open, 2, &[]),
            ticket("d", StatusValue::Closed, 2, &["l"]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::Open, Orientation::Inverted);
        assert_eq!(root_ids(&forest), vec!["l"]);
        assert!(forest[0].children.is_empty());
    }

    #[test]
    fn inverted_open_selection_excludes_closed_leaf_root() {
        // A closed leaf is a root by Decision D but filtered by selection at the
        // top level; the open leaf remains.
        let tickets = vec![
            ticket("c", StatusValue::Closed, 2, &[]),
            ticket("o", StatusValue::Open, 2, &[]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::Open, Orientation::Inverted);
        assert_eq!(root_ids(&forest), vec!["o"]);
    }

    #[test]
    fn inverted_multi_level_chain_reverses_normal() {
        // Regression guard: the same leaf->mid->epic chain nests the opposite way
        // under each orientation, over identical fixtures.
        let tickets = vec![
            ticket("l", StatusValue::Open, 2, &[]),
            ticket("m", StatusValue::Open, 2, &["l"]),
            ticket("e", StatusValue::Open, 2, &["m"]),
        ];
        let normal = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Normal);
        assert_eq!(root_ids(&normal), vec!["e"]);
        let nm = &find(&normal, "e").children[0];
        assert_eq!(nm.id, "m");
        assert_eq!(child_ids(nm), vec!["l"]);

        let inverted =
            assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Inverted);
        assert_eq!(root_ids(&inverted), vec!["l"]);
        let im = &find(&inverted, "l").children[0];
        assert_eq!(im.id, "m");
        assert_eq!(child_ids(im), vec!["e"]);
    }
}
