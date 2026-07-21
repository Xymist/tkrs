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
    let source = match orientation {
        Orientation::Normal => ChildSource::Deps(&lookup),
        Orientation::Inverted => ChildSource::Dependants(build_inverted_adjacency(tickets)),
    };

    eligible_roots(tickets, selection, orientation, None, &lookup)
        .into_iter()
        .map(|ticket| build_root_node(ticket, &source, selection))
        .collect()
}

/// Builds the single subtree rooted at `root_id`, walked in the requested
/// `orientation`. Every ticket whose id equals `root_id` becomes its own
/// root, in slice order, mirroring [`assemble_ticket_forest`]'s duplicate-id
/// handling. `selection` applies to the root exactly as it does to every
/// other level: a root that fails the filter yields an empty forest.
pub fn assemble_ticket_subtree(
    tickets: &[Ticket],
    selection: TicketSelection,
    orientation: Orientation,
    root_id: &str,
) -> Vec<TicketNode> {
    let lookup: HashMap<&str, &Ticket> = tickets.iter().map(|t| (t.id(), t)).collect();
    let source = match orientation {
        Orientation::Normal => ChildSource::Deps(&lookup),
        Orientation::Inverted => ChildSource::Dependants(build_inverted_adjacency(tickets)),
    };

    eligible_roots(tickets, selection, orientation, Some(root_id), &lookup)
        .into_iter()
        .map(|ticket| build_root_node(ticket, &source, selection))
        .collect()
}

/// The roots a forest walk starts from: every ticket matching `root_id` (in
/// slice order) when given, otherwise every ticket eligible under
/// `orientation`'s root rule. `selection` is applied to candidates in both
/// cases.
fn eligible_roots<'a>(
    tickets: &'a [Ticket],
    selection: TicketSelection,
    orientation: Orientation,
    root_id: Option<&str>,
    lookup: &HashMap<&str, &'a Ticket>,
) -> Vec<&'a Ticket> {
    match root_id {
        Some(id) => tickets
            .iter()
            .filter(|t| t.id() == id)
            .filter(|t| matches_selection(t, selection))
            .collect(),
        None => {
            let dependency_ids: HashSet<&str> = tickets
                .iter()
                .flat_map(|t| t.deps().iter().map(String::as_str))
                .collect();
            tickets
                .iter()
                .filter(|t| matches_selection(t, selection))
                .filter(|t| is_root(t, orientation, &dependency_ids, lookup))
                .collect()
        }
    }
}

/// Builds a fully expanded node for a single root ticket, seeding the
/// per-root `visited` guard with the root itself.
fn build_root_node(
    ticket: &Ticket,
    source: &ChildSource,
    selection: TicketSelection,
) -> TicketNode {
    let mut visited = HashSet::new();
    visited.insert(ticket.id().to_string());
    TicketNode {
        id: ticket.id().to_string(),
        summary: ticket.summary(),
        children: children_recursively(ticket, source, &mut visited, selection),
    }
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

/// A single ticket node in an assembled [`TicketGraph`], deduped by id
/// across the whole graph regardless of how many roots reach it.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphNode {
    pub id: String,
    pub summary: String,
}

/// A single directed edge in an assembled [`TicketGraph`]. Direction is
/// resolved at assembly time from the walk orientation (see
/// [`assemble_ticket_graph`]) rather than carried here.
#[derive(Debug, Clone, PartialEq)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
}

/// The full ticket dependency graph: every selection-passing node reachable
/// from the roots, deduped, and every selection-passing edge between them,
/// deduped and in discovery order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TicketGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

