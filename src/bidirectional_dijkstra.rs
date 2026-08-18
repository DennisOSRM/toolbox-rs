//! A search that runs from both ends and meets in the middle.
//!
//! # Why it is the honest yardstick
//!
//! A plain search from the source sweeps a disc of the network until the
//! target falls inside it. Two searches, one from each end, sweep two discs of
//! half the radius, and two half-radius discs hold rather less than one full
//! one. On a road network that is worth a constant factor, and it is a factor
//! that costs nothing but bookkeeping — no preprocessing, no overlay, no
//! customization to keep up to date. Anything that does keep a preprocessed
//! structure should be measured against this rather than against the plain
//! search, or it is being credited with a speedup that a few lines of code
//! would have given for free.
//!
//! # The two graphs
//!
//! The backward search walks arcs into a node rather than out of it, so it
//! needs the graph with every arc turned around. On an undirected network the
//! two are the same object and it can simply be handed over twice.
use crate::{
    addressable_binary_heap::AddressableHeapWithStats,
    graph::{Graph, INVALID_NODE_ID, NodeID},
    heap_stats::{Counters, HeapStats, Untracked},
};

/// A search from both ends, counting nothing.
///
/// This is what a run whose time is being taken wants: no counters, nothing
/// carried that a measurement would be measuring instead of the search.
pub type BidirectionalDijkstra = BidirectionalSearch<Untracked>;

/// The same search, counting what its two queues did.
pub type TrackedBidirectionalDijkstra = BidirectionalSearch<Counters>;

pub struct BidirectionalSearch<S: HeapStats<NodeID>> {
    forward: AddressableHeapWithStats<NodeID, usize, NodeID, S>,
    backward: AddressableHeapWithStats<NodeID, usize, NodeID, S>,
    upper_bound: usize,
    meeting_node: NodeID,
}

