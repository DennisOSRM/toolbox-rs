//! A search from one node to a set of them that goes over the cells of a
//! partition rather than through them.
//!
//! # What it does
//!
//! A plain Dijkstra walks every arc it comes to. This one walks the arcs of a
//! cell only where it has to. Away from the source and the targets it steps
//! from one side of a cell straight to the other, at the cost the customization
//! worked out for that pair of border nodes, and never looks at what is inside.
//! The larger the cell it can step over, the more of the graph it never reads.
//!
//! # The rule
//!
//! For a settled node, the highest level whose cell holds neither the source
//! nor any target is the level it may step over. Inside a cell that holds one
//! of them, a step over the cell would skip the very node the search is
//! looking for, so there the arcs of the graph are walked as usual.
//!
//! At the level it may step over, two sets of arcs leave the node: the ones
//! across its cell, which the customization tabulated, and the ones out of the
//! cell, which are arcs of the graph. Both are needed. The first is how the
//! search crosses a cell and the second is how it leaves one.
//!
//! # Which way round it runs
//!
//! Forwards only. A search that met a second one coming back from the target
//! would be quicker for a single pair, but there is no second search to meet
//! when the targets are a set, and a comparison against a plain Dijkstra is
//! easier to read when neither side is bidirectional: what is left in it is
//! what the cells bought and nothing else.

use log::debug;
use rustc_hash::FxHashSet;
use std::sync::Arc;

use crate::{
    addressable_binary_heap::AddressableHeapWithStats,
    customization::{CellDistances, Customization, Level},
    graph::{Graph, NodeID},
    heap_stats::{Counters, HeapStats, Untracked},
};

/// A query that counts nothing, which is what a run whose time is being taken
/// wants.
pub type MldQuery = MldSearch<Untracked>;

/// The same query, counting what its queue did.
pub type TrackedMldQuery = MldSearch<Counters>;

pub struct MldSearch<S: HeapStats<NodeID>> {
    queue: AddressableHeapWithStats<NodeID, usize, NodeID, S>,
    /// The cells of every level, held for the length of a run.
    ///
    /// `of_node` is a flat array over the nodes, so the cell a node sits in on
    /// a level is one index. Asking the directory instead walks the parent of
    /// a cell once per level between the finest and the one asked about, and
    /// the walk happens for every node the search settles.
    levels: Vec<Arc<Level>>,
    /// The tabulated cells, by level and then by cell.
    ///
    /// Filled as cells are first stepped over. Reaching for one through the
    /// customization takes a lock, and a query that did that per settled node
    /// would spend its time queueing rather than searching. Cell ids run from
    /// zero without gaps, so this is an index rather than a hash.
    matrices: Vec<Vec<Option<Arc<CellDistances>>>>,
    /// Which cells hold a target, by level and then by cell.
    ///
    /// Worked out once per run by walking each target up the levels. Asking
    /// instead whether any target shares a cell with a node would cost the
    /// number of targets, every time a node is settled.
    holds_target: Vec<Vec<bool>>,
    targets: FxHashSet<NodeID>,
    reached_target_count: usize,
}

