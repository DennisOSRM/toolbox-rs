//! An edge-based graph worked out as it is walked, rather than held.
//!
//! # What it is
//!
//! A node-based graph says where a road goes. It cannot say what happens at
//! the junction: a route that arrives at a crossing and leaves the way it came
//! costs, in such a graph, nothing at all. Turning that into something a search
//! can price means making each arc a node, and each turn between two arcs an
//! arc. That is the edge-based graph.
//!
//! # Why it is not built
//!
//! A continent is eighteen million nodes and forty-two million arcs, and its
//! turns number the sum over each node of its in-degree times its out-degree:
//! about ninety-eight million. Written down that is a graph of forty-two
//! million nodes and ninety-eight million arcs, more than twice the arcs of the
//! one it came from, and better part of a gigabyte before a weight is stored.
//!
//! None of it has to be written down. [`StaticGraph`](crate::static_graph)
//! already numbers its arcs densely from zero, so an arc *is* an edge-based
//! node id with no table in between, and the turns leaving it are exactly the
//! arcs leaving its head. A turn from `e` to `f` is named by `f` alone, since
//! `f` says both which turn it is and where it lands. So the whole expansion is
//! two questions the underlying graph already answers, and this module asks
//! them one node at a time through [`Adjacency`].
//!
//! # What it costs to hold
//!
//! Nothing beyond the graph and the coordinates that were there already. An
//! arc's head is written down and its tail is not, and a turn needs both, so
//! the tail would have to be searched for among the offsets. It is not: an arc
//! is reached by turning onto it at the head of the arc before it, so whatever
//! a search was standing on last says the tail outright, and [`Adjacency`]
//! hands that over. Over a continent that leaves two searches of the offsets a
//! query, at the source, rather than one per node settled.

use std::cell::Cell;

use crate::{
    geometry::FPCoordinate,
    graph::{Adjacency, Arcs, EdgeID, NodeID},
    heap_stats::HeapStats,
    unidirectional_dijkstra::UnidirectionalSearch,
};

/// The two segments that meet at a turn, in a frame where both are flat.
///
/// The units are micro-degrees of latitude, with longitude scaled to match at
/// the latitude the turn is at. Nothing here is a distance, and none of it is
/// meant to be: only the angle between the two is asked about.
pub struct Turn {
    /// the segment arrived along, pointing the way the route was going
    pub arriving: (f64, f64),
    /// the segment about to be taken, pointing the way it would go
    pub leaving: (f64, f64),
    /// whether the turn goes back down the segment it arrived on
    pub reversal: bool,
}

impl Turn {
    /// Positive turning left of the way on, negative turning right.
    #[must_use]
    pub fn cross(&self) -> f64 {
        self.arriving.0 * self.leaving.1 - self.arriving.1 * self.leaving.0
    }

    /// Positive carrying on the way it was going, negative doubling back.
    #[must_use]
    pub fn dot(&self) -> f64 {
        self.arriving.0 * self.leaving.0 + self.arriving.1 * self.leaving.1
    }

    /// How far off carrying straight on the turn is, in degrees: zero straight
    /// on, ninety a right angle, a hundred and eighty back the way it came.
    /// Left of the way on is positive.
    ///
    /// Nothing a search runs over calls this, and nothing should: a cost that
    /// wants to know how sharp a turn is can be written against
    /// [`Self::cross`] and [`Self::dot`] and not pay for an `atan2` a turn.
    /// [`AnglePenalty`] is the worked example.
    #[must_use]
    pub fn degrees(&self) -> f64 {
        self.cross().atan2(self.dot()).to_degrees()
    }
}

/// What a turn costs, in whatever unit the graph's own arcs are weighted in.
pub trait TurnCost {
    /// What the turn costs, or `None` when it may not be taken at all.
    fn cost(&self, turn: &Turn) -> Option<u32>;
}

/// Every turn is allowed and none of them costs anything.
///
/// The edge-based graph under this answers every distance the node-based graph
/// does, which is what makes it the thing to check an expansion against.
pub struct FreeTurns;

impl TurnCost for FreeTurns {
    fn cost(&self, _turn: &Turn) -> Option<u32> {
        Some(0)
    }
}