/// Assembles the ticket dependency graph for the given `orientation`,
/// optionally restricted to the subtree rooted at `root_id` (same root
/// resolution as [`assemble_ticket_subtree`] when `Some`, or
/// [`assemble_ticket_forest`]'s root rule when `None`).
///
/// Unlike a forest, a node is defined once globally: a ticket reachable from
/// more than one path (a diamond, or a dependency shared by two roots)
/// appears exactly once in `nodes`, but every selection-passing edge that
/// reaches it is still recorded in `edges`. A global `visited` set stops the
/// walk from re-descending into an already-explored node (also terminating
/// cycles), without dropping the edge that reached it again.
///
/// Edge direction always follows the walk itself (`from` = the node being
/// expanded, `to` = its child from [`ChildSource::children_of`]), which
/// resolves to `dependant --> dependency` under `Orientation::Normal` and
/// `dependency --> dependant` under `Orientation::Inverted`.
///
/// Diverges from `tk tree`'s root eligibility in one case: when `root_id` is
/// `None`, any selection-matching ticket the eligible-root walk never
/// reaches -- a pure dependency cycle with no eligible root under either
/// orientation, or a ticket whose only inbound edge comes from a ticket that
/// fails `selection` -- would otherwise vanish from the graph entirely.
/// After the eligible-root walk, every remaining selection-matching ticket
/// not yet visited is swept in, in slice order, and walked the same way, so
/// every selection-matching ticket appears as a node somewhere and every
/// selection-passing edge between represented tickets is recorded. This
/// fallback does not run when `root_id` is `Some`, so `--root` keeps
/// restricting the graph to the requested subtree exactly as it restricts a
/// `tk tree` subtree.
pub fn assemble_ticket_graph(
    tickets: &[Ticket],
    selection: TicketSelection,
    orientation: Orientation,
    root_id: Option<&str>,
) -> TicketGraph {
    let lookup: HashMap<&str, &Ticket> = tickets.iter().map(|t| (t.id(), t)).collect();
    let source = match orientation {
        Orientation::Normal => ChildSource::Deps(&lookup),
        Orientation::Inverted => ChildSource::Dependants(build_inverted_adjacency(tickets)),
    };

    let roots = eligible_roots(tickets, selection, orientation, root_id, &lookup);

    let mut graph = TicketGraph::default();
    let mut visited_nodes: HashSet<String> = HashSet::new();
    let mut visited_edges: HashSet<(String, String)> = HashSet::new();

    for root in roots {
        // A duplicate root id only pushes the shared node once, but each
        // duplicate ticket object still walks its own `deps()`, so the edge
        // walk always runs regardless of whether the node push happened.
        if visited_nodes.insert(root.id().to_string()) {
            graph.nodes.push(GraphNode {
                id: root.id().to_string(),
                summary: root.summary(),
            });
        }
        walk_graph_edges(
            root,
            &source,
            selection,
            &mut visited_nodes,
            &mut visited_edges,
            &mut graph,
        );
    }

    if root_id.is_none() {
        for ticket in tickets {
            if !matches_selection(ticket, selection) {
                continue;
            }
            // Unlike the eligible-root loop above, a ticket already reached
            // by that walk is fully explored already, so the fallback both
            // adds the node and walks its edges in one gate.
            if visited_nodes.insert(ticket.id().to_string()) {
                graph.nodes.push(GraphNode {
                    id: ticket.id().to_string(),
                    summary: ticket.summary(),
                });
                walk_graph_edges(
                    ticket,
                    &source,
                    selection,
                    &mut visited_nodes,
                    &mut visited_edges,
                    &mut graph,
                );
            }
        }
    }

    graph
}

/// Records every selection-passing edge from `ticket` to its children,
/// descending into a child only the first time it is seen (`visited_nodes`),
/// so a repeat path still contributes its edge without re-walking a subtree
/// that is already fully recorded.
fn walk_graph_edges<'a>(
    ticket: &'a Ticket,
    source: &ChildSource<'a>,
    selection: TicketSelection,
    visited_nodes: &mut HashSet<String>,
    visited_edges: &mut HashSet<(String, String)>,
    graph: &mut TicketGraph,
) {
    for child in source.children_of(ticket) {
        if !matches_selection(child, selection) {
            continue;
        }

        let edge = (ticket.id().to_string(), child.id().to_string());
        if !visited_edges.insert(edge.clone()) {
            continue;
        }
        graph.edges.push(GraphEdge {
            from: edge.0,
            to: edge.1,
        });

        if visited_nodes.insert(child.id().to_string()) {
            graph.nodes.push(GraphNode {
                id: child.id().to_string(),
                summary: child.summary(),
            });
            walk_graph_edges(
                child,
                source,
                selection,
                visited_nodes,
                visited_edges,
                graph,
            );
        }
    }
}

/// Prints a [`TicketGraph`] to stdout as a Mermaid flowchart.
pub fn print_mermaid(graph: &TicketGraph) {
    print!("{}", render_mermaid(graph));
}

