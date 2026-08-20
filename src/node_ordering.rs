//! A numbering of the nodes that puts the ones a query touches at the front.
//!
//! # Why the numbers matter
//!
//! Everything a search keeps about a node it keeps in an array at that node's
//! number: what the queue holds it at, where it sits in the queue, which cells
//! it lies in. On a continent each of those arrays is tens or hundreds of
//! megabytes, and the numbers a graph arrives with say nothing about which of
//! them a query will read. A search over the overlay touches the border nodes
//! of coarse cells, a few hundred thousand of eighteen million, and reads them
//! scattered the whole width of every array. Nearly every read is a miss.
//!
//! Numbered so that those nodes come first, the same reads fall in the first
//! few megabytes of the same arrays and the arrays are otherwise untouched.
//! Nothing about the search changes; the misses do.
//!
//! # The order
//!
//! This is OSRM's `makePermutation`, which sorts twice.
//!
//! First by the cells a node lies in, coarsest cell first and finer cells
//! breaking the tie, which lays the nodes of a cell side by side and the cells
//! of a coarser cell side by side. [`PackedPartition`] holds exactly that key
//! already: it packs the coarse levels into the high bits, so the order it
//! wants is the order of the words themselves and the sort is one pass rather
//! than one pass per level.
//!
//! Then, stably, by the highest level at which a node lies on a border, that
//! level first. A node on the border of a coarse cell is one the overlay walks
//! and gets a low number; a node no cell has a border at is one only a search
//! near its own end ever reaches, and goes to the back. The sort being stable,
//! the cells stay laid out side by side within each group.

use std::cmp::Reverse;

use rkyv::{Archive, Deserialize, Serialize};

use crate::{
    edge::InputEdge,
    graph::{Graph, NodeID},
    level_directory::{CellId, LevelDirectory},
    packed_partition::PackedPartition,
};

/// Where each node goes, and what came from where.
///
/// Written out beside the graph and the directory it was worked out for, so
/// that whoever asks a question of them can put a node of the input into the
/// numbering and read an answer back out of it.
#[derive(Clone, Debug, Default, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub struct NodeOrdering {
    /// the number each node of the input now has
    to_new: Vec<u32>,
    /// the node of the input each number now holds
    to_old: Vec<u32>,
    /// how many nodes lie on the border of a cell of some level, which is how
    /// much of the front of the numbering a search over the overlay reads
    on_a_border: usize,
}

impl NodeOrdering {
    /// Works out the numbering of a graph cut into the given cells.
    ///
    /// # Panics
    ///
    /// Panics if the graph and the partition are not over the same nodes.
    #[must_use]
    pub fn of<G: Graph<u32>>(graph: &G, partition: &PackedPartition) -> Self {
        let nodes = partition.number_of_nodes();
        assert_eq!(
            graph.number_of_nodes(),
            nodes,
            "the partition was built over another graph"
        );

        // the highest level at which each node lies on a border, and zero for
        // one that never does. One walk of the arcs answers for both ends of
        // every arc, so the graph is not wanted the other way round.
        let mut border_level = vec![0_u8; nodes];
        for node in graph.node_range() {
            let word = partition.word(node);
            for edge in graph.edge_range(node) {
                let target = graph.target(edge);
                if let Some(level) = partition.highest_different_level(word, partition.word(target))
                {
                    let level = u8::try_from(level + 1)
                        .expect("a partition of more levels than a byte counts");
                    border_level[node] = border_level[node].max(level);
                    border_level[target] = border_level[target].max(level);
                }
            }
        }
        let on_a_border = border_level.iter().filter(|&&level| level > 0).count();

        let mut to_old =
            (0..u32::try_from(nodes).expect("the graph is too large to hold")).collect::<Vec<_>>();
        // the word packs the coarse levels high, so its own order is the order
        // wanted: by coarsest cell, then by the cells inside it
        to_old.sort_unstable_by_key(|&node| partition.word(node as usize));
        // and then the borders to the front, the coarsest first. Stable, so
        // the cells stay side by side inside each group.
        to_old.sort_by_key(|&node| Reverse(border_level[node as usize]));

        let mut to_new = vec![0_u32; nodes];
        for (place, &node) in to_old.iter().enumerate() {
            to_new[node as usize] = u32::try_from(place).expect("the graph is too large to hold");
        }

        Self {
            to_new,
            to_old,
            on_a_border,
        }
    }