/// Turning back down the arc just travelled is refused. Everything else is
/// free.
///
/// This asks nothing of the coordinates and has no number in it to get wrong,
/// which is the whole of its appeal: a reversal is a reversal because of what
/// the graph says, not because of where anything lies.
pub struct NoUTurns;

impl TurnCost for NoUTurns {
    fn cost(&self, turn: &Turn) -> Option<u32> {
        if turn.reversal { None } else { Some(0) }
    }
}

/// How finely the cost is written down against the cosine of the turn angle.
///
/// A thousand steps put about a fifth of a degree between two entries where the
/// cost is still rising, and less than that towards a right angle. Where the
/// cosine says least about the angle, near straight on, the cost is flat
/// anyway.
const COSINE_STEPS: usize = 1024;

/// A reversal is refused, and everything else costs by how far off straight on
/// it is.
///
/// Within `free_within` degrees of carrying on there is nothing to pay. From
/// there the cost rises evenly with the angle, reaching `sharpest` at a right
/// angle and staying there for anything sharper. Both numbers are in the unit
/// the graph's arcs are weighted in, which this module has no way to know.
///
/// # How the angle is not worked out
///
/// A turn is priced by the cosine of its angle rather than by the angle. The
/// cosine of the angle between two segments is their dot product over the
/// product of their lengths, which is one square root, and the cost against it
/// is written down once when this is built. Asking for the angle itself costs
/// an `atan2` a turn instead, and a search over a continent looks at ninety
/// eight million of them: measured, that was thirteen seconds of a twenty nine
/// second run.
pub struct AnglePenalty {
    /// what a turn costs, by the cosine of how far off straight it is, from
    /// turning back on itself at the start to carrying straight on at the end
    by_cosine: Vec<u32>,
}

impl AnglePenalty {
    #[must_use]
    pub fn new(free_within: f64, sharpest: u32) -> Self {
        let by_cosine = (0..COSINE_STEPS)
            .map(|at| {
                let cosine = 2.0 * at as f64 / (COSINE_STEPS - 1) as f64 - 1.0;
                let off = cosine.clamp(-1.0, 1.0).acos().to_degrees();
                if off <= free_within {
                    return 0;
                }
                let span = 90.0 - free_within;
                if span <= 0.0 {
                    return sharpest;
                }
                let share = ((off - free_within) / span).min(1.0);
                (share * f64::from(sharpest)) as u32
            })
            .collect();
        Self { by_cosine }
    }

    /// What carrying straight on costs, which is what a turn with no angle to
    /// speak of is charged.
    fn straight_on(&self) -> u32 {
        self.by_cosine[COSINE_STEPS - 1]
    }
}

impl TurnCost for AnglePenalty {
    fn cost(&self, turn: &Turn) -> Option<u32> {
        if turn.reversal {
            return None;
        }
        let lengths = (turn.arriving.0 * turn.arriving.0 + turn.arriving.1 * turn.arriving.1)
            * (turn.leaving.0 * turn.leaving.0 + turn.leaving.1 * turn.leaving.1);
        if lengths <= 0.0 {
            // two nodes in the same place leave a segment with no direction,
            // and nothing that has no direction has been turned away from
            return Some(self.straight_on());
        }
        let cosine = (turn.dot() / lengths.sqrt()).clamp(-1.0, 1.0);
        let at = ((cosine + 1.0) * 0.5 * (COSINE_STEPS - 1) as f64) as usize;
        Some(self.by_cosine[at])
    }
}

/// A graph whose nodes are the arcs of another, expanded as it is asked.
///
/// A node of this is an arc of `graph`, by the same id. It holds a reference to
/// the graph and to the coordinates, a turn cost, and one node id worth of
/// state to start the next tail search from.
pub struct EdgeBasedGraph<'a, G, T> {
    graph: &'a G,
    coordinates: &'a [FPCoordinate],
    turns: T,
    /// where the last tail was found, so the next search starts near it
    hint: Cell<NodeID>,
    /// how many offsets the tail searches have read, and how many were asked
    steps: Cell<usize>,
    lookups: Cell<usize>,
}

