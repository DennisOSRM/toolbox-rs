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

use crate::{
    border_levels::BorderLevels,
    dense_heap::DenseHeap,
    graph::{Arcs, NodeID},
    heap_stats::{Counters, HeapStats, Untracked},
    overlay::{CellTable, Overlay},
    packed_partition::PackedPartition,
};

/// A query that counts nothing, which is what a run whose time is being taken
/// wants.
pub type MldQuery = MldSearch<Untracked>;

/// The same query, counting what its queue did.
pub type TrackedMldQuery = MldSearch<Counters>;

pub struct MldSearch<S: HeapStats<NodeID>> {
    queue: DenseHeap<S>,
    /// Which cells hold a target, every level in one run of memory with
    /// `holds_target_at` saying where each of them starts.
    ///
    /// Worked out once per run by walking each target up the levels. Asking
    /// instead whether any target shares a cell with a node would cost the
    /// number of targets, every time a node is settled.
    ///
    /// One run rather than a run per level: a level apiece is a vector of
    /// vectors, so reading it is the pointer of the level and then the entry,
    /// two reads that depend on one another, and the query makes that read for
    /// every node it settles.
    holds_target: Vec<bool>,
    holds_target_at: Vec<usize>,
    /// The cells marked, so that `holds_target` can be put back the way it was
    /// without walking it.
    ///
    /// It is as wide as the partition, which on a continent is two thirds of a
    /// million cells over six levels. Building it per run costs more than the
    /// search does by a factor of ten thousand at a low rank: a search that
    /// settles a hundred nodes was paying to allocate and zero six megabytes
    /// first. It is built once, and a run puts back only what it touched.
    marked: Vec<usize>,
    targets: FxHashSet<NodeID>,
    /// The cells the source sits in, as the one word that says all of them.
    source_word: u128,
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
            queue: DenseHeap::<S>::new(),
            holds_target: Vec::new(),
            holds_target_at: Vec::new(),
            marked: Vec::new(),
            targets: FxHashSet::default(),
            source_word: 0,
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

