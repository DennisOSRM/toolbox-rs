//! The highest level at which each arc leaves a cell, worked out once.
//!
//! # What it is for
//!
//! A search stepping over a cell has to take the arcs that leave it, and those
//! are the arcs of the graph whose two ends sit in different cells at that
//! level. Asked per arc, that question is a read of the cell of the far end,
//! which is a jump into an array as wide as the graph and a miss almost every
//! time. A settled node has a couple of arcs, so the search pays a couple of
//! misses for every node it settles, and pays them only to throw most of the
//! arcs away.
//!
//! Cells nest, so an arc that leaves its cell on some level leaves it on every
//! level below too. One number per arc therefore settles the question for
//! every level at once: the highest level whose cells its two ends part on.
//! Held beside the arcs and read in step with them, it turns those misses into
//! a byte off a run of memory the search is already walking.
//!
//! # What OSRM does, and what this does instead
//!
//! OSRM works out the same number, in `MultiLevelGraph::GetHighestBorderLevel`,
//! which is its partition's `GetHighestDifferentLevel` over the two ends of the
//! arc. It then sorts each node's arcs by it, so that the arcs leaving the cell
//! at a level are the tail of that node's block and can be walked without the
//! others being read at all.
//!
//! That sorting buys two things: the question is not asked, and the arcs that
//! stay inside are not read. The first is the whole of the cost here. The
//! second is worth having where a node has many arcs, and a road network is not
//! that: a continent comes to a shade over two arcs a node, so skipping the
//! ones that stay inside saves about one sequential read where the offsets that
//! make it possible cost a table of a node by a level. OSRM can afford that
//! table because it renumbers the border nodes to the front and so keeps only a
//! short prefix of it. Until that renumbering is here, the number is kept per
//! arc and the arcs are left where they lie.

use crate::{
    graph::{EdgeID, Graph},
    packed_partition::PackedPartition,
};

/// An arc whose ends never part, which leaves no cell on any level.
const STAYS_INSIDE: u8 = 0;

/// For each arc, the highest level at which it leaves the cell of its source.
pub struct BorderLevels {
    /// One past the highest level the arc leaves a cell at, so that
    /// [`STAYS_INSIDE`] can mean an arc that leaves none. A byte holds it: a
    /// partition of more than two hundred and fifty levels is not one anybody
    /// builds, and [`PackedPartition`] caps the count well below that.
    of_edge: Vec<u8>,
}

impl BorderLevels {
    /// Works out the level of every arc of a graph.
    ///
    /// The graph and the partition have to be over the same nodes. Which graph
    /// is not asked: the arcs of a network turned around leave the same cells
    /// as the arcs it was turned around from, but they are held in a different
    /// order, so a search over the reversed graph wants its own.
    ///
    /// # Panics
    ///
    /// Panics if the graph holds a node the partition does not.
    #[must_use]
    pub fn of<G: Graph<u32>>(graph: &G, partition: &PackedPartition) -> Self {
        let mut of_edge = vec![STAYS_INSIDE; graph.number_of_edges()];
        for node in graph.node_range() {
            let word = partition.word(node);
            for edge in graph.edge_range(node) {
                let target = graph.target(edge);
                if let Some(level) = partition.highest_different_level(word, partition.word(target))
                {
                    of_edge[edge] = u8::try_from(level + 1)
                        .expect("a partition of more levels than a byte counts");
                }
            }
        }
        Self { of_edge }
    }

    /// how many arcs this was worked out over
    #[must_use]
    pub fn len(&self) -> usize {
        self.of_edge.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.of_edge.is_empty()
    }

    /// Whether this arc leaves the cell its source sits in at this level.
    ///
    /// # Panics
    ///
    /// Panics for an arc the graph does not have.
    #[must_use]
    #[inline]
    pub fn leaves_cell(&self, edge: EdgeID, level: usize) -> bool {
        // cells nest, so an arc parting at some level parts at every level
        // below it, and one comparison answers for the level asked about
        usize::from(self.of_edge[edge]) > level
    }