impl<'a, G: Arcs<u32>, T: TurnCost> EdgeBasedGraph<'a, G, T> {
    /// # Panics
    ///
    /// If the coordinates do not cover the graph's nodes.
    #[must_use]
    pub fn new(graph: &'a G, coordinates: &'a [FPCoordinate], turns: T) -> Self {
        assert!(
            coordinates.len() >= graph.number_of_nodes(),
            "the coordinates do not cover the graph"
        );
        Self {
            graph,
            coordinates,
            turns,
            hint: Cell::new(0),
            steps: Cell::new(0),
            lookups: Cell::new(0),
        }
    }

    /// How many offsets the tail searches have read, over how many tails were
    /// asked for. Their ratio is what holding no array of tails costs.
    #[must_use]
    pub fn tail_steps(&self) -> (usize, usize) {
        (self.steps.get(), self.lookups.get())
    }

    /// The node an arc runs into, which is where its turns are.
    #[must_use]
    pub fn head(&self, arc: EdgeID) -> NodeID {
        self.graph.target(arc)
    }

    /// Whether a node's block of arcs holds this one.
    fn owns(&self, node: NodeID, arc: EdgeID) -> bool {
        let block = self.offsets(node);
        block.start <= arc && arc < block.end
    }

    /// A node's block, counted so that the cost of finding a tail is known.
    fn offsets(&self, node: NodeID) -> core::ops::Range<EdgeID> {
        self.steps.set(self.steps.get() + 1);
        self.graph.edge_range(node)
    }

    /// The node an arc runs out of.
    ///
    /// An adjacency array writes down where each node's arcs begin, so a head
    /// is there to be read and a tail is not. Rather than keep a second array
    /// of tails, which on a continent is a hundred and seventy megabytes on top
    /// of a graph of five hundred, it is searched for among the offsets, which
    /// are sorted because the blocks are laid out in order.
    ///
    /// The node the last lookup landed on is tried first, since a walk of one
    /// node's turns asks about arcs that are all its own. Past that it is a
    /// plain binary search of the whole array.
    ///
    /// Widening a bracket around the last answer was tried instead and was
    /// worse: 42.6 offsets read per lookup against 24 for the search below.
    /// A Dijkstra settles in order of distance and not in order of arc, so one
    /// lookup says nothing about where the next will land, and the walk outward
    /// is paid for before a binary search of a wider bracket than this one.
    #[must_use]
    pub fn tail(&self, arc: EdgeID) -> NodeID {
        self.lookups.set(self.lookups.get() + 1);
        let last = self.graph.number_of_nodes() - 1;
        let hint = self.hint.get().min(last);
        if self.owns(hint, arc) {
            return hint;
        }

        // the answer is the last node whose block starts at or before the arc
        let (mut low, mut high) = (0, last);
        while low < high {
            let middle = low + (high - low).div_ceil(2);
            if self.offsets(middle).start <= arc {
                low = middle;
            } else {
                high = middle - 1;
            }
        }
        self.hint.set(low);
        low
    }

    /// Every arc leaving a node, as the edge-based node it is and what it cost
    /// to have taken it.
    ///
    /// This is where a search that names its ends by node rather than by arc
    /// starts from.
    pub fn for_each_departure(&self, node: NodeID, mut f: impl FnMut(EdgeID, usize)) {
        for arc in self.graph.edge_range(node) {
            f(arc, self.graph.weight(arc) as usize);
        }
    }

    /// The turns leaving an arc, as the arc each lands on and what it costs to
    /// get there, the arc's own weight included.
    ///
    /// A turn that the cost refuses is not offered at all.
    pub fn for_each_turn(&self, arc: EdgeID, f: impl FnMut(EdgeID, u32)) {
        self.turns_from(arc, self.tail(arc), f);
    }

    /// The same, told which node the arc runs out of rather than looking.
    fn turns_from(&self, arc: EdgeID, from: NodeID, mut f: impl FnMut(EdgeID, u32)) {
        let via = self.head(arc);
        // one unit of longitude is this much of a unit of latitude, here
        let squeeze = f64::from(self.coordinates[via].lat)
            .mul_add(1e-6_f64.to_radians(), 0.0)
            .cos();
        let arriving = self.vector(from, via, squeeze);

        // Asked arc at a time rather than through `for_each_arc`, which hands
        // over where an arc goes and not which arc it is. Which arc it is, is
        // the whole of the answer here: it is the node being turned onto.
        for onto in self.graph.edge_range(via) {
            let to = self.graph.target(onto);
            let turn = Turn {
                arriving,
                leaving: self.vector(via, to, squeeze),
                reversal: to == from,
            };
            if let Some(cost) = self.turns.cost(&turn) {
                f(onto, self.graph.weight(onto) + cost);
            }
        }
    }

