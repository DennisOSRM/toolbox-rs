//! A search over the cells of a partition, run from both ends at once.
//!
//! # Why both ends
//!
//! [`MldSearch`](crate::mld_query::MldSearch) grows one front from the source
//! until it reaches the targets. Stepping over a cell means walking the clique
//! between its border nodes, so what a query costs is the number of times it
//! steps into a cell multiplied by how wide that cell's border is, and on a
//! continent nearly four fifths of that is spent on the coarsest level. Two
//! fronts of half the reach step into far fewer cells than one front of the
//! whole, and the clique walk falls with them.
//!
//! This is the query Delling, Goldberg, Pajor and Werneck describe: a
//! bidirectional search on the graph made of the overlay together with the two
//! cells holding the ends.
//!
//! # The graph the two sides walk
//!
//! Which arcs exist at a node does not depend on which way the search is
//! running, nor on what has been settled: a node is stepped over at the
//! highest level whose cell holds neither the source nor the target, and that
//! is fixed once the two ends are known. Both sides therefore walk one graph,
//! one of them forwards and the other backwards, which is what makes the
//! ordinary stopping rule sound here.
//!
//! Backwards means two things. The arcs of the graph are taken from `reverse`,
//! the graph with every arc turned around. The arcs across a cell are read out
//! of the same table the forward side reads, down a column rather than along a
//! row: what it costs to reach this border node from each of the others.
use log::debug;

use crate::{
    border_levels::BorderLevels,
    dense_heap::DenseHeap,
    graph::{Graph, INVALID_NODE_ID, NodeID},
    heap_stats::{Counters, HeapStats, Untracked},
    overlay::{CellTable, Overlay},
    packed_partition::PackedPartition,
};

/// A search over the cells from both ends, counting nothing.
pub type BidirectionalMldQuery = BidirectionalMldSearch<Untracked>;

/// The same search, counting what its two queues did.
pub type TrackedBidirectionalMldQuery = BidirectionalMldSearch<Counters>;

/// Which front a step belongs to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Side {
    Forward,
    Backward,
}

pub struct BidirectionalMldSearch<S: HeapStats<NodeID>> {
    forward: DenseHeap<S>,
    backward: DenseHeap<S>,
    /// The cells of the two ends, as the one word apiece that says all of
    /// them. Neither end moves during a run, so these are read once and then
    /// held against the word of every node the run settles.
    source_word: u128,
    target_word: u128,
    upper_bound: usize,
    meeting_node: NodeID,
}