    /// What this search has taken, which goes with the nodes of the graph and
    /// not with any budget set for the cell tables.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.queue.bytes()
    }

    /// The node a reached node was reached from.
    ///
    /// `None` if the search never reached it, and the node itself if it is the
    /// source, there being nowhere it was reached from. Every node the search
    /// reached has one, so this is the tree of the arcs it relaxed and kept:
    /// one arc per node, the last one to improve it.
    #[must_use]
    pub fn parent(&self, node: NodeID) -> Option<NodeID> {
        self.queue.inserted(node).then(|| self.queue.data(node))
    }

    /// The nodes of the way to a target, as the search found them.
    ///
    /// These are not all the nodes of the way. A step over a cell is one entry
    /// here and a path through that cell in the graph, and turning the one
    /// into the other is what
    /// [`unpack`](crate::path_unpacking::unpack) is for.
    ///
    /// # Panics
    ///
    /// Panics if the queue holds a node whose parent it does not.
    #[must_use]
    pub fn retrieve_packed_path(&self, target: NodeID) -> Option<Vec<NodeID>> {
        if !self.queue.inserted(target) {
            return None;
        }
        let mut path = vec![target];
        let mut node = target;
        loop {
            let parent = self.queue.data(node);
            if parent == node {
                path.reverse();
                return Some(path);
            }
            path.push(parent);
            node = parent;
        }
    }

    /// Clears the search space, keeping what was allocated for it.
    pub fn clear(&mut self) {
        self.queue.clear();
        self.targets.clear();
        self.reached_target_count = 0;
        for place in self.marked.drain(..) {
            self.holds_target[place] = false;
        }
        self.source_word = 0;
    }

    /// Makes room for the cells of this partition, once.
    ///
    /// A second run over the same partition finds the room already there and
    /// the entries already put back by `clear`.
    fn make_room_for<O: Overlay>(&mut self, customization: &O) {
        let levels = customization.levels();
        let mut at = Vec::with_capacity(levels + 1);
        let mut total = 0;
        for level in 0..levels {
            at.push(total);
            total += customization.cells_on_level(level);
        }
        at.push(total);
        if self.holds_target_at == at {
            debug_assert!(
                !self.holds_target.iter().any(|&marked| marked),
                "a cell was left marked by the run before this one"
            );
            return;
        }
        self.holds_target_at = at;
        self.holds_target = vec![false; total];
    }

    /// Runs the search, and says whether every target was reached.
    ///
    /// # Panics
    ///
    /// Panics if a level of the partition has no cells worked out for it,
    /// which would mean a directory that does not describe the graph.
    pub fn run<O: Overlay>(
        &mut self,
        customization: &O,
        source: NodeID,
        targets: &[NodeID],
    ) -> bool {
        self.clear();
        self.targets.extend(targets.iter().copied());
        debug!("[start] source: {source}, {} targets", self.targets.len());

        // everything that is asked per settled node is worked out once here
        let partition = customization.partition();
        let level_count = partition.levels();
        self.make_room_for(customization);
        for &target in &self.targets {
            let word = partition.word(target);
            for level in 0..level_count {
                let place = self.holds_target_at[level] + partition.cell_in(word, level) as usize;
                if !self.holds_target[place] {
                    self.holds_target[place] = true;
                    self.marked.push(place);
                }
            }
        }

        // the source's cells never move during a run, so they are not asked
        // for once per settled node
        self.source_word = partition.word(source);

        let graph = customization.graph();
        let borders = customization.border_levels();
        self.queue.insert(source, 0, source);

        while !self.queue.is_empty() && self.reached_target_count < self.targets.len() {
            let u = self.queue.delete_min();
            let distance = self.queue.weight(u);

            if self.targets.contains(&u) {
                self.reached_target_count += 1;
                debug!("[done] reached {u} at {distance}");
            }

            match self.level_to_step_over(partition, u) {
                Some(level) => {
                    // The cell is stepped over once for each way into it, not
                    // once for each of its border nodes. A node reached from
                    // inside the cell was reached across it already, and the
                    // table holds shortest distances, so what a second step
                    // from there would offer -- in at one border node, across
                    // to a second, across again to a third -- is never shorter
                    // than the single step the table gave for going straight
                    // in at the first and out at the third. On a cell of a
                    // continent with a thousand nodes on its border that is a
                    // thousand relaxations for each of a thousand nodes, in
                    // place of a thousand for each way in.
                    let came_from = self.queue.data(u);
                    if u == came_from
                        || !partition.same_cell_at(
                            partition.word(u),
                            partition.word(came_from),
                            level,
                        )
                    {
                        self.relax_across_cell(customization, partition, u, distance, level);
                    }
                    self.relax_out_of_cell(graph, borders, u, distance, level);
                }
                None => self.relax_every_arc(graph, u, distance),
            }
        }

        self.reached_target_count == self.targets.len()
    }

    /// The highest level whose cell around this node holds neither the source
    /// nor a target, and `None` when even the finest one does.
    ///
    /// The source is answered by the bits of the two words, which needs no
    /// cell id at all. The targets are a set rather than one node, so those
    /// are still asked cell by cell, and the walk runs no further than the
    /// level the source alone allows.
    #[inline(never)]
    fn level_to_step_over(&self, partition: &PackedPartition, node: NodeID) -> Option<usize> {
        let word = partition.word(node);
        let highest = partition.highest_different_level(word, self.source_word)?;
        (0..=highest).rev().find(|&level| {
            let place = self.holds_target_at[level] + partition.cell_in(word, level) as usize;
            !self.holds_target[place]
        })
    }

    /// The arcs across the cell, which the customization worked out.
    #[inline(never)]
    fn relax_across_cell<O: Overlay>(
        &mut self,
        customization: &O,
        partition: &PackedPartition,
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

        let Some(from) = distances.place_of(node) else {
            // the node is inside the cell rather than on its border, which is
            // where a search that started here begins
            return;
        };
        // the row and the nodes it is about, walked in step as two pieces of
        // memory rather than asked for an entry at a time
        let here = u32::try_from(node).unwrap_or(u32::MAX);
        for (&target, &across) in distances.border_nodes().iter().zip(distances.row(from)) {
            if across == u32::MAX || target == here {
                continue;
            }
            self.relax(target as NodeID, distance + across as usize, node);
        }
    }

    /// The arcs of the graph that leave the cell, which is how the search gets
    /// out of one.
    #[inline(never)]
    fn relax_out_of_cell<G: Arcs<u32>>(
        &mut self,
        graph: &G,
        borders: &BorderLevels,
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
            self.relax(target, distance + graph.weight(edge) as usize, node);
        }
    }

    /// Every arc of the graph, which is what a plain Dijkstra does and what
    /// this does inside a cell that holds the source or a target.
    #[inline(never)]
    fn relax_every_arc<G: Arcs<u32>>(&mut self, graph: &G, node: NodeID, distance: usize) {
        for edge in graph.edge_range(node) {
            let target = graph.target(edge);
            self.relax(target, distance + graph.weight(edge) as usize, node);
        }
    }

    /// One look into the queue rather than up to four.
    ///
    /// Stepping over a cell relaxes every border node of it, and on a coarse
    /// cell of a continent that is thousands of them, nearly all settled
    /// already. Asking in turn whether each has been seen, whether it is still
    /// on the queue and what it is held at, is three looks apiece to find out
    /// there is nothing to do.
    #[inline(never)]
    fn relax(&mut self, node: NodeID, distance: usize, from: NodeID) {
        self.queue.insert_or_decrease(node, distance, from);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        customization::Customization,
        edge::InputEdge,
        grid_graph::{grid, grid_directory, grid_edges},
        heap_stats::SettledNodes,
        level_directory::LevelDirectory,
        static_graph::StaticGraph,
        unidirectional_dijkstra::{TrackedUnidirectionalDijkstra, UnidirectionalDijkstra},
    };
    use rand::{RngExt, SeedableRng, prelude::StdRng};

    /// Four nodes in a line, cut into two cells of two, joined above.
    fn two_cells() -> Customization {
        let edges = vec![
            InputEdge::new(0, 1, 3_u32),
            InputEdge::new(1, 0, 3_u32),
            InputEdge::new(1, 2, 7_u32),
            InputEdge::new(2, 1, 7_u32),
            InputEdge::new(2, 3, 5_u32),
            InputEdge::new(3, 2, 5_u32),
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
        let edges = vec![InputEdge::new(0, 1, 1_u32)];
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
                edge.data = rng.random_range(1..25_u32);
            }
            let customization = Customization::new(StaticGraph::new(edges), grid_directory(side));

            let count = side * side;
            let source = rng.random_range(0..count as NodeID);
            let targets = (0..rng.random_range(1..6))
                .map(|_| rng.random_range(0..count as NodeID))
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

    /// Six levels, so there is a cell of a thousand nodes to step over and
    /// five sizes of cell between that and the source. A partition of a road
    /// network is shaped like this and a grid of eight is not.
    #[test]
    fn a_grid_of_six_levels_agrees_with_a_plain_search() {
        agrees_with_dijkstra_on(64, false, 0x_51E5, 3);
    }

    /// The query holds the coarsest level it can, and goes finer only inside
    /// the two cells that force it to.
    ///
    /// Every level of a partition is sound to step over, so a query that
    /// descended early would hand back exactly the same distances and no test
    /// of those distances would say a word. It would only be slower. The rule
    /// is worked out here from the directory rather than asked of the query,
    /// so that this checks the rule rather than the code that implements it.
    #[test]
    fn nothing_is_stepped_over_below_the_top_level_without_cause() {
        let side = 64;
        let (graph, directory) = grid(side, true);
        let customization = Customization::new(graph, directory);
        let directory = customization.directory();
        let top = directory.levels() - 1;

        let mut query = MldSearch::<SettledNodes>::new();
        let count = side * side;
        for (source, target) in [
            (0, count - 1),
            (count - 1, 0),
            (7, count / 2),
            (count / 3, 11),
        ] {
            query.run(&customization, source, &[target]);
            let source_top = directory.cell_of(source, top);
            let target_top = directory.cell_of(target, top);

            let mut settled = 0;
            for &node in query.stats().settled() {
                settled += 1;
                let cell = directory.cell_of(node, top);
                if cell == source_top || cell == target_top {
                    continue;
                }
                // outside both ends' top cells, so the top level holds neither
                // end and is the one to step over
                let used = (0..=top).rev().find(|&level| {
                    let at = directory.cell_of(node, level);
                    at != directory.cell_of(source, level) && at != directory.cell_of(target, level)
                });
                assert_eq!(
                    used,
                    Some(top),
                    "{source} to {target}: node {node} sits outside both ends' top cells \
                     and was stepped over at {used:?} rather than at {top}"
                );
            }
            assert!(settled > 0, "{source} to {target}: nothing was settled");
        }
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
