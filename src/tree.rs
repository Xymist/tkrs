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

/// Builds the forest of ticket trees for the given `orientation`. Regular
/// root eligibility and the parent/child edge direction are both taken from
/// the unfiltered dependency graph before `selection` is applied:
///
/// - `Normal`: a ticket is a root unless some other ticket lists it as a
///   dependency; children of a node are its resolvable dependencies.
/// - `Inverted`: a ticket is a root if it has no resolvable dependency of
///   its own (its `deps()` is empty, or none of its dep ids resolve to a
///   ticket in the slice); children of a node are the tickets that depend
///   on it.
///
/// Within a single root's branch, a path-local guard (reset on unwind, not
/// shared across siblings) prevents a ticket from being added twice along
/// the *same* ancestor chain -- which still terminates a dependency cycle --
/// while letting a ticket reachable from more than one branch of the same
/// root (a diamond) repeat once per branch, exactly like it already repeats
/// once per top-level root when two roots share a dependency.
///
/// Regular root eligibility being computed from the unfiltered graph can
/// leave a selection-matching ticket unrepresented in two situations: its
/// only dependant fails `selection` (an open dependency of a closed ticket
/// never resurfaces once its filtered gatekeeper is gone), or it belongs to
/// a pure dependency cycle with no eligible root in either direction (both
/// apply symmetrically under `Inverted`, e.g. an open epic whose only
/// resolvable dependency is a closed leaf). After the regular walk, every
/// remaining selection-matching ticket that also belongs to the appropriate
/// strongly-connected component of a selection-filtered view of the graph is
/// swept in, in slice order, as an additional fallback root: `Inverted` uses
/// [`is_in_sink_scc`] (a ticket whose own resolvable dependencies stay
/// within its SCC); `Normal` uses the dual [`is_in_source_scc`] (a ticket
/// whose dependants -- who depends on it -- stay within its SCC).
/// Restricting the fallback to the right kind of SCC membership (rather than
/// "not yet represented" alone) prevents a ticket merely upstream
/// (`Inverted`) or downstream (`Normal`) of a cycle from being wrongly
/// seeded as a second, standalone root before the cycle's own
/// representative is reached -- it always arrives nested beneath the
/// cycle's fallback root instead.
///
/// The rendered forest is path-expanded: a ticket appears once per distinct
/// root-to-node dependency path reaching it, so node count is bounded by the
/// number of simple paths through the graph, not the number of tickets --
/// on a densely layered diamond graph this grows rapidly. [`TicketGraph`]
/// (via [`assemble_ticket_graph`]) dedupes each ticket to a single node.
pub fn assemble_ticket_forest(
    tickets: &[Ticket],
    selection: TicketSelection,
    orientation: Orientation,
) -> Vec<TicketNode> {
    let lookup: HashMap<&str, &Ticket> = tickets.iter().map(|t| (t.id(), t)).collect();
    let adjacency = build_inverted_adjacency(tickets);
    let source = match orientation {
        Orientation::Normal => ChildSource::Deps(&lookup),
        Orientation::Inverted => ChildSource::Dependants(&adjacency),
    };

    let mut forest: Vec<TicketNode> =
        eligible_roots(tickets, selection, orientation, None, &lookup)
            .into_iter()
            .map(|ticket| build_root_node(ticket, &source, selection))
            .collect();

    let mut represented = HashSet::new();
    collect_node_ids(&forest, &mut represented);

    // The SCC eligibility checks run over a selection-filtered view: a
    // ticket that fails `selection` contributes no edges to this analysis
    // (mirroring how `children_recursively` already prunes past a filtered
    // ticket), so a closed gatekeeper never masks a dependency that becomes
    // structurally free once the gatekeeper itself is excluded.
    let filtered_lookup: HashMap<&str, &Ticket> = tickets
        .iter()
        .filter(|t| matches_selection(t, selection))
        .map(|t| (t.id(), t))
        .collect();
    let filtered_adjacency =
        build_inverted_adjacency(tickets.iter().filter(|t| matches_selection(t, selection)));

    for ticket in tickets {
        if !matches_selection(ticket, selection) || represented.contains(ticket.id()) {
            continue;
        }
        let eligible = match orientation {
            Orientation::Normal => is_in_source_scc(ticket, &filtered_adjacency, &filtered_lookup),
            Orientation::Inverted => is_in_sink_scc(ticket, &filtered_lookup),
        };
        if !eligible {
            continue;
        }
        let node = build_root_node(ticket, &source, selection);
        collect_node_ids(std::slice::from_ref(&node), &mut represented);
        forest.push(node);
    }

    forest
}

/// Builds the tree for `--root root_id`. `--root` selects a *scope* --
/// `root_id` plus its selection-filtered dependency closure, exactly the
/// ticket set the `Normal`-orientation walk below reaches -- and
/// `orientation` only changes how that fixed scope is presented:
///
/// - `Normal`: walks the scope directly. Every ticket whose id equals
///   `root_id` becomes its own root, in slice order, mirroring
///   [`assemble_ticket_forest`]'s duplicate-id handling. `selection` applies
///   to the root exactly as it does to every other level: a root that fails
///   the filter yields an empty forest, and the closure itself stops at any
///   ticket that fails `selection` (its own dependencies never enter the
///   scope).
/// - `Inverted`: first computes the scope via the `Normal` walk above, then
///   re-runs the unrestricted [`assemble_ticket_forest`] in `Inverted`
///   orientation over the scope alone. The scope's own leaves become roots
///   and nesting grows toward `root_id`, so a ticket outside the scope that
///   happens to depend on a scope member is never pulled in.
///   [`assemble_ticket_forest`] carries its own completeness guarantee (see
///   its doc comment), so a scope whose bottom is a dependency cycle is
///   still never silently dropped here, with no extra handling needed in
///   this function.
pub fn assemble_ticket_subtree(
    tickets: &[Ticket],
    selection: TicketSelection,
    orientation: Orientation,
    root_id: &str,
) -> Vec<TicketNode> {
    if orientation == Orientation::Inverted {
        let closure = assemble_ticket_subtree(tickets, selection, Orientation::Normal, root_id);
        let scope = scoped_tickets(tickets, &closure);
        // This delegates its fallback sweep to `assemble_ticket_forest`
        // (see its doc comment for the sink-SCC-eligible sweep it carries),
        // so a scope whose bottom is a dependency cycle is guaranteed
        // complete: every scope member ends up represented, in slice order.
        return assemble_ticket_forest(&scope, selection, Orientation::Inverted);
    }

    let lookup: HashMap<&str, &Ticket> = tickets.iter().map(|t| (t.id(), t)).collect();
    let source = ChildSource::Deps(&lookup);

    eligible_roots(tickets, selection, orientation, Some(root_id), &lookup)
        .into_iter()
        .map(|ticket| build_root_node(ticket, &source, selection))
        .collect()
}

/// The ticket subset for a `--root root_id` scope: every ticket from
/// `tickets` whose id appears anywhere in `closure` (the `Normal`-orientation
/// subtree already assembled for that root), preserving `tickets`' original
/// slice order rather than walk-discovery order. Membership is by id, so a
/// duplicate-id ticket enters the scope whenever any ticket sharing its id is
/// reached by the normal walk -- the same duplicate-id handling used
/// everywhere else in this module, just applied to a whole-slice filter
/// instead of a single walk.
fn scoped_tickets(tickets: &[Ticket], closure: &[TicketNode]) -> Vec<Ticket> {
    let mut closure_ids = HashSet::new();
    collect_node_ids(closure, &mut closure_ids);

    tickets
        .iter()
        .filter(|t| closure_ids.contains(t.id()))
        .cloned()
        .collect()
}