/// Mermaid init directive forcing straight (non-curved) edge rendering,
/// emitted as the first line of every rendered graph.
const MERMAID_INIT_DIRECTIVE: &str = r#"%%{init: {"flowchart": {"curve": "linear"}}}%%"#;

/// Renders a [`TicketGraph`] as a Mermaid `flowchart TD`: the linear-curve
/// init directive, the `flowchart TD` header, a 4-space-indented node
/// definition block, a blank line, then a 4-space-indented edge block,
/// terminated by a trailing newline. A graph with no nodes renders just the
/// directive and header lines.
pub fn render_mermaid(graph: &TicketGraph) -> String {
    let mut out = format!("{MERMAID_INIT_DIRECTIVE}\nflowchart TD\n");
    if graph.nodes.is_empty() {
        return out;
    }

    let mermaid_ids = sanitize_mermaid_ids(&graph.nodes);

    for node in &graph.nodes {
        out.push_str(&format!(
            "    {}[\"{}\"]\n",
            mermaid_ids[&node.id],
            escape_mermaid_label(&node.summary)
        ));
    }

    if !graph.edges.is_empty() {
        out.push('\n');
        for edge in &graph.edges {
            out.push_str(&format!(
                "    {} --> {}\n",
                mermaid_ids[&edge.from], mermaid_ids[&edge.to]
            ));
        }
    }

    out
}

/// Maps each node's ticket id to a Mermaid-safe id: [`sanitize_mermaid_id`]
/// applied first, then a collision between two distinct ticket ids that
/// sanitize to the same string is disambiguated with a numeric `_N` suffix,
/// in node discovery order.
fn sanitize_mermaid_ids(nodes: &[GraphNode]) -> HashMap<String, String> {
    let mut mermaid_ids: HashMap<String, String> = HashMap::new();
    let mut seen: HashSet<String> = HashSet::new();

    for node in nodes {
        let base = sanitize_mermaid_id(&node.id);
        let mut candidate = base.clone();
        let mut suffix = 1;
        while !seen.insert(candidate.clone()) {
            suffix += 1;
            candidate = format!("{base}_{suffix}");
        }
        mermaid_ids.insert(node.id.clone(), candidate);
    }

    mermaid_ids
}

/// Sanitizes a ticket id into a Mermaid-safe node id: every character
/// outside `[A-Za-z0-9_]` becomes `_`, and the result is always prefixed
/// with `t_`. The prefix guards against three cases a bare sanitized id
/// cannot: colliding with a Mermaid grammar keyword (a ticket literally
/// named `end`), a leading digit, and an empty id.
fn sanitize_mermaid_id(id: &str) -> String {
    let mut sanitized = String::from("t_");
    sanitized.extend(id.chars().map(|c| {
        if c.is_ascii_alphanumeric() || c == '_' {
            c
        } else {
            '_'
        }
    }));
    sanitized
}