impl<S: HeapStats<NodeID>> Default for BidirectionalSearch<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: HeapStats<NodeID>> BidirectionalSearch<S> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            forward: AddressableHeapWithStats::new(),
            backward: AddressableHeapWithStats::new(),
            upper_bound: usize::MAX,
            meeting_node: INVALID_NODE_ID,
        }
    }

    /// What each side's queue did on the last run. The two are kept apart
    /// because how the work split between them is the thing worth knowing
    /// about a search that runs from both ends.
    pub fn stats(&self) -> (&S, &S) {
        (self.forward.stats(), self.backward.stats())
    }

    /// clears the search space stored in both queues.
    pub fn clear(&mut self) {
        self.forward.clear();
        self.backward.clear();
        self.upper_bound = usize::MAX;
        self.meeting_node = INVALID_NODE_ID;
    }

    /// The node the two sides met at, or `INVALID_NODE_ID` if they never did.
    #[must_use]
    pub fn meeting_node(&self) -> NodeID {
        self.meeting_node
    }

    /// how many nodes were explored (not settled), counting both sides.
    pub fn search_space_len(&self) -> usize {
        self.forward.inserted_len() + self.backward.inserted_len()
    }

    /// Settles the next node of one side and relaxes what leaves it.
    ///
    /// The node it settles is done for this side, so whatever the other side
    /// holds for that node is a path from one end to the other through it, and
    /// the shortest such path seen so far is what the search is bounded by.
    fn advance<G: Graph<usize>>(
        queue: &mut AddressableHeapWithStats<NodeID, usize, NodeID, S>,
        other: &AddressableHeapWithStats<NodeID, usize, NodeID, S>,
        graph: &G,
        bound: &mut usize,
        meeting: &mut NodeID,
    ) {
        let u = queue.delete_min();
        let distance = queue.weight(u);

        // one look, not two: a node the other side has never seen is held at
        // no distance at all, which is the same answer as asking first whether
        // it has seen it
        let from_there = other.weight(u);
        if from_there != usize::MAX {
            let through = distance + from_there;
            if through < *bound {
                *bound = through;
                *meeting = u;
            }
        }

        for edge in graph.edge_range(u) {
            let v = graph.target(edge);
            queue.insert_or_decrease(v, distance + *graph.data(edge), u);
        }
    }

    /// Runs a search from `s` and a search from `t` until they meet, and hands
    /// back what the way between them costs, or `usize::MAX` if there is none.
    ///
    /// `reverse` is `graph` with every arc turned around; on an undirected
    /// network the same graph is handed over twice.
    ///
    /// The object is reusable and clears itself on every run.
    pub fn run<G: Graph<usize>>(&mut self, graph: &G, reverse: &G, s: NodeID, t: NodeID) -> usize {
        self.clear();

        self.forward.insert(s, 0, s);
        self.backward.insert(t, 0, t);

        while !self.forward.is_empty() && !self.backward.is_empty() {
            let front = self.forward.min_weight();
            let back = self.backward.min_weight();

            // neither side can reach past its own front, so once the two
            // fronts together are no shorter than the best way already found,
            // there is nothing left that could beat it
            if front + back >= self.upper_bound {
                break;
            }

            // work on whichever side has come the shorter way, which keeps the
            // two fronts growing together rather than one running ahead
            if front <= back {
                Self::advance(
                    &mut self.forward,
                    &self.backward,
                    graph,
                    &mut self.upper_bound,
                    &mut self.meeting_node,
                );
            } else {
                Self::advance(
                    &mut self.backward,
                    &self.forward,
                    reverse,
                    &mut self.upper_bound,
                    &mut self.meeting_node,
                );
            }
        }

        self.upper_bound
    }

    /// The nodes of the way that was found, from source to target.
    ///
    /// It is walked out of both queues: back from the meeting node to the
    /// source through the forward parents, and on from it to the target
    /// through the backward ones.
    pub fn retrieve_node_path(&self) -> Option<Vec<NodeID>> {
        if self.upper_bound == usize::MAX {
            return None;
        }

        let mut path = vec![self.meeting_node];
        let mut node = self.meeting_node;
        loop {
            let parent = *self.forward.data(node);
            if parent == node {
                break;
            }
            path.push(parent);
            node = parent;
        }
        path.reverse();

        let mut node = self.meeting_node;
        loop {
            let parent = *self.backward.data(node);
            if parent == node {
                break;
            }
            path.push(parent);
            node = parent;
        }

        Some(path)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        bidirectional_dijkstra::{
            BidirectionalDijkstra, BidirectionalSearch, TrackedBidirectionalDijkstra,
        },
        edge::InputEdge,
        graph::Graph,
        grid_graph::{grid_edges, node_at},
        heap_stats::Untracked,
        static_graph::StaticGraph,
        unidirectional_dijkstra::UnidirectionalDijkstra,
    };

    /// The same arcs, turned around, which is what the backward side walks.
    fn reversed(edges: &[InputEdge<usize>]) -> StaticGraph<usize> {
        StaticGraph::new(
            edges
                .iter()
                .map(|e| InputEdge::new(e.target, e.source, e.data))
                .collect(),
        )
    }

    fn create_graph() -> Vec<InputEdge<usize>> {
        vec![
            InputEdge::new(0, 1, 3),
            InputEdge::new(1, 2, 3),
            InputEdge::new(4, 2, 1),
            InputEdge::new(2, 3, 6),
            InputEdge::new(0, 4, 2),
            InputEdge::new(4, 5, 2),
            InputEdge::new(5, 3, 7),
            InputEdge::new(1, 5, 2),
        ]
    }

    /// The table the plain search is held against, so the two searches are
    /// held against the same numbers rather than against each other alone.
    #[test]
    fn apsp() {
        let edges = create_graph();
        let graph = StaticGraph::new(edges.clone());
        let reverse = reversed(&edges);

        let no = usize::MAX;
        let results_table = [
            [0, 3, 3, 9, 2, 4],
            [no, 0, 3, 9, no, 2],
            [no, no, 0, 6, no, no],
            [no, no, no, 0, no, no],
            [no, no, 1, 7, 0, 2],
            [no, no, no, 7, no, 0],
        ];

        let mut search = BidirectionalDijkstra::new();
        for (i, table) in results_table.iter().enumerate() {
            for (j, expected) in table.iter().enumerate() {
                assert_eq!(*expected, search.run(&graph, &reverse, i, j), "{i} to {j}");
            }
        }
    }

    /// What the two sides agree on has to be what one side on its own would
    /// have said, on graphs nobody worked out by hand. This is the check that
    /// the stopping rule does not stop early.
    #[test]
    fn both_ends_agree_with_one_end() {
        use rand::{RngExt, SeedableRng, prelude::StdRng};

        let mut rng = StdRng::seed_from_u64(0x_B1_D1);
        for round in 0..20 {
            let count = 8 + round;
            let mut edges = Vec::new();
            for source in 0..count {
                for target in 0..count {
                    if source != target && rng.random_range(0..3) == 0 {
                        edges.push(InputEdge::new(
                            source,
                            target,
                            rng.random_range(1..20_usize),
                        ));
                    }
                }
            }
            if edges.is_empty() {
                continue;
            }
            let graph = StaticGraph::new(edges.clone());
            let reverse = reversed(&edges);
            let mut both = BidirectionalDijkstra::new();
            let mut one = UnidirectionalDijkstra::new();

            for source in 0..count {
                for target in 0..count {
                    assert_eq!(
                        one.run(&graph, source, target),
                        both.run(&graph, &reverse, source, target),
                        "round {round}: {source} to {target}",
                    );
                }
            }
        }
    }

    /// The way that comes back has to be a way of the graph, and it has to
    /// cost what was reported. The join at the meeting node is what this
    /// catches: a path that is walked out of two queues can be handed back
    /// with the middle node in it twice, or with a step that is not an arc.
    #[test]
    fn a_retrieved_path_walks_the_graph_and_costs_what_was_reported() {
        use rand::{RngExt, SeedableRng, prelude::StdRng};

        let mut rng = StdRng::seed_from_u64(0x_B1_D2);
        for round in 0..20 {
            let count = 8 + round;
            let mut edges = Vec::new();
            for source in 0..count {
                for target in 0..count {
                    if source != target && rng.random_range(0..3) == 0 {
                        edges.push(InputEdge::new(
                            source,
                            target,
                            rng.random_range(1..20_usize),
                        ));
                    }
                }
            }
            if edges.is_empty() {
                continue;
            }
            let graph = StaticGraph::new(edges.clone());
            let reverse = reversed(&edges);
            let mut search = BidirectionalDijkstra::new();

            for source in 0..count {
                for target in 0..count {
                    let distance = search.run(&graph, &reverse, source, target);
                    if distance == usize::MAX {
                        continue;
                    }
                    let path = search
                        .retrieve_node_path()
                        .expect("a way was found but not handed back");
                    assert_eq!(path.first(), Some(&source), "round {round}: {path:?}");
                    assert_eq!(path.last(), Some(&target), "round {round}: {path:?}");

                    let mut walked = 0;
                    for step in path.windows(2) {
                        let edge = graph
                            .find_edge(step[0], step[1])
                            .expect("the way takes an arc the graph does not have");
                        walked += *graph.data(edge);
                    }
                    assert_eq!(walked, distance, "round {round}: {path:?}");
                }
            }
        }
    }

    /// Two searches from two ends look at less than one search from one end.
    ///
    /// This is the whole reason to have it, so it is worth asserting rather
    /// than assuming. A grid is the shape that shows it: a search from one end
    /// sweeps a disc until the target falls into it, and two searches sweep
    /// two discs of half the radius, which together cover about half the
    /// ground. On a line there is nothing to halve and the two ends lose;
    /// corner to corner of a grid there is nothing to halve either, as the
    /// disc has to swallow the whole of it either way.
    #[test]
    fn two_ends_look_at_less_than_one_end() {
        // the two ends sit well inside the grid, so neither disc runs into an
        // edge of it and gets clipped into looking smaller than it is
        let side = 256;
        let edges = grid_edges(side, true);
        let graph = StaticGraph::new(edges);
        let source = node_at(side, 128, 96);
        let target = node_at(side, 128, 160);

        let mut one = UnidirectionalDijkstra::new();
        let mut both = BidirectionalDijkstra::new();
        assert_eq!(
            one.run(&graph, source, target),
            both.run(&graph, &graph, source, target)
        );

        assert!(
            both.search_space_len() * 4 < one.search_space_len() * 3,
            "two ends explored {}, one end explored {}",
            both.search_space_len(),
            one.search_space_len(),
        );
    }

    /// A search that collects nothing carries nothing to collect it in.
    #[test]
    fn collecting_nothing_costs_nothing() {
        use std::mem::size_of;
        assert_eq!(
            size_of::<BidirectionalSearch<Untracked>>(),
            size_of::<BidirectionalDijkstra>(),
        );
        assert!(
            size_of::<TrackedBidirectionalDijkstra>() > size_of::<BidirectionalDijkstra>(),
            "counting is supposed to take up room, or it is not counting"
        );
    }

    /// Each run says what that run did and nothing about the one before it.
    #[test]
    fn a_second_run_does_not_carry_the_first() {
        let edges = create_graph();
        let graph = StaticGraph::new(edges.clone());
        let reverse = reversed(&edges);
        let mut search = TrackedBidirectionalDijkstra::new();

        search.run(&graph, &reverse, 0, 3);
        let (forward, backward) = search.stats();
        let first = (*forward, *backward);
        search.run(&graph, &reverse, 0, 3);
        let (forward, backward) = search.stats();

        assert_eq!((*forward, *backward), first);
    }

    /// An end that cannot be reached is reported as unreachable rather than as
    /// some distance the two sides happened to agree on.
    #[test]
    fn an_unreachable_end_stays_unreachable() {
        let edges = vec![InputEdge::new(0, 1, 1_usize), InputEdge::new(2, 3, 1_usize)];
        let graph = StaticGraph::new(edges.clone());
        let reverse = reversed(&edges);
        let mut search = BidirectionalDijkstra::new();

        assert_eq!(search.run(&graph, &reverse, 0, 3), usize::MAX);
        assert_eq!(search.retrieve_node_path(), None);
    }
}