/// Collects the id of every node in `nodes` and all of their descendants,
/// recursively, into `ids`.
fn collect_node_ids(nodes: &[TicketNode], ids: &mut HashSet<String>) {
    for node in nodes {
        ids.insert(node.id.clone());
        collect_node_ids(&node.children, ids);
    }
}

/// True if `ticket` belongs to a sink strongly-connected component of
/// `scope_lookup`'s raw dependency graph -- every scope ticket transitively
/// reachable from `ticket` via resolvable `deps()` can also transitively
/// reach `ticket` back. A ticket with no resolvable in-scope dependency at
/// all is vacuously eligible (its forward-reachable set is empty), though in
/// practice such a leaf is already an ordinary `Inverted` root before the
/// fallback sweep in [`assemble_ticket_forest`] (also used by
/// [`assemble_ticket_subtree`]'s `Inverted` branch) runs.
///
/// This restricts the fallback sweep to genuine cycle (or singleton-leaf)
/// members: a ticket merely upstream of a cycle has a resolvable dependency
/// that can never reach it back, so it is excluded here and instead always
/// arrives as a dependant nested beneath the cycle's own fallback root.
///
/// Reachability is resolved through `scope_lookup`, which keeps one ticket
/// per id (last wins); for duplicate-id tickets with differing dep lists,
/// eligibility is computed against that single instance rather than the
/// union the rendering walk fans out over.
fn is_in_sink_scc(ticket: &Ticket, scope_lookup: &HashMap<&str, &Ticket>) -> bool {
    let forward = reachable_via_deps(ticket, scope_lookup);
    forward.iter().all(|id| {
        let Some(other) = scope_lookup.get(id.as_str()).copied() else {
            // Invariant: `forward` only ever contains ids that resolved via
            // `scope_lookup` in `reachable_via_deps`, so this never happens.
            return true;
        };
        reachable_via_deps(other, scope_lookup).contains(ticket.id())
    })
}

/// Every scope ticket id transitively reachable from `ticket` by following
/// resolvable `deps()` edges: a plain worklist DFS. Scopes assembled for a
/// single `--root` (or a selection-filtered pass over the whole store) are
/// small, so the O(n) traversal this performs per call (and the O(n^2) total
/// across [`is_in_sink_scc`]'s callers) is fine.
fn reachable_via_deps<'a>(
    ticket: &'a Ticket,
    scope_lookup: &HashMap<&str, &'a Ticket>,
) -> HashSet<String> {
    let mut visited = HashSet::new();
    let mut stack = vec![ticket];
    while let Some(current) = stack.pop() {
        for dep_id in current.deps() {
            let Some(dep_ticket) = scope_lookup.get(dep_id.as_str()).copied() else {
                continue;
            };
            if visited.insert(dep_id.clone()) {
                stack.push(dep_ticket);
            }
        }
    }
    visited
}

/// True if `ticket` belongs to a source strongly-connected component of the
/// raw dependency graph described by `adjacency` (dependants, as built by
/// [`build_inverted_adjacency`]) and `lookup` (id -> ticket) -- every ticket
/// transitively dependent on `ticket` can also be transitively reached back
/// from `ticket`, both via the dependants relation. This is the `Normal`
/// orientation's dual of [`is_in_sink_scc`]: a source SCC has no incoming
/// edge from outside itself, so seeding one representative per source SCC as
/// a fallback root never wrongly seeds a ticket that is merely downstream of
/// a cycle (something the cycle depends on) -- such a ticket instead always
/// arrives nested beneath the cycle's own fallback root, mirroring how
/// [`is_in_sink_scc`] excludes a ticket upstream of an inverted cycle.
///
/// As with [`is_in_sink_scc`], a ticket with no resolvable in-scope
/// dependant at all is vacuously eligible, and duplicate-id tickets are
/// resolved against whichever single instance `lookup` keeps (last wins).
fn is_in_source_scc<'a>(
    ticket: &'a Ticket,
    adjacency: &HashMap<&str, Vec<&'a Ticket>>,
    lookup: &HashMap<&str, &'a Ticket>,
) -> bool {
    let forward = reachable_via_dependants(ticket, adjacency);
    forward.iter().all(|id| {
        let Some(other) = lookup.get(id.as_str()).copied() else {
            // Invariant: `forward` only ever contains ids reached through
            // `adjacency`'s ticket references, all of which are members of
            // `lookup` too (built from the same, already-filtered tickets).
            return true;
        };
        reachable_via_dependants(other, adjacency).contains(ticket.id())
    })
}