/// Escapes a label for Mermaid's quoted node-text form. `#` must be escaped
/// before `"` -- escaping `"` first would turn its own `#` into a second
/// escape sequence.
fn escape_mermaid_label(label: &str) -> String {
    label.replace('#', "#35;").replace('"', "#quot;")
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
    //
    // Decision E -- subtree/graph root eligibility (`eligible_roots`'s `Some(id)`
    // branch), two chained `.filter`s forming a short-circuit AND:
    //   subroot(t) = (t.id() == id) && matches_selection(t, selection)
    //   c1 = t.id() == id
    //   c2 = matches_selection(t, selection)
    // Independence pairs (outcome = becomes-a-subtree-root):
    //   c1: (T,T)=root vs (F,T)=not-root
    //       -> subtree_restricts_to_root_id_normal (rooted at "b": "b" matches
    //          and passes -> root; "a"/"c" fail the id -> not roots, same fixture)
    //   c2: (T,T)=root vs (T,F)=not-root
    //       -> subtree_restricts_to_root_id_normal (T,T) vs
    //          subtree_root_failing_status_filter_is_empty (closed "a" matches the
    //          id but fails Open selection)
    //
    // Decision F -- `sanitize_mermaid_id`'s per-character keep test, an OR whose
    // false decision maps the character to '_':
    //   keep(c) = c.is_ascii_alphanumeric() || c == '_'
    //   c1 = c.is_ascii_alphanumeric()
    //   c2 = c == '_'
    // Independence pairs (outcome = character-kept-verbatim):
    //   c1: (T,-)=kept vs (F,F)=replaced
    //       -> sanitize_mermaid_id_replaces_non_word_chars (alnum kept; '.'/'-'/'!' replaced)
    //   c2: (F,T)=kept vs (F,F)=replaced
    //       -> sanitize_mermaid_id_preserves_literal_underscore ('_' kept; '!' replaced)
    //
    // The `walk_graph_edges` and graph root-loop guards are single-condition
    // (branch coverage only): the selection guard by
    // graph_selection_excludes_filtered_node_and_its_edges; the visited_nodes
    // re-descent guard's skip arm (record the edge, don't re-walk) by
    // graph_diamond_preserves_both_edges_into_shared_dependency; the
    // visited_edges dedup skip arm by graph_duplicate_edges_are_recorded_once;
    // and the root-loop node dedup skip arm by
    // graph_duplicate_root_ids_dedupe_node_but_keep_each_branch.

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

    // --- Subtree assembly (--root) ------------------------------------------

    #[test]
    fn subtree_restricts_to_root_id_normal() {
        // `b` is not a forest root (`a` depends on it), but assembling its
        // subtree directly yields only `b` and its own dependency `c`.
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &["b"]),
            ticket("b", StatusValue::Open, 2, &["c"]),
            ticket("c", StatusValue::Open, 2, &[]),
        ];
        let subtree =
            assemble_ticket_subtree(&tickets, TicketSelection::All, Orientation::Normal, "b");
        assert_eq!(root_ids(&subtree), vec!["b"]);
        assert_eq!(child_ids(&subtree[0]), vec!["c"]);
    }

    #[test]
    fn subtree_restricts_to_root_id_inverted() {
        // Inverted: rooting at `m` walks its dependants only (`e`), ignoring
        // both `m`'s own dependency `l` and the rest of the forest.
        let tickets = vec![
            ticket("l", StatusValue::Open, 2, &[]),
            ticket("m", StatusValue::Open, 2, &["l"]),
            ticket("e", StatusValue::Open, 2, &["m"]),
        ];
        let subtree =
            assemble_ticket_subtree(&tickets, TicketSelection::All, Orientation::Inverted, "m");
        assert_eq!(root_ids(&subtree), vec!["m"]);
        assert_eq!(child_ids(&subtree[0]), vec!["e"]);
    }

    #[test]
    fn subtree_root_failing_status_filter_is_empty() {
        // `selection` applies to the root exactly like every other level: a
        // closed root under Open selection yields nothing, not a shallow tree.
        let tickets = vec![
            ticket("a", StatusValue::Closed, 2, &["b"]),
            ticket("b", StatusValue::Open, 2, &[]),
        ];
        let subtree =
            assemble_ticket_subtree(&tickets, TicketSelection::Open, Orientation::Normal, "a");
        assert!(subtree.is_empty());
    }

    #[test]
    fn subtree_duplicate_root_ids_each_keep_their_own_dependencies() {
        // Mirrors duplicate_ids_keep_their_own_dependencies for the subtree
        // path: both tickets sharing id "a" become their own root, in slice
        // order, each walking its own deps().
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &["x"]),
            ticket("a", StatusValue::Open, 2, &["y"]),
            ticket("x", StatusValue::Open, 2, &[]),
            ticket("y", StatusValue::Open, 2, &[]),
        ];
        let subtree =
            assemble_ticket_subtree(&tickets, TicketSelection::All, Orientation::Normal, "a");
        assert_eq!(root_ids(&subtree), vec!["a", "a"]);
        assert_eq!(child_ids(&subtree[0]), vec!["x"]);
        assert_eq!(child_ids(&subtree[1]), vec!["y"]);
    }

    #[test]
    fn subtree_unknown_root_id_yields_empty() {
        let tickets = vec![ticket("a", StatusValue::Open, 2, &[])];
        let subtree =
            assemble_ticket_subtree(&tickets, TicketSelection::All, Orientation::Normal, "ghost");
        assert!(subtree.is_empty());
    }

    // --- Graph assembly ------------------------------------------------------

    fn graph_node_ids(graph: &TicketGraph) -> Vec<&str> {
        graph.nodes.iter().map(|n| n.id.as_str()).collect()
    }

    fn graph_edge_pairs(graph: &TicketGraph) -> Vec<(&str, &str)> {
        graph
            .edges
            .iter()
            .map(|e| (e.from.as_str(), e.to.as_str()))
            .collect()
    }

    #[test]
    fn graph_diamond_preserves_both_edges_into_shared_dependency() {
        // A -> [B, C]; B -> D; C -> D. Unlike the forest, which collapses D
        // under whichever branch reaches it first, the graph dedupes D as a
        // single node but keeps both A->B->D and A->C->D edges into it.
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &["b", "c"]),
            ticket("b", StatusValue::Open, 2, &["d"]),
            ticket("c", StatusValue::Open, 2, &["d"]),
            ticket("d", StatusValue::Open, 2, &[]),
        ];
        let graph =
            assemble_ticket_graph(&tickets, TicketSelection::All, Orientation::Normal, None);
        assert_eq!(graph_node_ids(&graph), vec!["a", "b", "d", "c"]);
        assert_eq!(
            graph_edge_pairs(&graph),
            vec![("a", "b"), ("b", "d"), ("a", "c"), ("c", "d")]
        );
    }

    #[test]
    fn graph_edge_direction_normal_is_dependant_to_dependency() {
        let tickets = vec![
            ticket("l", StatusValue::Open, 2, &[]),
            ticket("m", StatusValue::Open, 2, &["l"]),
            ticket("e", StatusValue::Open, 2, &["m"]),
        ];
        let graph =
            assemble_ticket_graph(&tickets, TicketSelection::All, Orientation::Normal, None);
        assert_eq!(graph_edge_pairs(&graph), vec![("e", "m"), ("m", "l")]);
    }

    #[test]
    fn graph_edge_direction_inverted_is_dependency_to_dependant() {
        // Same fixture as the Normal case above: the arrow direction flips
        // to leaf-first while the underlying edges are unchanged.
        let tickets = vec![
            ticket("l", StatusValue::Open, 2, &[]),
            ticket("m", StatusValue::Open, 2, &["l"]),
            ticket("e", StatusValue::Open, 2, &["m"]),
        ];
        let graph =
            assemble_ticket_graph(&tickets, TicketSelection::All, Orientation::Inverted, None);
        assert_eq!(graph_edge_pairs(&graph), vec![("l", "m"), ("m", "e")]);
    }

    #[test]
    fn graph_restricts_to_root_id() {
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &["b"]),
            ticket("b", StatusValue::Open, 2, &["c"]),
            ticket("c", StatusValue::Open, 2, &[]),
            ticket("z", StatusValue::Open, 2, &[]),
        ];
        let graph = assemble_ticket_graph(
            &tickets,
            TicketSelection::All,
            Orientation::Normal,
            Some("b"),
        );
        assert_eq!(graph_node_ids(&graph), vec!["b", "c"]);
        assert_eq!(graph_edge_pairs(&graph), vec![("b", "c")]);
    }

    #[test]
    fn graph_root_id_on_duplicate_ticket_ids_dedupes_node_but_keeps_each_branch() {
        // The `Some(root_id)` branch of `eligible_roots` matches both tickets
        // sharing id "a"; the root loop's visited_nodes guard pushes "a" once
        // but still walks each duplicate's own deps() branch.
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &["x"]),
            ticket("a", StatusValue::Open, 2, &["y"]),
            ticket("x", StatusValue::Open, 2, &[]),
            ticket("y", StatusValue::Open, 2, &[]),
        ];
        let graph = assemble_ticket_graph(
            &tickets,
            TicketSelection::All,
            Orientation::Normal,
            Some("a"),
        );
        assert_eq!(graph_node_ids(&graph), vec!["a", "x", "y"]);
        assert_eq!(graph_edge_pairs(&graph), vec![("a", "x"), ("a", "y")]);
    }

    #[test]
    fn graph_pure_cycle_with_no_root_is_still_represented_normal() {
        // A -> B -> A has no eligible root under Normal (both are depended
        // upon by the other), so the eligible-root walk finds nothing. The
        // fallback sweep must still surface both nodes and both edges.
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &["b"]),
            ticket("b", StatusValue::Open, 2, &["a"]),
        ];
        let graph =
            assemble_ticket_graph(&tickets, TicketSelection::All, Orientation::Normal, None);
        assert_eq!(graph_node_ids(&graph), vec!["a", "b"]);
        assert_eq!(graph_edge_pairs(&graph), vec![("a", "b"), ("b", "a")]);
    }

    #[test]
    fn graph_pure_cycle_with_no_root_is_still_represented_inverted() {
        // Same cycle, inverted: neither ticket's own dep is unresolvable, so
        // neither is an inverted root either; the fallback sweep applies the
        // same way regardless of orientation.
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &["b"]),
            ticket("b", StatusValue::Open, 2, &["a"]),
        ];
        let graph =
            assemble_ticket_graph(&tickets, TicketSelection::All, Orientation::Inverted, None);
        assert_eq!(graph_node_ids(&graph), vec!["a", "b"]);
        assert_eq!(graph_edge_pairs(&graph), vec![("a", "b"), ("b", "a")]);
    }

    #[test]
    fn graph_fallback_sweep_skips_nodes_already_reached_from_a_root() {
        // `r` is a normal root reaching nothing; `a` <-> `b` is a rootless
        // cycle elsewhere in the same store. The fallback sweep must add the
        // cycle without duplicating `r`'s already-visited node/edges or
        // re-walking `a`/`b` more than once each.
        let tickets = vec![
            ticket("r", StatusValue::Open, 2, &[]),
            ticket("a", StatusValue::Open, 2, &["b"]),
            ticket("b", StatusValue::Open, 2, &["a"]),
        ];
        let graph =
            assemble_ticket_graph(&tickets, TicketSelection::All, Orientation::Normal, None);
        assert_eq!(graph_node_ids(&graph), vec!["r", "a", "b"]);
        assert_eq!(graph_edge_pairs(&graph), vec![("a", "b"), ("b", "a")]);
    }

    #[test]
    fn graph_selection_excludes_filtered_node_and_its_edges() {
        // `c` is closed; under Open selection it is dropped as a node and
        // both the edge into it and its own edge to `d` never appear. `d`
        // itself is Open and was never reached (its only path in was through
        // the filtered-out `c`), so the fallback sweep surfaces it as its own
        // disconnected node -- the same mechanism that recovers a rootless
        // cycle applies to any unreached selection-matching ticket.
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &["b", "c"]),
            ticket("b", StatusValue::Open, 2, &[]),
            ticket("c", StatusValue::Closed, 2, &["d"]),
            ticket("d", StatusValue::Open, 2, &[]),
        ];
        let graph =
            assemble_ticket_graph(&tickets, TicketSelection::Open, Orientation::Normal, None);
        assert_eq!(graph_node_ids(&graph), vec!["a", "b", "d"]);
        assert_eq!(graph_edge_pairs(&graph), vec![("a", "b")]);
    }

    #[test]
    fn graph_empty_when_no_tickets_match() {
        let graph = assemble_ticket_graph(&[], TicketSelection::All, Orientation::Normal, None);
        assert!(graph.nodes.is_empty());
        assert!(graph.edges.is_empty());
    }

    #[test]
    fn graph_duplicate_edges_are_recorded_once() {
        // `walk_graph_edges`' visited_edges guard: a ticket naming the same dep
        // twice yields the identical (from, to) edge twice; the second is
        // dropped, so the edge and its target node each appear once.
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &["d", "d"]),
            ticket("d", StatusValue::Open, 2, &[]),
        ];
        let graph =
            assemble_ticket_graph(&tickets, TicketSelection::All, Orientation::Normal, None);
        assert_eq!(graph_node_ids(&graph), vec!["a", "d"]);
        assert_eq!(graph_edge_pairs(&graph), vec![("a", "d")]);
    }

    #[test]
    fn graph_duplicate_root_ids_dedupe_node_but_keep_each_branch() {
        // Two roots sharing id "a" (a hand-edited store) each contribute their
        // own branch, but the root loop's visited_nodes guard pushes the shared
        // "a" node only once.
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &["x"]),
            ticket("a", StatusValue::Open, 2, &["y"]),
            ticket("x", StatusValue::Open, 2, &[]),
            ticket("y", StatusValue::Open, 2, &[]),
        ];
        let graph =
            assemble_ticket_graph(&tickets, TicketSelection::All, Orientation::Normal, None);
        assert_eq!(graph_node_ids(&graph), vec!["a", "x", "y"]);
        assert_eq!(graph_edge_pairs(&graph), vec![("a", "x"), ("a", "y")]);
    }

    // --- Mermaid rendering -----------------------------------------------

    #[test]
    fn render_mermaid_matches_expected_layout() {
        let graph = TicketGraph {
            nodes: vec![
                GraphNode {
                    id: "tic-5bbab".into(),
                    summary: "[P2] tic-5bbab: Mermaid graph output".into(),
                },
                GraphNode {
                    id: "tic-cbc40".into(),
                    summary: "[P2] tic-cbc40: Restrict tree output".into(),
                },
            ],
            edges: vec![GraphEdge {
                from: "tic-5bbab".into(),
                to: "tic-cbc40".into(),
            }],
        };
        assert_eq!(
            render_mermaid(&graph),
            "%%{init: {\"flowchart\": {\"curve\": \"linear\"}}}%%\n\
             flowchart TD\n\
             \x20   t_tic_5bbab[\"[P2] tic-5bbab: Mermaid graph output\"]\n\
             \x20   t_tic_cbc40[\"[P2] tic-cbc40: Restrict tree output\"]\n\
             \n\
             \x20   t_tic_5bbab --> t_tic_cbc40\n"
        );
    }

    #[test]
    fn render_mermaid_empty_graph_is_directive_and_header_only() {
        let graph = TicketGraph::default();
        assert_eq!(
            render_mermaid(&graph),
            "%%{init: {\"flowchart\": {\"curve\": \"linear\"}}}%%\nflowchart TD\n"
        );
    }

    #[test]
    fn render_mermaid_node_with_no_edges_omits_edge_block() {
        let graph = TicketGraph {
            nodes: vec![GraphNode {
                id: "a".into(),
                summary: "[P2] a: Title a".into(),
            }],
            edges: Vec::new(),
        };
        assert_eq!(
            render_mermaid(&graph),
            "%%{init: {\"flowchart\": {\"curve\": \"linear\"}}}%%\n\
             flowchart TD\n    t_a[\"[P2] a: Title a\"]\n"
        );
    }

    #[test]
    fn escape_mermaid_label_escapes_hash_before_quote() {
        // Order matters: escaping `#` first keeps a literal `"` mapping only
        // to `#quot;` instead of a doubly-escaped sequence.
        assert_eq!(
            escape_mermaid_label("titled \"epic\" #1"),
            "titled #quot;epic#quot; #35;1"
        );
    }

    #[test]
    fn sanitize_mermaid_id_replaces_non_word_chars() {
        assert_eq!(sanitize_mermaid_id("tic.5b-bab!"), "t_tic_5b_bab_");
    }

    #[test]
    fn sanitize_mermaid_id_preserves_literal_underscore() {
        // Decision F, c2=T: an existing underscore is kept verbatim rather than
        // treated as a non-word character, while `!` still maps to `_`.
        assert_eq!(sanitize_mermaid_id("a_b!"), "t_a_b_");
    }

    #[test]
    fn sanitize_mermaid_id_prefixes_a_reserved_mermaid_token() {
        // A ticket id that is itself a Mermaid grammar keyword (`end`) must
        // not surface as a bare node id; the `t_` prefix guards it.
        assert_eq!(sanitize_mermaid_id("end"), "t_end");
    }

    #[test]
    fn sanitize_mermaid_id_prefixes_an_empty_id() {
        assert_eq!(sanitize_mermaid_id(""), "t_");
    }

    #[test]
    fn sanitize_mermaid_id_prefixes_a_leading_digit() {
        // Without the prefix a leading digit would be a malformed identifier
        // in some Mermaid renderers.
        assert_eq!(sanitize_mermaid_id("123abc"), "t_123abc");
    }

    #[test]
    fn sanitize_mermaid_ids_disambiguates_collisions() {
        // Two distinct ticket ids that sanitize to the same string get a
        // numeric suffix on the second (and later) occurrence, applied after
        // the shared `t_` prefix.
        let nodes = vec![
            GraphNode {
                id: "tic.a".into(),
                summary: String::new(),
            },
            GraphNode {
                id: "tic!a".into(),
                summary: String::new(),
            },
        ];
        let ids = sanitize_mermaid_ids(&nodes);
        assert_eq!(ids["tic.a"], "t_tic_a");
        assert_eq!(ids["tic!a"], "t_tic_a_2");
    }
}