impl<S: HeapStats<NodeID>> Default for BidirectionalMldSearch<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: HeapStats<NodeID>> BidirectionalMldSearch<S> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            forward: DenseHeap::<S>::new(),
            backward: DenseHeap::<S>::new(),
            source_word: 0,
            target_word: 0,
            upper_bound: usize::MAX,
            meeting_node: INVALID_NODE_ID,
        }
    }

    /// What each side's queue did on the last run.
    pub fn stats(&self) -> (&S, &S) {
        (self.forward.stats(), self.backward.stats())
    }

    /// The nodes the two queues ever held.
    #[must_use]
    pub fn search_space_len(&self) -> usize {
        self.forward.inserted_len() + self.backward.inserted_len()
    }

    /// The node the two sides met at, or `INVALID_NODE_ID` if they never did.
    #[must_use]
    pub fn meeting_node(&self) -> NodeID {
        self.meeting_node
    }

    /// The nodes of the way that was found, from source to target, as the
    /// search found them.
    ///
    /// Walked out of both queues, as with a plain search from both ends: back
    /// from the meeting node through the forward parents, and on from it
    /// through the backward ones.
    ///
    /// These are not all the nodes of the way. A step over a cell is one entry
    /// here and a path through that cell in the graph, and turning the one
    /// into the other is what
    /// [`unpack`](crate::path_unpacking::unpack) is for.
    ///
    /// # Panics
    ///
    /// Panics if a queue holds a node whose parent it does not.
    #[must_use]
    pub fn retrieve_packed_path(&self) -> Option<Vec<NodeID>> {
        if self.upper_bound == usize::MAX || self.meeting_node == INVALID_NODE_ID {
            return None;
        }

        let mut path = vec![self.meeting_node];
        let mut node = self.meeting_node;
        loop {
            let parent = self.forward.data(node);
            if parent == node {
                break;
            }
            path.push(parent);
            node = parent;
        }
        path.reverse();

        let mut node = self.meeting_node;
        loop {
            let parent = self.backward.data(node);
            if parent == node {
                break;
            }
            path.push(parent);
            node = parent;
        }
        Some(path)
    }

    /// Clears the search space, keeping what was allocated for it.
    pub fn clear(&mut self) {
        self.forward.clear();
        self.backward.clear();
        self.source_word = 0;
        self.target_word = 0;
        self.upper_bound = usize::MAX;
        self.meeting_node = INVALID_NODE_ID;
    }

    fn queue(&self, side: Side) -> &DenseHeap<S> {
        match side {
            Side::Forward => &self.forward,
            Side::Backward => &self.backward,
        }
    }

    fn queue_mut(&mut self, side: Side) -> &mut DenseHeap<S> {
        match side {
            Side::Forward => &mut self.forward,
            Side::Backward => &mut self.backward,
        }
    }

    /// What it costs to get from `source` to `target`, and `usize::MAX` when
    /// there is no way.
    ///
    /// `reverse` is `customization`'s graph with every arc turned around, and
    /// `reverse_borders` is [`BorderLevels`] worked out over that graph. The
    /// arcs turned around leave the same cells, but they are held in another
    /// order, so the backward side cannot read the table the forward side does.
    ///
    /// # Panics
    ///
    /// Panics if a level of the partition has no cells worked out for it.
    pub fn run<O: Overlay, G: Graph<u32>>(
        &mut self,
        customization: &O,
        reverse: &G,
        reverse_borders: &BorderLevels,
        source: NodeID,
        target: NodeID,
    ) -> usize {
        self.clear();
        debug!("[start] source: {source}, target: {target}");

        // neither end moves during a run, so the cells they sit in are read
        // once rather than once per settled node
        let partition = customization.partition();
        self.source_word = partition.word(source);
        self.target_word = partition.word(target);

        let graph = customization.graph();
        let borders = customization.border_levels();
        self.forward.insert(source, 0, source);
        self.backward.insert(target, 0, target);

        while !self.forward.is_empty() && !self.backward.is_empty() {
            let front = self.forward.min_weight();
            let back = self.backward.min_weight();
            // neither side reaches past its own front, so once the two fronts
            // together are no shorter than the best way already found there is
            // nothing left that could beat it
            if front + back >= self.upper_bound {
                break;
            }

            let side = if front <= back {
                Side::Forward
            } else {
                Side::Backward
            };
            let u = self.queue_mut(side).delete_min();
            let distance = self.queue(side).weight(u);

            // this side is done with u, so whatever the other side holds for
            // it is a way from one end to the other through it
            let other = match side {
                Side::Forward => &self.backward,
                Side::Backward => &self.forward,
            };
            if other.inserted(u) {
                let through = distance + other.weight(u);
                if through < self.upper_bound {
                    self.upper_bound = through;
                    self.meeting_node = u;
                    debug!("[meet] {u} at {through}");
                }
            }

            match partition.query_level(self.source_word, self.target_word, u) {
                Some(level) => {
                    // the cell is stepped over once for each way into it, not
                    // once for each of its border nodes: a node reached from
                    // inside the cell was reached across it already, and the
                    // table holds shortest distances
                    let came_from = self.queue(side).data(u);
                    if u == came_from
                        || !partition.same_cell_at(
                            partition.word(u),
                            partition.word(came_from),
                            level,
                        )
                    {
                        self.relax_across_cell(customization, partition, side, u, distance, level);
                    }
                    match side {
                        Side::Forward => {
                            self.relax_out_of_cell(graph, borders, side, u, distance, level);
                        }
                        Side::Backward => {
                            self.relax_out_of_cell(
                                reverse,
                                reverse_borders,
                                side,
                                u,
                                distance,
                                level,
                            );
                        }
                    }
                }
                None => match side {
                    Side::Forward => self.relax_every_arc(graph, side, u, distance),
                    Side::Backward => self.relax_every_arc(reverse, side, u, distance),
                },
            }
        }

        self.upper_bound
    }

    /// The arcs across the cell, which the customization worked out.
    ///
    /// The forward side reads the row of this node, what it costs to get from
    /// here to each of the others. The backward side reads its column, what it
    /// costs to get to here from each of the others.
    #[inline(never)]
    fn relax_across_cell<O: Overlay>(
        &mut self,
        customization: &O,
        partition: &PackedPartition,
        side: Side,
        node: NodeID,
        distance: usize,
        level: usize,
    ) {
        let cell = partition.cell_of(node, level);
        // an index into the customization, which lends the table out rather
        // than counting it, so this is a load rather than a lock and a hash
        let Some(distances) = customization.distances_of(level, cell) else {
            return;
        };

        let Some(place) = distances.place_of(node) else {
            // the node is inside the cell rather than on its border
            return;
        };
        let here = u32::try_from(node).unwrap_or(u32::MAX);
        match side {
            Side::Forward => {
                for (&other, &across) in distances.border_nodes().iter().zip(distances.row(place)) {
                    if across == u32::MAX || other == here {
                        continue;
                    }
                    self.relax(side, other as NodeID, distance + across as usize, node);
                }
            }
            Side::Backward => {
                for (&other, &across) in
                    distances.border_nodes().iter().zip(distances.column(place))
                {
                    if across == u32::MAX || other == here {
                        continue;
                    }
                    self.relax(side, other as NodeID, distance + across as usize, node);
                }
            }
        }
    }

    /// The arcs of the graph that leave the cell, which is how a search gets
    /// out of one.
    #[inline(never)]
    fn relax_out_of_cell<G: Graph<u32>>(
        &mut self,
        graph: &G,
        borders: &BorderLevels,
        side: Side,
        node: NodeID,
        distance: usize,
        level: usize,
    ) {
        for edge in graph.edge_range(node) {
            // read in step with the arcs rather than asked of the partition,
            // which would be a jump into an array as wide as the graph for
            // every arc of every node the search settles
            if !borders.leaves_cell(edge, level) {
                continue;
            }
            let target = graph.target(edge);
            self.relax(side, target, distance + *graph.data(edge) as usize, node);
        }
    }

    /// Every arc of the graph, which is what this does inside a cell holding
    /// one of the two ends.
    #[inline(never)]
    fn relax_every_arc<G: Graph<u32>>(
        &mut self,
        graph: &G,
        side: Side,
        node: NodeID,
        distance: usize,
    ) {
        for edge in graph.edge_range(node) {
            let target = graph.target(edge);
            self.relax(side, target, distance + *graph.data(edge) as usize, node);
        }
    }

    fn relax(&mut self, side: Side, node: NodeID, weight: usize, came_from: NodeID) {
        self.queue_mut(side)
            .insert_or_decrease(node, weight, came_from);
    }
}