/// Every ticket id transitively dependent on `ticket` -- reachable by
/// repeatedly following `adjacency` (the dependants relation, the mirror of
/// [`reachable_via_deps`]'s `deps()` walk): a plain worklist DFS with the
/// same complexity note.
fn reachable_via_dependants<'a>(
    ticket: &'a Ticket,
    adjacency: &HashMap<&str, Vec<&'a Ticket>>,
) -> HashSet<String> {
    let mut visited = HashSet::new();
    let mut stack = vec![ticket];
    while let Some(current) = stack.pop() {
        let Some(dependants) = adjacency.get(current.id()) else {
            continue;
        };
        for &dependant in dependants {
            if visited.insert(dependant.id().to_string()) {
                stack.push(dependant);
            }
        }
    }
    visited
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
/// path-local ancestor guard with the root itself.
fn build_root_node(
    ticket: &Ticket,
    source: &ChildSource,
    selection: TicketSelection,
) -> TicketNode {
    let mut path = HashSet::new();
    path.insert(ticket.id().to_string());
    TicketNode {
        id: ticket.id().to_string(),
        summary: ticket.summary(),
        children: children_recursively(ticket, source, &mut path, selection),
    }
}

/// Where a node's children come from during the walk. Normal children are
/// read from the ticket's own `deps()` rather than an id-keyed map so that
/// tickets sharing a duplicate id each keep their own dependency list.
enum ChildSource<'a> {
    Deps(&'a HashMap<&'a str, &'a Ticket>),
    Dependants(&'a HashMap<&'a str, Vec<&'a Ticket>>),
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
/// dependency it names, in the order it is visited). Generic over any
/// iterator of ticket references so callers can build adjacency over a
/// filtered subset (e.g. only selection-passing tickets) without cloning.
fn build_inverted_adjacency<'a>(
    tickets: impl IntoIterator<Item = &'a Ticket>,
) -> HashMap<&'a str, Vec<&'a Ticket>> {
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

/// Walks `ticket`'s children (per `source`), pruning only a candidate
/// already on the *current* ancestor `path` -- not one merely visited
/// elsewhere in this root's tree -- so a real dependency cycle still
/// terminates while a diamond (the same ticket reachable through two
/// distinct branches under the same root) renders once under each branch,
/// exactly like a shared dependency already repeats once per top-level
/// root. `path` is extended immediately before recursing into a child and
/// shrunk back immediately once that child's own recursion returns (not
/// deferred until every sibling is processed), so it always reflects
/// strictly the ancestor chain from the root down to the current frame --
/// never anything from an already-finished sibling branch, which would
/// otherwise block a *different* branch that legitimately reaches the same
/// ticket by a different path.
///
/// A separate `seen` set, local to this one call and never shared with any
/// recursive call, collapses a literal duplicate edge within the *same*
/// `children_of(ticket)` list (a hand-edited `deps: [b, b]`, or a duplicate
/// dependant edge under `Inverted`) to a single sibling, without touching
/// `path` and so without affecting cycle detection or cross-branch repeats.
///
/// `matches_selection` is checked before `seen` and `path`, not after: a
/// selection-failing candidate is rejected without ever occupying either
/// slot, so it can never block a distinct, selection-passing ticket that
/// happens to share its id (e.g. a duplicate-id store where a closed and an
/// open ticket of the same id are both dependants of the same dependency)
/// from being rendered when its turn comes.
fn children_recursively<'a>(
    ticket: &'a Ticket,
    source: &ChildSource<'a>,
    path: &mut HashSet<String>,
    selection: TicketSelection,
) -> Vec<TicketNode> {
    let mut children = Vec::new();
    let mut seen = HashSet::new();

    for child in source.children_of(ticket) {
        if !matches_selection(child, selection) {
            continue;
        }

        // Collapse a literal duplicate edge within this one child list (a
        // hand-edited `deps: [b, b]`, or a duplicate dependant edge under
        // `Inverted`) to a single sibling. `seen` is local to this call --
        // it never affects `path`, so it cannot mask a *different* branch
        // that legitimately reaches the same ticket (see `path` below).
        if !seen.insert(child.id().to_string()) {
            continue;
        }

        if !path.insert(child.id().to_string()) {
            continue;
        }

        children.push(TicketNode {
            id: child.id().to_string(),
            summary: child.summary(),
            children: children_recursively(child, source, path, selection),
        });

        path.remove(child.id());
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
///
/// `root_id: Some` and `Orientation::Inverted` combine the same way
/// [`assemble_ticket_subtree`] does: `--root` fixes the *scope* (`root_id`
/// plus its selection-filtered dependency closure, per the `Normal`
/// orientation), and `Inverted` only changes how that scope is presented --
/// the unrestricted graph assembly (fallback sweep included, so a cycle
/// inside the scope stays safe) runs over the scope alone, leaf-first, up to
/// `root_id`. A ticket outside the scope that depends on a scope member is
/// never pulled in.
pub fn assemble_ticket_graph(
    tickets: &[Ticket],
    selection: TicketSelection,
    orientation: Orientation,
    root_id: Option<&str>,
) -> TicketGraph {
    if orientation == Orientation::Inverted
        && let Some(id) = root_id
    {
        let closure = assemble_ticket_subtree(tickets, selection, Orientation::Normal, id);
        let scope = scoped_tickets(tickets, &closure);
        return assemble_ticket_graph(&scope, selection, Orientation::Inverted, None);
    }

    let lookup: HashMap<&str, &Ticket> = tickets.iter().map(|t| (t.id(), t)).collect();
    let adjacency = build_inverted_adjacency(tickets);
    let source = match orientation {
        Orientation::Normal => ChildSource::Deps(&lookup),
        Orientation::Inverted => ChildSource::Dependants(&adjacency),
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
    // (short-circuit `continue`s, in this order):
    //   child(c) = matches_selection(c, selection) && seen.insert(c) && path.insert(c)
    //   c1 = matches_selection(c, selection)     [c passes the status filter]
    //   c2 = seen.insert(c)                       [c not already emitted from
    //                                              THIS SAME children_of()
    //                                              call -- a fresh set per
    //                                              call, never shared with
    //                                              recursion, so it dedupes a
    //                                              literal duplicate edge
    //                                              without touching `path`]
    //   c3 = path.insert(c)                       [c not already on the
    //                                              CURRENT ancestor path --
    //                                              path-local, not per-root:
    //                                              inserted immediately
    //                                              before recursing into c
    //                                              and removed immediately
    //                                              after c's own recursion
    //                                              returns, before the next
    //                                              sibling is considered]
    // Independence pairs (outcome = candidate-is-a-child), each held against
    // the (F,F,F)=emit baseline realized by an ordinary kept child in the
    // cited fixture:
    //   c1: (T,F,F)=skipped vs (F,F,F)=child
    //       -> selection_filters_nested_dependencies ("o"/"p" kept, closed "c"
    //          rejected before `seen`/`path` are ever touched)
    //          (inverted analogue: inverted_open_selection_prunes_closed_dependant)
    //   c2: (F,T,F)=skipped vs (F,F,F)=child
    //       -> duplicate_dependency_entry_renders_as_a_single_sibling (the
    //          second "b" in `deps: [b, b]` is rejected by `seen` alone,
    //          without ever reaching `path`) and its inverted mirror
    //          inverted_duplicate_dependant_edge_renders_as_a_single_sibling
    //   c3: (F,F,T)=skipped vs (F,F,F)=child
    //       -> dependency_cycle_terminates_without_repeating (back-edge to "a"
    //          passes selection and is the only occurrence in this call's own
    //          children list, so `seen` passes too, but "a" is already on the
    //          ancestor path -- a real cycle)
    //          (inverted analogue: inverted_dependency_cycle_terminates_without_repeating)
    // `path` being scoped to the ancestor chain and shrunk back immediately
    // per child (not deferred until every sibling is processed) is what lets
    // a ticket reachable through two *distinct* branches under the same root
    // repeat once per branch instead of being wrongly collapsed to one: an
    // id removed from `path` the instant its own branch finishes is free
    // again for an unrelated, later branch to reach independently. Locked by
    // diamond_within_a_root_repeats_the_shared_dependency_under_each_branch,
    // its inverted mirror
    // inverted_diamond_within_a_root_repeats_the_shared_dependant_under_each_branch,
    // and (for the case where the shared descendant is listed as a *direct*
    // sibling edge as well as reachable through another sibling, so
    // `path`-removal timing specifically matters)
    // diamond_via_siblings_renders_identically_regardless_of_dep_order. The
    // c1-before-c2/c3 ordering -- a selection-failing candidate must not
    // occupy either slot -- is locked by
    // inverted_selection_failing_duplicate_id_dependant_does_not_block_open_copy
    // (a `seen`-then-`path` slot that a selection-failing duplicate-id
    // dependant must not consume).
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
    //
    // Decision G -- `assemble_ticket_graph`'s scope-then-invert guard, an AND
    // of two independent conditions:
    //   scope_invert(orientation, root_id) = (orientation == Inverted) && root_id.is_some()
    //   c1 = orientation == Inverted
    //   c2 = root_id.is_some()
    // Independence pairs (outcome = scope computed via the Normal closure,
    // then re-run as an unrestricted Inverted assembly over the scope alone):
    //   c1: (T,T)=scoped-invert vs (F,T)=graph_restricts_to_root_id (Normal
    //       orientation, root_id Some: no scoping needed, direct root walk)
    //   c2: (T,T)=scoped-invert vs (T,F)=graph_edge_direction_inverted_is_dependency_to_dependant
    //       (Inverted, root_id None: the ordinary unrestricted Inverted path)
    //   -> graph_root_id_inverted_scope_is_normal_closure_leaf_first covers (T,T)
    // `assemble_ticket_subtree`'s equivalent scope-then-invert *guard* is
    // single-condition (`root_id` is always fixed there, so only
    // `orientation` varies): covered by subtree_restricts_to_root_id_normal
    // (Normal) and subtree_root_id_inverted_scope_is_normal_closure_leaf_first
    // (Inverted). Its Inverted branch body then runs the two-condition
    // fallback sweep enumerated as Decision H below.
    //
    // Decision H -- `assemble_ticket_subtree`'s Inverted branch no longer
    // carries its own fallback sweep: it delegates to `assemble_ticket_forest`
    // over the `--root`-scoped slice, so the skip guard exercised here is
    // Decision J's, evaluated in the Inverted arm over a scope whose
    // `filtered_lookup` is that scope's selection-filtered id->ticket map. The
    // subtree_root_id_inverted_* tests below are therefore a second,
    // `--root`-scoped MC/DC pass over Decision J's guard (and, through c3, over
    // `is_in_sink_scc` -- Decision I). The guard is the same OR of three
    // independent conditions (c3 excludes a ticket merely upstream of a cycle:
    // it reaches a sink SCC via its deps without belonging to one, so it must
    // be rendered beneath the cycle's branch rather than seeded as a standalone
    // root):
    //   skip(t) = !matches_selection(t, selection)
    //             || represented.contains(t.id())
    //             || !is_in_sink_scc(t, filtered_lookup)
    //   c1 = !matches_selection(t, selection)    [t fails the status filter]
    //   c2 = represented.contains(t.id())        [t is already covered by an
    //                                             earlier root's branch]
    //   c3 = !is_in_sink_scc(t, filtered_lookup) [t is not part of a sink SCC:
    //                                             it has a resolvable dep that
    //                                             can never reach it back]
    // The false decision emits `t` as an additional fallback root;
    // `build_root_node` does NOT re-check `matches_selection`, so the c1 arm
    // is the only thing keeping a selection-failing scope member out of the
    // forest. Independence pairs (outcome = t is skipped, i.e. NOT emitted),
    // each held against the (F,F,F)=emit baseline realized by "a" in every
    // cited test:
    //   c1: (T,F,F)=skip vs (F,F,F)=emit
    //       -> subtree_root_id_inverted_fallback_skips_selection_failing_scope_member
    //          (closed duplicate "a", swept first while "a" is not yet
    //          represented and is itself in the sink SCC, is skipped by
    //          !matches_selection; the open "a" later at (F,F,F) is the copy
    //          actually emitted)
    //   c2: (F,T,F)=skip vs (F,F,F)=emit
    //       -> subtree_root_id_inverted_fallback_does_not_duplicate_already_represented_members
    //          ("m", a leaf with an empty (vacuously sink) forward-reachable
    //          set, is already represented -> skipped at (F,T,F); "a" is
    //          emitted at (F,F,F)) and subtree_root_id_inverted_scoped_cycle_is_not_silently_empty
    //          ("b" already represented -> skipped at (F,T,F) once "a" seeds
    //          the cycle)
    //   c3: (F,F,T)=skip vs (F,F,F)=emit
    //       -> subtree_root_id_inverted_epic_upstream_of_cycle_is_not_a_standalone_root
    //          ("r", upstream of cycle a<->b with no in-scope dependant of
    //          its own, is not yet represented and passes selection, but is
    //          excluded at (F,F,T) since it cannot reach itself back; "a" is
    //          emitted at (F,F,F))
    // Masking-MC/DC pass complete: all four rows above (F,F,F emit; T,F,F,
    // F,T,F, F,F,T skip) each map to two passing examples that hold the
    // remaining conditions per masking MC/DC. On any c2 row the OR
    // short-circuits at c2=T before c3 is evaluated, so c3 is masked there
    // and its would-be value is irrelevant to isolating c2: "r" in the
    // does-not-duplicate test (whose c3 would be T, being upstream of the
    // a<->b cycle) isolates c2 against (F,F,F) exactly as "m"/"b" (whose c3
    // would be F) do. "m" and "b" are cited for c2 as the clearest witnesses,
    // their masked c3 leaving the (F,T,F) row free of any incidental c3=T.
    //
    // Decision I -- `is_in_sink_scc`'s back-reachability check (the negated
    // `is_in_sink_scc` is Decision H's c3). It is an `.all()`-fold over one
    // condition per forward-reachable id; the `let Some(other) = .. else`
    // guard's `return true` covers an invariant `reachable_via_deps` never
    // violates (every id in `forward` resolved via `scope_lookup`), so it is
    // not an independent condition but dead-defensive true:
    //   sink(t) = reachable_via_deps(t).all(|id|
    //                 reachable_via_deps(scope_lookup[id]).contains(t.id()))
    //   c = reachable_via_deps(other, ..).contains(t.id())  [a forward-reachable
    //                                                        member reaches t back]
    // The fold is true iff every forward-reachable member reaches `t` back (t
    // sits in a sink SCC), and vacuously true when `t` has no resolvable
    // forward reach at all. Fold rows (outcome = t-is-in-a-sink-SCC), the
    // non-vacuous pair realized where Decision H actually evaluates c3
    // (c1=F, c2=F):
    //   c true for every member (fold T) => sink
    //       -> subtree_root_id_inverted_scoped_cycle_is_not_silently_empty
    //          ("a" in the a<->b cycle: both forward members reach it back, so
    //          is_in_sink_scc is true and "a" is seeded as a fallback root)
    //   c false for some member (fold F) => not-sink
    //       -> subtree_root_id_inverted_epic_upstream_of_cycle_is_not_a_standalone_root
    //          ("r" reaches "a"/"b" but neither reaches "r" back, so
    //          is_in_sink_scc is false and "r" is excluded from the sweep) and,
    //          directly at the forest level (no `--root` scoping),
    //          cycle_with_upstream_dep_seeds_at_cycle_not_standalone_inverted
    // The vacuous empty-forward arm (all() over [] = true) never reaches the
    // sweep -- such a leaf is already an eligible Inverted root, represented
    // before c3 is evaluated -- so it is exercised directly by
    // is_in_sink_scc_leaf_with_empty_forward_set_is_vacuously_true.
    //
    // Decision J -- `assemble_ticket_forest`'s own fallback-sweep skip guard,
    // generalizing Decision H's shape to both orientations and the
    // unrestricted case over a freshly selection-filtered lookup/adjacency
    // (rather than a `--root`-scoped one) -- an OR of three independent
    // conditions:
    //   skip(t) = !matches_selection(t, selection)
    //             || represented.contains(t.id())
    //             || !eligible(t, orientation)
    //   c1 = !matches_selection(t, selection)
    //   c2 = represented.contains(t.id())
    //   c3 = !eligible(t, orientation)   [is_in_sink_scc under Inverted,
    //                                    is_in_source_scc under Normal]
    // Independence pairs (outcome = t is skipped), each held against a
    // (F,F,F)=emit baseline:
    //   c1: (T,F,F)=skip vs (F,F,F)=emit
    //       -> open_dep_of_closed_dependant_appears_as_fallback_root_normal
    //          (closed "c" skipped by !matches_selection; open "d" emitted)
    //          and its inverted mirror
    //          open_dep_of_closed_dependant_appears_as_fallback_root_inverted
    //          (closed "l" skipped; open "e" emitted)
    //   c2: (F,T,F)=skip vs (F,F,F)=emit
    //       -> pure_cycle_with_no_root_renders_normal ("b" already
    //          represented by "a"'s own branch -> skipped; "a" emitted) and
    //          inverted_pure_cycle_with_no_root_renders (same shape, Inverted)
    //   c3: (F,F,T)=skip vs (F,F,F)=emit
    //       -> cycle_with_downstream_dep_seeds_at_cycle_not_standalone_normal
    //          ("z" is downstream of the cycle -- something depends on it,
    //          but it depends on nothing back -- so is_in_source_scc is
    //          false and "z" is excluded; "a" is emitted). c3's Inverted arm
    //          (!is_in_sink_scc) is mirrored directly by
    //          cycle_with_upstream_dep_seeds_at_cycle_not_standalone_inverted
    //          ("r" is upstream of cycle a<->b, reaching it via deps without
    //          being reached back, so is_in_sink_scc is false and "r" is
    //          excluded; "a" is emitted) and, over a `--root`-scoped input, by
    //          subtree_root_id_inverted_epic_upstream_of_cycle_is_not_a_standalone_root
    //          (Decision H).
    //
    // Decision K -- `is_in_source_scc`'s back-reachability check, the
    // `Normal`-orientation mirror of Decision I (same `.all()`-fold shape,
    // walking `adjacency`/dependants instead of `scope_lookup`/deps; the
    // `let Some(other) = .. else` guard is the same dead-defensive
    // invariant, not an independent condition):
    //   source(t) = reachable_via_dependants(t).all(|id|
    //                   reachable_via_dependants(lookup[id]).contains(t.id()))
    // Fold rows (outcome = t-is-in-a-source-SCC):
    //   c true for every member (fold T) => source
    //       -> pure_cycle_with_no_root_renders_normal ("a" in the a<->b
    //          cycle: both forward-dependant members reach it back)
    //   c false for some member (fold F) => not-source
    //       -> cycle_with_downstream_dep_seeds_at_cycle_not_standalone_normal
    //          ("z" is reached by "a" via dependants, but nothing depends on
    //          "a" back through "z", so the fold fails)
    // The vacuous empty-forward arm (all() over [] = true), mirroring
    // Decision I's own vacuous arm, is exercised directly (not through the
    // full sweep -- such a ticket is already an ordinary `Normal` root
    // before the sweep runs) by
    // is_in_source_scc_leaf_with_empty_forward_set_is_vacuously_true.

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
    fn diamond_within_a_root_repeats_the_shared_dependency_under_each_branch() {
        // A -> [B, C]; B -> D; C -> D. The path-local guard only tracks the
        // current ancestor chain, so D is not "on the path" once B's branch
        // unwinds -- it renders once under B and again under C, exactly like
        // a shared dependency already repeats once per top-level root.
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
        assert_eq!(
            child_ids(&a.children[1]),
            vec!["d"],
            "d must repeat under c too, not just under b"
        );
    }

    #[test]
    fn diamond_via_siblings_renders_identically_regardless_of_dep_order() {
        // Order-sensitivity regression: `e` depends on both `x` and `y`, and
        // `y` also depends on `x` (two paths reach `x`: directly from `e`,
        // and via `y`). `x` must appear once directly under `e` and once
        // nested under `y` in EITHER order `e` lists its own deps in -- a
        // `path` guard that removed a child's id only after every sibling
        // finished (rather than immediately after that child's own
        // recursion returns) would incorrectly block the nested occurrence
        // whenever `x` happened to be listed before `y`, since `x` would
        // still be "on the path" for the whole rest of `e`'s loop. This is
        // exactly the shape `tk create --parent` plus inter-child deps
        // produces, and `tk dep` sorts deps lexically, so both orders occur
        // in real stores.
        let x_listed_first = vec![
            ticket("e", StatusValue::Open, 2, &["x", "y"]),
            ticket("x", StatusValue::Open, 2, &[]),
            ticket("y", StatusValue::Open, 2, &["x"]),
        ];
        let forest =
            assemble_ticket_forest(&x_listed_first, TicketSelection::All, Orientation::Normal);
        let e = find(&forest, "e");
        assert_eq!(child_ids(e), vec!["x", "y"]);
        assert_eq!(
            child_ids(&e.children[1]),
            vec!["x"],
            "x must also nest under y when x is listed first"
        );

        let y_listed_first = vec![
            ticket("e", StatusValue::Open, 2, &["y", "x"]),
            ticket("x", StatusValue::Open, 2, &[]),
            ticket("y", StatusValue::Open, 2, &["x"]),
        ];
        let forest =
            assemble_ticket_forest(&y_listed_first, TicketSelection::All, Orientation::Normal);
        let e = find(&forest, "e");
        assert_eq!(child_ids(e), vec!["y", "x"]);
        assert_eq!(
            child_ids(&e.children[0]),
            vec!["x"],
            "x must also nest under y when y is listed first"
        );
    }

    #[test]
    fn duplicate_dependency_entry_renders_as_a_single_sibling() {
        // A hand-edited store can list the same dependency twice in one
        // ticket's own deps (`deps: [b, b]`). `ChildSource::Deps` resolves
        // both entries to the identical `b` object, so the same sibling
        // literally appears twice in one `children_of(a)` call. The
        // path-local guard must still collapse this to a single sibling --
        // deferring `path`'s removal until the whole call (not each child)
        // finishes is what keeps the second occurrence from finding an
        // empty slot and rendering `b` twice under the same parent, which
        // `tui-tree-widget` would reject as a duplicate sibling identifier.
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &["b", "b"]),
            ticket("b", StatusValue::Open, 2, &[]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Normal);
        assert_eq!(root_ids(&forest), vec!["a"]);
        assert_eq!(child_ids(&forest[0]), vec!["b"]);
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
    fn pure_cycle_with_no_root_renders_normal() {
        // A <-> B has no eligible root under Normal (both are depended upon
        // by the other), so the regular walk finds nothing. Quirk 1(b): the
        // fallback sweep's source-SCC eligibility check still surfaces the
        // whole cycle, seeded once, with the back-edge pruned.
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &["b"]),
            ticket("b", StatusValue::Open, 2, &["a"]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Normal);
        assert_eq!(root_ids(&forest), vec!["a"]);
        assert_eq!(child_ids(&forest[0]), vec!["b"]);
        assert!(
            forest[0].children[0].children.is_empty(),
            "back-edge to a must be pruned"
        );
    }

    #[test]
    fn open_dep_of_closed_dependant_appears_as_fallback_root_normal() {
        // Quirk 1(a): `c` (closed) depends on `d` (open). Under the default
        // Open view `c` is invisible, but the unfiltered `is_root` check
        // still disqualifies `d` from being a root, since something (`c`)
        // does depend on it -- even though that something is now gone from
        // view. Without the fallback sweep `d` would silently vanish; the
        // sweep's source-SCC check is vacuously true once the filtered-out
        // `c` contributes no edges, so `d` is seeded as its own root.
        let tickets = vec![
            ticket("c", StatusValue::Closed, 2, &["d"]),
            ticket("d", StatusValue::Open, 2, &[]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::Open, Orientation::Normal);
        assert_eq!(root_ids(&forest), vec!["d"]);
        assert!(forest[0].children.is_empty());
    }

    #[test]
    fn cycle_with_downstream_dep_seeds_at_cycle_not_standalone_normal() {
        // Invariant: a ticket downstream of a cycle (something the cycle
        // depends on) is never seeded as a standalone fallback root; it
        // nests beneath the cycle's own fallback root instead. Here `z` is a
        // leaf that the cycle `a<->b` depends on (`a`'s deps are `[b, z]`),
        // with `z` placed first in slice order so a naive "not yet
        // represented" check would otherwise seed it standalone before `a`
        // is reached. `is_in_source_scc` excludes `z`: something (`a`)
        // depends on it, but it cannot reach `a` back, so only `a` is seeded
        // and `z` is nested under it, appearing exactly once.
        let tickets = vec![
            ticket("z", StatusValue::Open, 2, &[]),
            ticket("a", StatusValue::Open, 2, &["b", "z"]),
            ticket("b", StatusValue::Open, 2, &["a"]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Normal);
        assert_eq!(root_ids(&forest), vec!["a"]);
        assert_eq!(child_ids(&forest[0]), vec!["b", "z"]);
        assert!(
            forest[0].children[0].children.is_empty(),
            "back-edge to a must be pruned"
        );
        assert!(
            forest[0].children[1].children.is_empty(),
            "z has no dependencies of its own"
        );
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
    fn inverted_diamond_within_a_root_repeats_the_shared_dependant_under_each_branch() {
        // Decision B, c1 (inverted) mirror of the Normal diamond test: `e`
        // depends on both `m1` and `m2`, which both depend on `l`. The
        // path-local guard lets `e` render once under `m1` and again under
        // `m2`, since `e` is not "on the path" once `m1`'s branch unwinds.
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
        assert_eq!(
            child_ids(&l.children[1]),
            vec!["e"],
            "e must repeat under m2 too, not just under m1"
        );
    }

    #[test]
    fn inverted_duplicate_dependant_edge_renders_as_a_single_sibling() {
        // Mirror of duplicate_dependency_entry_renders_as_a_single_sibling
        // for Inverted: `x` lists the same dependency (`l`) twice
        // (`deps: [l, l]`), so `build_inverted_adjacency` pushes `x` onto
        // `l`'s dependant list twice -- the same object appears twice in one
        // `children_of(l)` call. The path-local guard must still collapse
        // this to a single sibling under `l`.
        let tickets = vec![
            ticket("l", StatusValue::Open, 2, &[]),
            ticket("x", StatusValue::Open, 2, &["l", "l"]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Inverted);
        assert_eq!(root_ids(&forest), vec!["l"]);
        assert_eq!(child_ids(&forest[0]), vec!["x"]);
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
    fn inverted_pure_cycle_with_no_root_renders() {
        // A <-> B: neither ticket's own dep is unresolvable, so neither is
        // an Inverted root either. Quirk 1(b) under Inverted: the sink-SCC
        // fallback still surfaces the whole cycle, seeded once, with the
        // back-edge pruned.
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &["b"]),
            ticket("b", StatusValue::Open, 2, &["a"]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Inverted);
        assert_eq!(root_ids(&forest), vec!["a"]);
        assert_eq!(child_ids(&forest[0]), vec!["b"]);
        assert!(
            forest[0].children[0].children.is_empty(),
            "back-edge to a must be pruned"
        );
    }

    #[test]
    fn open_dep_of_closed_dependant_appears_as_fallback_root_inverted() {
        // Quirk 1(a) under Inverted, matching the ticket's own note: an open
        // epic (`e`) whose only resolvable dependency is a closed leaf
        // (`l`). `e` is not an Inverted root (it has a resolvable dep), and
        // `l` fails Open selection, so `e` would otherwise vanish entirely.
        // The sink-SCC check is vacuously true once `l` is excluded from the
        // filtered view (e's only dep no longer resolves), so `e` is seeded
        // as its own root -- alone, since Inverted's children are `e`'s
        // dependants and nothing depends on it.
        let tickets = vec![
            ticket("e", StatusValue::Open, 2, &["l"]),
            ticket("l", StatusValue::Closed, 2, &[]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::Open, Orientation::Inverted);
        assert_eq!(root_ids(&forest), vec!["e"]);
        assert!(forest[0].children.is_empty());
    }

    #[test]
    fn cycle_with_upstream_dep_seeds_at_cycle_not_standalone_inverted() {
        // Decision J, c3 independence pair (F,F,T) vs (F,F,F) under Inverted,
        // the direct-forest mirror of
        // cycle_with_downstream_dep_seeds_at_cycle_not_standalone_normal (whose
        // c3 arm is is_in_source_scc): `r` depends on the cycle `a<->b` and is
        // placed first in slice order. A naive "not yet represented" check
        // would seed `r` as its own standalone root before `a` is reached, and
        // `a`'s own inverted walk would then reach `r` again as a dependant.
        // `is_in_sink_scc` excludes `r` -- it has a resolvable dependency `a`
        // that can never reach it back -- so only `a` is seeded (F,F,F) and `r`
        // nests beneath it, appearing exactly once. This also exercises
        // Decision I's false fold row directly at the forest level, not through
        // the `--root`-scoped subtree entry point.
        let tickets = vec![
            ticket("r", StatusValue::Open, 2, &["a"]),
            ticket("a", StatusValue::Open, 2, &["b"]),
            ticket("b", StatusValue::Open, 2, &["a"]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::All, Orientation::Inverted);
        assert_eq!(root_ids(&forest), vec!["a"]);
        assert_eq!(child_ids(&forest[0]), vec!["r", "b"]);
        assert!(
            forest[0].children[0].children.is_empty(),
            "r has no in-scope dependants of its own"
        );
        assert!(
            forest[0].children[1].children.is_empty(),
            "back-edge to a must be pruned"
        );
    }

    #[test]
    fn inverted_selection_failing_duplicate_id_dependant_does_not_block_open_copy() {
        // Decision B, c1-before-c2 ordering: `x` is a leaf depended on by two
        // *distinct* tickets that both share id "d" -- one closed, one open.
        // `ChildSource::Dependants` never collapses duplicate ids (unlike
        // `Deps`'s lookup), so both appear as separate candidates under `x`.
        // With selection checked before the path guard, the closed copy is
        // rejected without ever touching the path, so it can never block the
        // open copy (sharing the same id) from occupying that slot.
        let tickets = vec![
            ticket("x", StatusValue::Open, 2, &[]),
            ticket("d", StatusValue::Closed, 5, &["x"]),
            ticket("d", StatusValue::Open, 2, &["x"]),
        ];
        let forest = assemble_ticket_forest(&tickets, TicketSelection::Open, Orientation::Inverted);
        assert_eq!(root_ids(&forest), vec!["x"]);
        assert_eq!(child_ids(&forest[0]), vec!["d"]);
        assert_eq!(
            forest[0].children[0].summary, "[P2] d: Title d",
            "the open copy (P2), not the closed copy (P5), must be the one rendered"
        );
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
    fn subtree_root_id_inverted_scope_is_normal_closure_leaf_first() {
        // `--root e` fixes the SCOPE to `e`'s normal-orientation closure
        // ({l, m, e}); `--inverted` only changes how that fixed scope is
        // presented -- leaf `l` becomes the root and nesting descends toward
        // `e` at the bottom, rather than walking `e`'s (nonexistent)
        // dependants.
        let tickets = vec![
            ticket("l", StatusValue::Open, 2, &[]),
            ticket("m", StatusValue::Open, 2, &["l"]),
            ticket("e", StatusValue::Open, 2, &["m"]),
        ];
        let subtree =
            assemble_ticket_subtree(&tickets, TicketSelection::All, Orientation::Inverted, "e");
        assert_eq!(root_ids(&subtree), vec!["l"]);
        let m_node = &subtree[0].children[0];
        assert_eq!(m_node.id, "m");
        assert_eq!(child_ids(m_node), vec!["e"]);
    }

    #[test]
    fn subtree_root_id_inverted_on_a_leaf_yields_just_the_leaf() {
        // The scope is the leaf's own (empty) dependency closure, so its
        // dependant `e` is out of scope and must not appear.
        let tickets = vec![
            ticket("l", StatusValue::Open, 2, &[]),
            ticket("e", StatusValue::Open, 2, &["l"]),
        ];
        let subtree =
            assemble_ticket_subtree(&tickets, TicketSelection::All, Orientation::Inverted, "l");
        assert_eq!(root_ids(&subtree), vec!["l"]);
        assert!(subtree[0].children.is_empty());
    }

    #[test]
    fn subtree_root_id_inverted_duplicate_root_ids_scope_includes_each_branch() {
        // Both tickets sharing id "a" separately depend on "x" and "y"; the
        // scope for --root a includes both duplicates plus each of their own
        // deps (mirroring subtree_duplicate_root_ids_each_keep_their_own_dependencies),
        // and the inverted rendering nests each duplicate "a" under its own leaf.
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &["x"]),
            ticket("a", StatusValue::Open, 2, &["y"]),
            ticket("x", StatusValue::Open, 2, &[]),
            ticket("y", StatusValue::Open, 2, &[]),
        ];
        let subtree =
            assemble_ticket_subtree(&tickets, TicketSelection::All, Orientation::Inverted, "a");
        assert_eq!(root_ids(&subtree), vec!["x", "y"]);
        assert_eq!(child_ids(&subtree[0]), vec!["a"]);
        assert_eq!(child_ids(&subtree[1]), vec!["a"]);
    }

    #[test]
    fn subtree_root_id_inverted_scoped_cycle_is_not_silently_empty() {
        // Regression: scope {a, b} is a pure cycle (a<->b), so the
        // unrestricted inverted forest over it has no eligible root and
        // would otherwise return empty, silently hiding the whole requested
        // scope. The fallback sweep gives "a" its own root branch and the
        // per-root visited guard prunes the back-edge to "a", so both
        // members are represented exactly once.
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &["b"]),
            ticket("b", StatusValue::Open, 2, &["a"]),
        ];
        let subtree =
            assemble_ticket_subtree(&tickets, TicketSelection::All, Orientation::Inverted, "a");
        assert_eq!(root_ids(&subtree), vec!["a"]);
        assert_eq!(child_ids(&subtree[0]), vec!["b"]);
        assert!(
            subtree[0].children[0].children.is_empty(),
            "back-edge to a must be pruned"
        );
    }

    #[test]
    fn subtree_root_id_inverted_fallback_does_not_duplicate_already_represented_members() {
        // Scope for --root r is {r, m, a, b}: an acyclic branch (leaf "m")
        // alongside a rootless cycle (a<->b) reached only through r->a. The
        // leaf-rooted forest already covers "r" (nested under leaf "m"); the
        // fallback sweep must add exactly one extra root ("a") for the
        // otherwise-unrepresented cycle, without re-adding "m"/"r" as a
        // second root or spawning a redundant root for "b" (already covered
        // by "a"'s own branch).
        let tickets = vec![
            ticket("r", StatusValue::Open, 2, &["m", "a"]),
            ticket("m", StatusValue::Open, 2, &[]),
            ticket("a", StatusValue::Open, 2, &["b"]),
            ticket("b", StatusValue::Open, 2, &["a"]),
        ];
        let subtree =
            assemble_ticket_subtree(&tickets, TicketSelection::All, Orientation::Inverted, "r");
        assert_eq!(root_ids(&subtree), vec!["m", "a"]);
        assert_eq!(child_ids(&subtree[0]), vec!["r"]);
        assert_eq!(child_ids(&subtree[1]), vec!["r", "b"]);
    }

    #[test]
    fn subtree_root_id_inverted_fallback_skips_selection_failing_scope_member() {
        // Decision H, c1 independence pair (T,F) vs (F,F). Scope {a, b} is a
        // pure cycle (a<->b) with a closed duplicate of "a" placed first in
        // slice order: the closed copy enters the scope by id-membership
        // (scoped_tickets keys on id), and since the cycle has no eligible
        // inverted root the forest starts empty. The sweep therefore reaches
        // the closed "a" while its id is not yet represented -- (c1=T fails
        // Open selection, c2=F not represented) -- and must skip it via the
        // `!matches_selection` arm. `build_root_node` never re-checks
        // selection, so without that arm the closed copy would be emitted as
        // the root; the priorities differ so the emitted root's summary pins
        // which copy won: the open "a" (P2), reached later at (F,F).
        let tickets = vec![
            ticket("a", StatusValue::Closed, 5, &["b"]),
            ticket("a", StatusValue::Open, 2, &["b"]),
            ticket("b", StatusValue::Open, 2, &["a"]),
        ];
        let subtree =
            assemble_ticket_subtree(&tickets, TicketSelection::Open, Orientation::Inverted, "a");
        assert_eq!(root_ids(&subtree), vec!["a"]);
        assert_eq!(
            subtree[0].summary, "[P2] a: Title a",
            "the open copy (P2), not the skipped closed copy (P5), is the root"
        );
        assert_eq!(child_ids(&subtree[0]), vec!["b"]);
        assert!(
            subtree[0].children[0].children.is_empty(),
            "back-edge to a must be pruned"
        );
    }

    #[test]
    fn subtree_root_id_inverted_epic_upstream_of_cycle_is_not_a_standalone_root() {
        // Decision H, c3 independence pair (F,F,T) vs (F,F,F). Regression:
        // "r" (the requested root) depends on "a", which cycles with "b"
        // (a<->b), and "r" is placed first in slice order. The initial
        // inverted forest is empty (the cycle has no eligible root). A
        // "not yet represented" check alone would wrongly seed "r" as its
        // own standalone root here (r has no in-scope dependants of its own,
        // so it looks like a leaf), before "a" is even reached -- and "a"'s
        // own walk would then reach "r" again as a dependant, so "r" would
        // appear twice, once wrongly at the top level and not leaf-first.
        // `is_in_sink_scc` excludes "r": it has a resolvable dependency "a"
        // that can never reach it back, so only "a" is seeded, and "r" is
        // nested at the very bottom, appearing exactly once.
        let tickets = vec![
            ticket("r", StatusValue::Open, 2, &["a"]),
            ticket("a", StatusValue::Open, 2, &["b"]),
            ticket("b", StatusValue::Open, 2, &["a"]),
        ];
        let subtree =
            assemble_ticket_subtree(&tickets, TicketSelection::All, Orientation::Inverted, "r");
        assert_eq!(root_ids(&subtree), vec!["a"]);
        assert_eq!(child_ids(&subtree[0]), vec!["r", "b"]);
        assert!(
            subtree[0].children[0].children.is_empty(),
            "r has no in-scope dependants of its own"
        );
        assert!(
            subtree[0].children[1].children.is_empty(),
            "back-edge to a must be pruned"
        );
    }

    #[test]
    fn is_in_sink_scc_leaf_with_empty_forward_set_is_vacuously_true() {
        // Decision I, vacuous all()-fold arm: a ticket with no resolvable
        // in-scope dependency has an empty forward-reachable set, so the
        // back-reachability fold is vacuously true and the ticket counts as a
        // (singleton-leaf) sink. The full sweep never reaches this branch --
        // such a leaf is already an eligible Inverted root, folded into
        // `represented` before c3 is evaluated -- so it is exercised directly
        // here, mirroring inverted_leaf_with_no_deps_is_a_root for Decision D.
        let scope = [ticket("l", StatusValue::Open, 2, &[])];
        let scope_lookup: HashMap<&str, &Ticket> = scope.iter().map(|t| (t.id(), t)).collect();
        assert!(is_in_sink_scc(&scope[0], &scope_lookup));
    }

    #[test]
    fn is_in_source_scc_leaf_with_empty_forward_set_is_vacuously_true() {
        // Decision K, vacuous all()-fold arm, mirroring
        // is_in_sink_scc_leaf_with_empty_forward_set_is_vacuously_true: a
        // ticket nobody depends on has an empty forward-dependants-reachable
        // set, so the fold is vacuously true and the ticket counts as a
        // (singleton-leaf) source.
        let scope = [ticket("r", StatusValue::Open, 2, &[])];
        let lookup: HashMap<&str, &Ticket> = scope.iter().map(|t| (t.id(), t)).collect();
        let adjacency = build_inverted_adjacency(&scope);
        assert!(is_in_source_scc(&scope[0], &adjacency, &lookup));
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
    fn graph_root_id_inverted_scope_is_normal_closure_leaf_first() {
        // `--root e` fixes the SCOPE to `e`'s normal-orientation closure
        // ({l, m, e}); `--inverted` re-runs the unrestricted Inverted
        // assembly over that scope alone, so leaf `l` is the root and edges
        // read dependency --> dependant up to `e`.
        let tickets = vec![
            ticket("l", StatusValue::Open, 2, &[]),
            ticket("m", StatusValue::Open, 2, &["l"]),
            ticket("e", StatusValue::Open, 2, &["m"]),
        ];
        let graph = assemble_ticket_graph(
            &tickets,
            TicketSelection::All,
            Orientation::Inverted,
            Some("e"),
        );
        assert_eq!(graph_node_ids(&graph), vec!["l", "m", "e"]);
        assert_eq!(graph_edge_pairs(&graph), vec![("l", "m"), ("m", "e")]);
    }

    #[test]
    fn graph_root_id_inverted_diamond_within_scope_keeps_both_edges() {
        // `a` depends on both `b` and `c`, which both depend on `d`. Rooting
        // at `a` --inverted scopes to {a, b, c, d} and re-runs the
        // unrestricted inverted graph over it, which keeps both diamond
        // edges into `a` (mirrors graph_diamond_preserves_both_edges_into_shared_dependency,
        // now for the scoped-inverted path).
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &["b", "c"]),
            ticket("b", StatusValue::Open, 2, &["d"]),
            ticket("c", StatusValue::Open, 2, &["d"]),
            ticket("d", StatusValue::Open, 2, &[]),
        ];
        let graph = assemble_ticket_graph(
            &tickets,
            TicketSelection::All,
            Orientation::Inverted,
            Some("a"),
        );
        assert_eq!(graph_node_ids(&graph), vec!["d", "b", "a", "c"]);
        assert_eq!(
            graph_edge_pairs(&graph),
            vec![("d", "b"), ("b", "a"), ("d", "c"), ("c", "a")]
        );
    }

    #[test]
    fn graph_root_id_inverted_excludes_out_of_scope_dependant() {
        // `z` depends on `m` but is not part of `m`'s own dependency closure;
        // scoping by --root m keeps `z` out even though --inverted would,
        // unscoped, walk dependants.
        let tickets = vec![
            ticket("l", StatusValue::Open, 2, &[]),
            ticket("m", StatusValue::Open, 2, &["l"]),
            ticket("z", StatusValue::Open, 2, &["m"]),
        ];
        let graph = assemble_ticket_graph(
            &tickets,
            TicketSelection::All,
            Orientation::Inverted,
            Some("m"),
        );
        assert_eq!(graph_node_ids(&graph), vec!["l", "m"]);
        assert_eq!(graph_edge_pairs(&graph), vec![("l", "m")]);
    }

    #[test]
    fn graph_root_id_inverted_on_a_leaf_yields_just_the_leaf_node() {
        // Mirror of the tree path: the scope is the leaf's own (empty)
        // dependency closure, so its dependant `e` is excluded.
        let tickets = vec![
            ticket("l", StatusValue::Open, 2, &[]),
            ticket("e", StatusValue::Open, 2, &["l"]),
        ];
        let graph = assemble_ticket_graph(
            &tickets,
            TicketSelection::All,
            Orientation::Inverted,
            Some("l"),
        );
        assert_eq!(graph_node_ids(&graph), vec!["l"]);
        assert!(graph.edges.is_empty());
    }

    #[test]
    fn graph_root_id_inverted_scoped_cycle_is_not_silently_empty() {
        // root_id: Some + Inverted recurses into assemble_ticket_graph with
        // root_id: None over the scope, so the existing fallback-sweep
        // coverage there (see
        // graph_pure_cycle_with_no_root_is_still_represented_inverted)
        // already handles a scope whose bottom is a pure cycle -- this locks
        // that the recursive entry point actually reaches it.
        let tickets = vec![
            ticket("a", StatusValue::Open, 2, &["b"]),
            ticket("b", StatusValue::Open, 2, &["a"]),
        ];
        let graph = assemble_ticket_graph(
            &tickets,
            TicketSelection::All,
            Orientation::Inverted,
            Some("a"),
        );
        assert_eq!(graph_node_ids(&graph), vec!["a", "b"]);
        assert_eq!(graph_edge_pairs(&graph), vec![("a", "b"), ("b", "a")]);
    }

    #[test]
    fn graph_root_id_inverted_upstream_of_cycle_deduplicates_the_upstream_node() {
        // "r" depends into cycle a<->b. Unlike the tree path, graph's
        // fallback sweep (in the recursive root_id: None call) has no
        // is_in_sink_scc restriction and could seed "r" as a node before "a"
        // is processed -- but nodes are deduped by a single global
        // visited_nodes set regardless of walk order, so "r" still appears
        // exactly once, and every edge (including a --> r) is still
        // recorded when "a"'s own walk reaches it.
        let tickets = vec![
            ticket("r", StatusValue::Open, 2, &["a"]),
            ticket("a", StatusValue::Open, 2, &["b"]),
            ticket("b", StatusValue::Open, 2, &["a"]),
        ];
        let graph = assemble_ticket_graph(
            &tickets,
            TicketSelection::All,
            Orientation::Inverted,
            Some("r"),
        );
        assert_eq!(graph_node_ids(&graph), vec!["r", "a", "b"]);
        assert_eq!(
            graph_edge_pairs(&graph),
            vec![("a", "r"), ("a", "b"), ("b", "a")]
        );
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