    /// The bytes themselves, one an arc, for writing into a store.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.of_edge
    }

    /// Takes back what [`as_bytes`](Self::as_bytes) wrote.
    ///
    /// This is what an instance does instead of working the levels out: they
    /// are settled once when the store is packed and do not change again, and
    /// working them out means walking every arc of the graph, which is the one
    /// thing a store that pages its arcs was built not to do.
    #[must_use]
    pub fn of_bytes(of_edge: Vec<u8>) -> Self {
        Self { of_edge }
    }

    /// What this takes: a byte an arc.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.of_edge.capacity()
    }

    /// The highest level at which the arc leaves a cell, and `None` for one
    /// whose ends sit in the same cell on every level.
    ///
    /// # Panics
    ///
    /// Panics for an arc the graph does not have.
    #[must_use]
    pub fn highest_of(&self, edge: EdgeID) -> Option<usize> {
        match self.of_edge[edge] {
            STAYS_INSIDE => None,
            held => Some(usize::from(held) - 1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        edge::InputEdge,
        grid_graph::{grid_directory, grid_edges},
        level_directory::LevelDirectory,
        packed_partition::PackedPartition,
        static_graph::StaticGraph,
    };

    /// What the table says has to be what asking the partition per arc says,
    /// for every arc and every level. This is the whole of the contract: the
    /// query stops asking and reads this instead.
    #[test]
    fn every_arc_says_what_the_partition_says() {
        for side in [4_usize, 8, 32] {
            let edges = grid_edges(side, true);
            let graph = StaticGraph::new(edges);
            let partition = PackedPartition::of(&grid_directory(side));
            let borders = BorderLevels::of(&graph, &partition);
            assert_eq!(borders.len(), graph.number_of_edges());

            for node in graph.node_range() {
                for edge in graph.edge_range(node) {
                    let target = graph.target(edge);
                    for level in 0..partition.levels() {
                        let by_partition =
                            partition.cell_of(node, level) != partition.cell_of(target, level);
                        assert_eq!(
                            borders.leaves_cell(edge, level),
                            by_partition,
                            "side {side}, arc {node} to {target} on level {level}"
                        );
                    }
                }
            }
        }
    }

    /// The arcs of a graph turned around leave the same cells, which is what
    /// says the backward side of a search may have one of these built over the
    /// reversed graph and get the same answers.
    #[test]
    fn an_arc_turned_around_leaves_the_same_cells() {
        let side = 16;
        let edges = grid_edges(side, true);
        let reversed = edges
            .iter()
            .map(|edge| InputEdge::new(edge.target, edge.source, edge.data))
            .collect::<Vec<_>>();
        let graph = StaticGraph::new(edges);
        let reverse = StaticGraph::new(reversed);
        let partition = PackedPartition::of(&grid_directory(side));

        let forward = BorderLevels::of(&graph, &partition);
        let backward = BorderLevels::of(&reverse, &partition);

        for node in graph.node_range() {
            for edge in graph.edge_range(node) {
                let target = graph.target(edge);
                // the same pair of nodes, found the other way round
                let back = reverse
                    .edge_range(target)
                    .find(|&back| reverse.target(back) == node)
                    .expect("the reversed graph holds the arc turned around");
                assert_eq!(forward.highest_of(edge), backward.highest_of(back));
            }
        }
    }

    /// An arc between two nodes of one cell leaves nothing, and one between
    /// cells leaves everything below where they part.
    #[test]
    fn an_arc_inside_a_cell_leaves_no_level() {
        // four nodes in a line, cut into two cells of two, joined above
        let edges = vec![
            InputEdge::new(0, 1, 3_u32),
            InputEdge::new(1, 2, 7_u32),
            InputEdge::new(2, 3, 5_u32),
        ];
        let graph = StaticGraph::new(edges);
        let directory = LevelDirectory::new(vec![0, 0, 1, 1], vec![vec![0, 0]]);
        let partition = PackedPartition::of(&directory);
        let borders = BorderLevels::of(&graph, &partition);

        // 0 to 1 stays inside cell 0, and 2 to 3 inside cell 1
        assert_eq!(borders.highest_of(graph.edge_range(0).start), None);
        assert_eq!(borders.highest_of(graph.edge_range(2).start), None);
        // 1 to 2 crosses on the finest level, and the two cells share the one
        // above, so that is as high as it goes
        assert_eq!(borders.highest_of(graph.edge_range(1).start), Some(0));
        assert!(borders.leaves_cell(graph.edge_range(1).start, 0));
        assert!(!borders.leaves_cell(graph.edge_range(1).start, 1));
    }
}