    /// The step from one node to another, with longitude squeezed so that the
    /// two axes are the same size on the ground.
    fn vector(&self, from: NodeID, to: NodeID, squeeze: f64) -> (f64, f64) {
        let (from, to) = (&self.coordinates[from], &self.coordinates[to]);
        (
            f64::from(to.lon - from.lon) * squeeze,
            f64::from(to.lat - from.lat),
        )
    }
}

impl<G: Arcs<u32>, T: TurnCost> Adjacency<u32> for EdgeBasedGraph<'_, G, T> {
    /// A node here is an arc there.
    fn number_of_nodes(&self) -> usize {
        self.graph.number_of_edges()
    }

    /// The node `n` runs out of is the node `from` runs into, so being told
    /// what this arc was reached from is being told its tail. Where the search
    /// began there is nothing to be told, and it is looked for instead.
    fn for_each_arc(&self, n: NodeID, from: NodeID, f: impl FnMut(NodeID, u32)) {
        let tail = if from == n {
            self.tail(n)
        } else {
            self.graph.target(from)
        };
        self.turns_from(n, tail, f);
    }
}

/// A search between two nodes of the underlying graph.
///
/// An edge-based node is an arc, so standing at a node means standing on any
/// of the arcs leaving it, and arriving at one means arriving on any of the
/// arcs running into it. Each source starts at what its arc costs, so the
/// answer is the cost of the whole route, first arc included.
///
/// [`UnidirectionalSearch::run`] takes the arc form instead: it starts at the
/// head of the arc it is given, having already paid for it, and stops on the
/// arc it is asked for.
pub fn between_nodes<S: HeapStats<NodeID>, G: Arcs<u32>, T: TurnCost>(
    search: &mut UnidirectionalSearch<S>,
    graph: &EdgeBasedGraph<G, T>,
    source: NodeID,
    target: NodeID,
) -> usize {
    let mut departures = Vec::new();
    graph.for_each_departure(source, |arc, cost| departures.push((arc, cost)));
    search.run_many(graph, &departures, |arc| graph.head(arc) == target)
}

/// The nodes a route of arcs runs through.
///
/// A search here leaves a path of arcs. The nodes are where the first one
/// starts and where each of them ends, so a route of `n` arcs is `n + 1`
/// nodes. An empty route is no nodes at all, since standing still is not
/// somewhere.
pub fn node_path<G: Arcs<u32>, T: TurnCost>(
    graph: &EdgeBasedGraph<G, T>,
    arcs: &[EdgeID],
) -> Vec<NodeID> {
    let Some(&first) = arcs.first() else {
        return Vec::new();
    };
    let mut path = Vec::with_capacity(arcs.len() + 1);
    path.push(graph.tail(first));
    path.extend(arcs.iter().map(|&arc| graph.head(arc)));
    path
}

#[cfg(test)]
mod tests {
    use super::{AnglePenalty, EdgeBasedGraph, FreeTurns, NoUTurns, between_nodes, node_path};
    use crate::{
        edge::InputEdge,
        geometry::FPCoordinate,
        graph::{Arcs, NodeID},
        static_graph::StaticGraph,
        unidirectional_dijkstra::UnidirectionalDijkstra,
    };

    /// A plus sign: a middle node with four spokes, every arc both ways.
    ///
    ///        3
    ///        |
    ///   1 -- 0 -- 2
    ///        |
    ///        4
    fn plus() -> (StaticGraph<u32>, Vec<FPCoordinate>) {
        let mut edges = Vec::new();
        for spoke in 1..5 {
            edges.push(InputEdge::new(0, spoke, 10));
            edges.push(InputEdge::new(spoke, 0, 10));
        }
        let coordinates = vec![
            FPCoordinate::new(0, 0),
            FPCoordinate::new(0, -1000),
            FPCoordinate::new(0, 1000),
            FPCoordinate::new(1000, 0),
            FPCoordinate::new(-1000, 0),
        ];
        (StaticGraph::new(edges), coordinates)
    }