    /// Reads back a numbering from the places it gave, which is what a run
    /// over an instance somebody else numbered has to work from.
    ///
    /// # Panics
    ///
    /// Panics unless the places are a numbering: each node once and no gaps.
    #[must_use]
    pub fn from_places(to_new: Vec<u32>, on_a_border: usize) -> Self {
        let mut to_old = vec![u32::MAX; to_new.len()];
        for (node, &place) in to_new.iter().enumerate() {
            let held = &mut to_old[place as usize];
            assert_eq!(*held, u32::MAX, "two nodes were given the number {place}");
            *held = u32::try_from(node).expect("the graph is too large to hold");
        }
        assert!(
            to_old.iter().all(|&node| node != u32::MAX),
            "the places are not a numbering"
        );
        Self {
            to_new,
            to_old,
            on_a_border,
        }
    }

    /// the place each node of the input was given
    #[must_use]
    pub fn places(&self) -> &[u32] {
        &self.to_new
    }

    /// how many nodes the numbering was worked out over
    #[must_use]
    pub fn len(&self) -> usize {
        self.to_new.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.to_new.is_empty()
    }

    /// How many nodes lie on the border of a cell of some level.
    ///
    /// These are the nodes the numbering puts first, and a search over the
    /// overlay reads no others, so this is how much of each array over the
    /// nodes such a search touches.
    #[must_use]
    pub const fn on_a_border(&self) -> usize {
        self.on_a_border
    }

    /// The number a node of the input now has.
    ///
    /// # Panics
    ///
    /// Panics for a node the numbering was not worked out over.
    #[must_use]
    #[inline]
    pub fn new_of(&self, old: NodeID) -> NodeID {
        self.to_new[old] as NodeID
    }

    /// The node of the input a number now holds.
    ///
    /// # Panics
    ///
    /// Panics for a number the numbering does not have.
    #[must_use]
    #[inline]
    pub fn old_of(&self, new: NodeID) -> NodeID {
        self.to_old[new] as NodeID
    }

    /// The same arcs, between the numbers the nodes now have.
    #[must_use]
    pub fn renumber(&self, edges: &[InputEdge<u32>]) -> Vec<InputEdge<u32>> {
        edges
            .iter()
            .map(|edge| {
                InputEdge::new(
                    self.new_of(edge.source),
                    self.new_of(edge.target),
                    edge.data,
                )
            })
            .collect()
    }