impl<S: HeapStats<NodeID>> Default for MldSearch<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: HeapStats<NodeID>> MldSearch<S> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            queue: AddressableHeapWithStats::<NodeID, usize, NodeID, S>::new(),
            levels: Vec::new(),
            matrices: Vec::new(),
            holds_target: Vec::new(),
            targets: FxHashSet::default(),
            reached_target_count: 0,
        }
    }

    /// What the last run did, as far as the collector was asked to keep.
    pub fn stats(&self) -> &S {
        self.queue.stats()
    }

    /// The nodes the queue ever held.
    #[must_use]
    pub fn search_space_len(&self) -> usize {
        self.queue.inserted_len()
    }

    /// What it costs to reach a node, once a run has reached it.
    #[must_use]
    pub fn distance(&self, node: NodeID) -> usize {
        self.queue.weight(node)
    }

    /// Clears the search space, keeping what was allocated for it.
    pub fn clear(&mut self) {
        self.queue.clear();
        self.targets.clear();
        self.reached_target_count = 0;
        for level in &mut self.matrices {
            level.clear();
        }
        for level in &mut self.holds_target {
            level.clear();
        }
        self.levels.clear();
    }

    /// Runs the search, and says whether every target was reached.
    ///
    /// # Panics
    ///
    /// Panics if a level of the partition has no cells worked out for it,
    /// which would mean a directory that does not describe the graph.
    pub fn run(
        &mut self,
        customization: &Customization,
        source: NodeID,
        targets: &[NodeID],
    ) -> bool {
        self.clear();
        self.targets.extend(targets.iter().copied());
        debug!("[start] source: {source}, {} targets", self.targets.len());

        let directory = customization.directory();
        let level_count = directory.levels();

        // everything that is asked per settled node is worked out once here
        self.levels = (0..level_count)
            .map(|level| customization.level(level))
            .collect();
        self.matrices = (0..level_count)
            .map(|level| vec![None; directory.cells_on_level(level)])
            .collect();
        self.holds_target = (0..level_count)
            .map(|level| vec![false; directory.cells_on_level(level)])
            .collect();
        for &target in &self.targets {
            for level in 0..level_count {
                let cell = self.levels[level].of_node[target] as usize;
                self.holds_target[level][cell] = true;
            }
        }

        let graph = customization.graph();
        self.queue.insert(source, 0, source);

        while !self.queue.is_empty() && self.reached_target_count < self.targets.len() {
            let u = self.queue.delete_min();
            let distance = self.queue.weight(u);

            if self.targets.contains(&u) {
                self.reached_target_count += 1;
                debug!("[done] reached {u} at {distance}");
            }

            match self.level_to_step_over(u, source) {
                Some(level) => {
                    self.relax_across_cell(customization, u, distance, level);
                    self.relax_out_of_cell(graph, u, distance, level);
                }
                None => self.relax_every_arc(graph, u, distance),
            }
        }

        self.reached_target_count == self.targets.len()
    }

    /// The highest level whose cell around this node holds neither the source
    /// nor a target, and `None` when even the finest one does.
    fn level_to_step_over(&self, node: NodeID, source: NodeID) -> Option<usize> {
        (0..self.levels.len()).rev().find(|&level| {
            let cell = self.levels[level].of_node[node];
            cell != self.levels[level].of_node[source] && !self.holds_target[level][cell as usize]
        })
    }

    /// The arcs across the cell, which the customization worked out.
    fn relax_across_cell(
        &mut self,
        customization: &Customization,
        node: NodeID,
        distance: usize,
        level: usize,
    ) {
        let cell = self.levels[level].of_node[node];
        let distances = match &self.matrices[level][cell as usize] {
            Some(distances) => distances.clone(),
            None => {
                // the one time this cell is reached for, and the only time a
                // lock is taken for it
                let Some(distances) = customization.distances_of(level, cell) else {
                    return;
                };
                self.matrices[level][cell as usize] = Some(distances.clone());
                distances
            }
        };

        let Some(from) = distances.place_of(node) else {
            // the node is inside the cell rather than on its border, which is
            // where a search that started here begins
            return;
        };
        for (to, &target) in distances.border_nodes.iter().enumerate() {
            let across = distances.distance(from, to);
            if across == usize::MAX || target == node {
                continue;
            }
            self.relax(target, distance + across, node);
        }
    }

    /// The arcs of the graph that leave the cell, which is how the search gets
    /// out of one.
    fn relax_out_of_cell<G: Graph<usize>>(
        &mut self,
        graph: &G,
        node: NodeID,
        distance: usize,
        level: usize,
    ) {
        let cell = self.levels[level].of_node[node];
        for edge in graph.edge_range(node) {
            let target = graph.target(edge);
            if self.levels[level].of_node[target] == cell {
                continue;
            }
            self.relax(target, distance + *graph.data(edge), node);
        }
    }

    /// Every arc of the graph, which is what a plain Dijkstra does and what
    /// this does inside a cell that holds the source or a target.
    fn relax_every_arc<G: Graph<usize>>(&mut self, graph: &G, node: NodeID, distance: usize) {
        for edge in graph.edge_range(node) {
            let target = graph.target(edge);
            self.relax(target, distance + *graph.data(edge), node);
        }
    }

    fn relax(&mut self, node: NodeID, distance: usize, from: NodeID) {
        if !self.queue.inserted(node) {
            self.queue.insert(node, distance, from);
            return;
        }
        if self.queue.contains(node) && self.queue.weight(node) > distance {
            self.queue
                .decrease_key_and_update_data(node, distance, from);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        customization::Customization,
        edge::InputEdge,
        grid_graph::{grid, grid_directory, grid_edges},
        level_directory::LevelDirectory,
        static_graph::StaticGraph,
        unidirectional_dijkstra::{TrackedUnidirectionalDijkstra, UnidirectionalDijkstra},
    };
    use rand::{RngExt, SeedableRng, prelude::StdRng};

    /// Four nodes in a line, cut into two cells of two, joined above.
    fn two_cells() -> Customization {
        let edges = vec![
            InputEdge::new(0, 1, 3_usize),
            InputEdge::new(1, 0, 3_usize),
            InputEdge::new(1, 2, 7_usize),
            InputEdge::new(2, 1, 7_usize),
            InputEdge::new(2, 3, 5_usize),
            InputEdge::new(3, 2, 5_usize),
        ];
        let directory = LevelDirectory::new(vec![0, 0, 1, 1], vec![vec![0, 0]]);
        Customization::new(StaticGraph::new(edges), directory)
    }

    /// What a plain search says, which is what this one has to say too.
    fn by_dijkstra(customization: &Customization, source: NodeID, target: NodeID) -> usize {
        UnidirectionalDijkstra::new().run(customization.graph(), source, target)
    }

    #[test]
    fn a_line_of_two_cells_costs_what_its_arcs_cost() {
        let customization = two_cells();
        let mut query = MldQuery::new();

        assert!(query.run(&customization, 0, &[3]));
        assert_eq!(query.distance(3), 15);
        assert_eq!(query.distance(3), by_dijkstra(&customization, 0, 3));
    }

    #[test]
    fn a_target_in_the_cell_of_the_source_is_reached_through_the_graph() {
        let customization = two_cells();
        let mut query = MldQuery::new();

        assert!(query.run(&customization, 0, &[1]));
        assert_eq!(query.distance(1), 3);
    }

    #[test]
    fn every_target_of_a_set_is_reached() {
        let customization = two_cells();
        let mut query = MldQuery::new();

        assert!(query.run(&customization, 0, &[1, 2, 3]));
        assert_eq!(query.distance(1), 3);
        assert_eq!(query.distance(2), 10);
        assert_eq!(query.distance(3), 15);
    }

    #[test]
    fn a_target_that_cannot_be_reached_is_reported() {
        // one arc into node 1 and none out of it, so nothing reaches node 0
        let edges = vec![InputEdge::new(0, 1, 1_usize)];
        let directory = LevelDirectory::new(vec![0, 1], vec![vec![0, 0]]);
        let customization = Customization::new(StaticGraph::new(edges), directory);
        let mut query = MldQuery::new();

        assert!(!query.run(&customization, 1, &[0]));
    }

    /// The whole of it, on graphs nobody worked out by hand: whatever the
    /// query says a target costs is what a search that knows nothing of cells
    /// says it costs.
    fn agrees_with_dijkstra_on(side: usize, both_ways: bool, seed: u64, rounds: usize) {
        let mut rng = StdRng::seed_from_u64(seed);
        for round in 0..rounds {
            // the same grid, with weights that give the cells something to say
            let mut edges = grid_edges(side, both_ways);
            for edge in &mut edges {
                edge.data = rng.random_range(1..25_usize);
            }
            let customization = Customization::new(StaticGraph::new(edges), grid_directory(side));

            let count = side * side;
            let source = rng.random_range(0..count);
            let targets = (0..rng.random_range(1..6))
                .map(|_| rng.random_range(0..count))
                .collect::<Vec<_>>();

            let mut query = MldQuery::new();
            query.run(&customization, source, &targets);

            for &target in &targets {
                let expected = by_dijkstra(&customization, source, target);
                if expected == usize::MAX {
                    continue;
                }
                assert_eq!(
                    query.distance(target),
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

    #[test]
    fn a_grid_of_arcs_one_way_agrees_with_a_plain_search() {
        // the rows run one way round, so a cell costs a different amount to
        // cross each way, which is where an assumption of symmetry would show
        agrees_with_dijkstra_on(8, false, 0x_1A9E, 20);
    }

    #[test]
    fn a_grid_of_three_levels_agrees_with_a_plain_search() {
        agrees_with_dijkstra_on(16, false, 0x_DEEB, 10);
    }

    /// Stepping over a cell is supposed to save work, not merely give the same
    /// answer by a longer road.
    ///
    /// The margin is what makes this worth asserting. Every level of the
    /// partition is a sound one to step over, so a rule that picked the
    /// finest instead of the coarsest would still give the right answers and
    /// no test of those answers would notice. It would step over cells of four
    /// nodes rather than of the whole quarter of the grid, and settle very
    /// nearly what a plain search settles. Measured on this grid, the coarsest
    /// rule settles about seven tenths of what the plain search does and the
    /// finest rule about all of it, so anything under the plain count by a
    /// clear margin says the level being chosen is the high one.
    #[test]
    fn a_far_target_is_reached_over_the_coarsest_cells_that_will_serve() {
        for side in [16_usize, 32] {
            let (graph, directory) = grid(side, true);
            let customization = Customization::new(graph, directory);
            let (source, target) = (0, side * side - 1);

            let mut query = TrackedMldQuery::new();
            query.run(&customization, source, &[target]);

            let mut plain = TrackedUnidirectionalDijkstra::new();
            let by_plain = plain.run(customization.graph(), source, target);

            assert_eq!(query.distance(target), by_plain);
            let settled = query.stats().deleted as f64;
            let plainly = plain.stats().deleted as f64;
            assert!(
                plainly / settled > 1.25,
                "side {side}: the query settled {settled} against {plainly}, which is what \
                 stepping over the finest cells rather than the coarsest looks like"
            );
        }
    }
}