#[cfg(test)]
mod tests {
    use rand::{RngExt, SeedableRng, prelude::StdRng};

    use crate::{
        bidirectional_mld_query::{BidirectionalMldQuery, TrackedBidirectionalMldQuery},
        border_levels::BorderLevels,
        customization::Customization,
        edge::InputEdge,
        graph::NodeID,
        grid_graph::{grid, grid_directory, grid_edges},
        mld_query::TrackedMldQuery,
        static_graph::StaticGraph,
        unidirectional_dijkstra::UnidirectionalDijkstra,
    };

    /// The same arcs, turned around, which is what the backward side walks.
    fn reversed(edges: &[InputEdge<u32>]) -> StaticGraph<u32> {
        StaticGraph::new(
            edges
                .iter()
                .map(|edge| InputEdge::new(edge.target, edge.source, edge.data))
                .collect(),
        )
    }

    fn by_dijkstra(graph: &StaticGraph<u32>, source: NodeID, target: NodeID) -> usize {
        UnidirectionalDijkstra::new().run(graph, source, target)
    }

    fn agrees_with_dijkstra_on(side: usize, both_ways: bool, seed: u64, rounds: usize) {
        let mut rng = StdRng::seed_from_u64(seed);
        for round in 0..rounds {
            // the same grid, with weights that give the cells something to say
            let mut edges = grid_edges(side, both_ways);
            for edge in &mut edges {
                edge.data = rng.random_range(1..25_u32);
            }
            let reverse = reversed(&edges);
            let plain = StaticGraph::new(edges.clone());
            let customization = Customization::new(StaticGraph::new(edges), grid_directory(side));
            let backward = BorderLevels::of(&reverse, customization.partition());

            let count = side * side;
            let mut query = BidirectionalMldQuery::new();
            for _ in 0..8 {
                let source = rng.random_range(0..count);
                let target = rng.random_range(0..count);
                let expected = by_dijkstra(&plain, source, target);
                assert_eq!(
                    query.run(&customization, &reverse, &backward, source, target),
                    expected,
                    "round {round}, side {side}, both_ways {both_ways}: {source} to {target}"
                );
            }
        }
    }