    /// The same partition, over the numbers the nodes now have.
    ///
    /// Only which cell each node lies in moves. The cells themselves are not
    /// renumbered, so what lies inside what is untouched.
    #[must_use]
    pub fn renumber_directory(&self, directory: &LevelDirectory) -> LevelDirectory {
        let mut base = vec![0 as CellId; self.len()];
        for old in 0..self.len() {
            base[self.new_of(old)] = directory.cell_of(old, 0);
        }
        let parents = (0..directory.levels().saturating_sub(1))
            .map(|level| directory.parents_on_level(level).to_vec())
            .collect();
        LevelDirectory::new(base, parents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        grid_graph::{grid_directory, grid_edges},
        static_graph::StaticGraph,
    };

    fn ordering_of(side: usize) -> (NodeOrdering, LevelDirectory, StaticGraph<u32>) {
        let edges = grid_edges(side, true);
        let graph = StaticGraph::new(edges);
        let directory = grid_directory(side);
        let partition = PackedPartition::of(&directory);
        let ordering = NodeOrdering::of(&graph, &partition);
        (ordering, directory, graph)
    }

    /// A numbering that lost or doubled a node would quietly corrupt every
    /// array built on it.
    #[test]
    fn every_node_gets_one_number_and_keeps_it() {
        for side in [4_usize, 8, 32] {
            let (ordering, _, _) = ordering_of(side);
            assert_eq!(ordering.len(), side * side);

            let mut seen = vec![false; ordering.len()];
            for old in 0..ordering.len() {
                let new = ordering.new_of(old);
                assert!(!seen[new], "side {side}: two nodes were given number {new}");
                seen[new] = true;
                assert_eq!(
                    ordering.old_of(new),
                    old as NodeID,
                    "side {side}: node {old}"
                );
            }
            assert!(seen.into_iter().all(|held| held), "side {side}");
        }
    }

    /// The whole point: a node on the border of a coarse cell comes before one
    /// on the border of a finer cell, and both before one on no border at all.
    #[test]
    fn the_coarsest_borders_come_first() {
        let side = 32;
        let (ordering, directory, graph) = ordering_of(side);
        let partition = PackedPartition::of(&directory);

        // worked out here rather than asked of the ordering, so this checks
        // the order rather than the code that produced it
        let mut border_level = vec![0_usize; ordering.len()];
        for node in graph.node_range() {
            for edge in graph.edge_range(node) {
                let target = graph.target(edge);
                if let Some(level) =
                    partition.highest_different_level(partition.word(node), partition.word(target))
                {
                    border_level[node] = border_level[node].max(level + 1);
                    border_level[target] = border_level[target].max(level + 1);
                }
            }
        }

        let levels_in_order = (0..ordering.len())
            .map(|place| border_level[ordering.old_of(place)])
            .collect::<Vec<_>>();
        assert!(
            levels_in_order.windows(2).all(|pair| pair[0] >= pair[1]),
            "the numbering does not run from the coarsest border down"
        );
        assert_eq!(
            ordering.on_a_border(),
            border_level.iter().filter(|&&level| level > 0).count()
        );
        // and the nodes a search over the overlay reads really are a small
        // part of the whole
        assert!(ordering.on_a_border() < ordering.len());
    }

    /// The renumbered graph and partition have to describe the same network,
    /// which is what says a query may be run on them and believed.
    #[test]
    fn the_renumbered_graph_holds_the_same_arcs() {
        let side = 16;
        let edges = grid_edges(side, true);
        let graph = StaticGraph::new(edges.clone());
        let directory = grid_directory(side);
        let partition = PackedPartition::of(&directory);
        let ordering = NodeOrdering::of(&graph, &partition);

        let moved = StaticGraph::new(ordering.renumber(&edges));
        assert_eq!(moved.number_of_nodes(), graph.number_of_nodes());
        assert_eq!(moved.number_of_edges(), graph.number_of_edges());

        for node in graph.node_range() {
            let mut before = graph
                .edge_range(node)
                .map(|edge| (ordering.new_of(graph.target(edge)), *graph.data(edge)))
                .collect::<Vec<_>>();
            let mut after = moved
                .edge_range(ordering.new_of(node))
                .map(|edge| (moved.target(edge), *moved.data(edge)))
                .collect::<Vec<_>>();
            before.sort_unstable();
            after.sort_unstable();
            assert_eq!(before, after, "node {node}");
        }
    }

    /// A node has to lie in the same cells after the move as before it, or the
    /// partition no longer describes the graph.
    #[test]
    fn the_renumbered_partition_holds_the_same_cells() {
        for side in [8_usize, 32] {
            let (ordering, directory, _) = ordering_of(side);
            let moved = ordering.renumber_directory(&directory);

            assert_eq!(moved.levels(), directory.levels());
            assert_eq!(moved.number_of_nodes(), directory.number_of_nodes());
            for old in 0..directory.number_of_nodes() {
                for level in 0..directory.levels() {
                    assert_eq!(
                        moved.cell_of(ordering.new_of(old), level),
                        directory.cell_of(old, level),
                        "side {side}, node {old}, level {level}"
                    );
                }
            }
        }
    }

    /// The nodes of a cell end up side by side, which is the other half of the
    /// order and what makes a cell a run of memory rather than a scattering.
    #[test]
    fn the_nodes_of_a_coarse_cell_lie_together() {
        let side = 32;
        let (ordering, directory, _) = ordering_of(side);
        let moved = ordering.renumber_directory(&directory);
        let top = directory.levels() - 1;

        // within the nodes that lie on no border at all, which the second sort
        // leaves in the order the first one put them, a cell is one run
        let mut runs = 0;
        let mut last = None;
        for place in ordering.on_a_border()..ordering.len() {
            let cell = moved.cell_of(place, top);
            if Some(cell) != last {
                runs += 1;
                last = Some(cell);
            }
        }
        let cells = directory.cells_on_level(top);
        assert!(
            runs <= cells,
            "the cells of the top level came apart into {runs} runs of {cells} cells"
        );
    }
}