    #[test]
    fn every_arc_knows_the_node_it_runs_out_of() {
        let (graph, coordinates) = plus();
        let expanded = EdgeBasedGraph::new(&graph, &coordinates, FreeTurns);

        for node in graph.node_range() {
            for arc in graph.edge_range(node) {
                assert_eq!(expanded.tail(arc), node, "arc {arc} of node {node}");
            }
        }
    }

    /// The hint is carried from one lookup to the next, so the order they are
    /// asked in must not change any answer.
    #[test]
    fn a_tail_is_the_same_whatever_was_asked_before() {
        let (graph, coordinates) = plus();
        let expanded = EdgeBasedGraph::new(&graph, &coordinates, FreeTurns);

        let forwards: Vec<NodeID> = (0..graph.number_of_edges())
            .map(|arc| expanded.tail(arc))
            .collect();
        let backwards: Vec<NodeID> = (0..graph.number_of_edges())
            .rev()
            .map(|arc| expanded.tail(arc))
            .collect();

        assert_eq!(forwards, backwards.into_iter().rev().collect::<Vec<_>>());
    }

    /// Free turns cost nothing and forbid nothing, so the expansion must
    /// answer every distance the graph it came from answers.
    #[test]
    fn free_turns_answer_what_the_node_based_graph_answers() {
        let (graph, coordinates) = plus();
        let expanded = EdgeBasedGraph::new(&graph, &coordinates, FreeTurns);
        let mut plain = UnidirectionalDijkstra::new();
        let mut turning = UnidirectionalDijkstra::new();

        for source in graph.node_range() {
            for target in graph.node_range() {
                if source == target {
                    continue;
                }
                let expected = plain.run(&graph, source, target);
                let found = between_nodes(&mut turning, &expanded, source, target);
                assert_eq!(found, expected, "from {source} to {target}");
            }
        }
    }

    #[test]
    fn a_route_of_arcs_reads_back_as_the_nodes_it_runs_through() {
        let (graph, coordinates) = plus();
        let expanded = EdgeBasedGraph::new(&graph, &coordinates, FreeTurns);
        let mut search = UnidirectionalDijkstra::new();

        assert_eq!(between_nodes(&mut search, &expanded, 1, 2), 20);
        let arcs = search
            .retrieve_node_path(search.met())
            .expect("the target was reached");
        assert_eq!(node_path(&expanded, &arcs), vec![1, 0, 2]);
    }

    #[test]
    fn a_route_of_no_arcs_runs_through_nowhere() {
        let (graph, coordinates) = plus();
        let expanded = EdgeBasedGraph::new(&graph, &coordinates, FreeTurns);
        assert!(node_path(&expanded, &[]).is_empty());
    }

    /// A spoke can only be left by going back through the middle, so refusing
    /// reversals leaves no way out of one at all.
    #[test]
    fn refusing_reversals_shuts_a_dead_end_in() {
        let (graph, coordinates) = plus();
        let expanded = EdgeBasedGraph::new(&graph, &coordinates, NoUTurns);
        let mut search = UnidirectionalDijkstra::new();

        assert_eq!(between_nodes(&mut search, &expanded, 1, 2), 20);
        // 1 to 0 and back to 1 is the only route, and it is a reversal
        assert_eq!(
            between_nodes(&mut search, &expanded, 1, 1),
            usize::MAX,
            "a reversal was taken"
        );
    }

    /// Going straight across the plus is free; turning a corner is not.
    #[test]
    fn a_corner_costs_what_carrying_straight_on_does_not() {
        let (graph, coordinates) = plus();
        let expanded = EdgeBasedGraph::new(&graph, &coordinates, AnglePenalty::new(10., 100));
        let mut search = UnidirectionalDijkstra::new();

        // west to east is a straight line through the middle
        assert_eq!(between_nodes(&mut search, &expanded, 1, 2), 20);
        // west to north is a right angle, and pays the whole penalty
        assert_eq!(between_nodes(&mut search, &expanded, 1, 3), 120);
    }
}