    #[test]
    fn a_grid_of_arcs_both_ways_agrees_with_a_plain_search() {
        agrees_with_dijkstra_on(8, true, 0x_A11D, 20);
    }

    /// The rows run one way round, so a cell costs a different amount to cross
    /// each way. This is where reading a column as though it were a row would
    /// show, and where an assumption of symmetry would hide.
    #[test]
    fn a_grid_of_arcs_one_way_agrees_with_a_plain_search() {
        agrees_with_dijkstra_on(8, false, 0x_1A9E, 20);
    }

    #[test]
    fn a_grid_of_six_levels_agrees_with_a_plain_search() {
        agrees_with_dijkstra_on(64, false, 0x_51E5, 3);
    }

    /// An end that cannot be reached is reported as unreachable.
    #[test]
    fn an_unreachable_end_stays_unreachable() {
        // a grid whose arcs all run one way, so the far corner cannot reach
        // the near one
        let side = 16;
        let edges = grid_edges(side, false);
        let reverse = reversed(&edges);
        let customization = Customization::new(StaticGraph::new(edges), grid_directory(side));
        let backward = BorderLevels::of(&reverse, customization.partition());

        let mut query = BidirectionalMldQuery::new();
        assert_eq!(
            query.run(&customization, &reverse, &backward, side * side - 1, 0),
            usize::MAX
        );
    }

    /// Each run says what that run did and nothing about the one before it.
    #[test]
    fn a_second_run_does_not_carry_the_first() {
        let side = 16;
        let edges = grid_edges(side, true);
        let reverse = reversed(&edges);
        let customization = Customization::new(StaticGraph::new(edges), grid_directory(side));
        let backward = BorderLevels::of(&reverse, customization.partition());

        let mut query = BidirectionalMldQuery::new();
        let first = query.run(&customization, &reverse, &backward, 0, side * side - 1);
        query.run(&customization, &reverse, &backward, 3, 7);
        assert_eq!(
            query.run(&customization, &reverse, &backward, 0, side * side - 1),
            first
        );
    }

    /// Two fronts step into fewer cells than one, which is the whole reason to
    /// run it from both ends.
    #[test]
    fn two_fronts_settle_less_than_one() {
        for side in [32_usize, 64] {
            let edges = grid_edges(side, true);
            let reverse = reversed(&edges);
            let (graph, directory) = grid(side, true);
            let customization = Customization::new(graph, directory);
            let backward = BorderLevels::of(&reverse, customization.partition());
            let (source, target) = (0, side * side - 1);

            let mut both = TrackedBidirectionalMldQuery::new();
            let by_both = both.run(&customization, &reverse, &backward, source, target);

            let one_way = Customization::new(StaticGraph::new(edges), grid_directory(side));
            let mut one = TrackedMldQuery::new();
            one.run(&one_way, source, &[target]);

            assert_eq!(by_both, one.distance(target));
            let (forward, backward) = both.stats();
            let settled = (forward.deleted + backward.deleted) as f64;
            let one_sided = one.stats().deleted as f64;
            assert!(
                settled < one_sided,
                "side {side}: two fronts settled {settled} against one front's {one_sided}"
            );
        }
    }
}
