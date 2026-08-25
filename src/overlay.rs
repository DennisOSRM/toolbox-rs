//! What a search over the cells needs, and nothing about where it is kept.
//!
//! # Why a trait and not a type
//!
//! The same search has to run in two settings that agree on the algorithm and
//! on nothing else.
//!
//! On a server the whole overlay is in memory. A table is worked out once and
//! kept, and handing it to a search is a load: the table is lent out for as
//! long as anyone wants it and nothing moves underneath.
//!
//! On a device it is on disk. A table is in a block that may not be
//! decompressed yet, and holding one has to keep it from being evicted while
//! the search reads it. That is a guard, not a reference, and it has a
//! lifetime the store decides rather than the caller.
//!
//! Those two are the same shape once the difference is named: a store hands
//! out something that reads like a table, for as long as the caller holds it.
//! [`Overlay::Table`] is that something, and everything else a search asks for
//! is the same in both settings.
//!
//! # What is deliberately not here
//!
//! Anything that builds. A store that reads a packed file cannot customize a
//! cell and should not be asked to; a customization that works cells out on
//! demand does so behind [`Overlay::distances_of`] and the search cannot tell.
//! What the search wants is the answer, and both can give it.

use crate::{
    border_levels::Borders,
    graph::{Arcs, NodeID},
    level_directory::CellId,
    packed_partition::PackedPartition,
};

/// The distances between the border nodes of one cell, however they are held.
///
/// A search reads a row of this once for every arc it takes across a cell, so
/// everything here is a slice rather than an entry at a time: the row and the
/// nodes it is about are walked in step, as two runs of memory.
pub trait CellTable {
    /// The nodes on the border of the cell, in the order the table is in.
    fn border_nodes(&self) -> &[u32];

    /// What it costs to get from the border node in `source` to each of them.
    fn row(&self, source: usize) -> &[u32];

    /// What it costs to reach the border node in `target` from each of them,
    /// which is a column of the table and is held as a row of the transpose.
    fn column(&self, target: usize) -> &[u32];

    /// Where a node sits in the table, and `None` for a node of the cell that
    /// is not on its border.
    fn place_of(&self, node: NodeID) -> Option<usize>;
}

/// Whatever holds the cells a search runs over.
pub trait Overlay {
    /// the arcs of the graph itself, which a search walks near its ends
    type Graph: Arcs<u32>;

    /// What [`distances_of`](Self::distances_of) hands out.
    ///
    /// A reference where the tables are in memory, and a guard where they are
    /// in a cache that would otherwise evict one while it is being read.
    type Table<'a>: CellTable
    where
        Self: 'a;

    fn graph(&self) -> &Self::Graph;

    /// The cell each node lies in on every level, one word apiece.
    fn partition(&self) -> &PackedPartition;

    /// The level at which each arc leaves a cell, which is how a search knows
    /// an arc is worth taking without asking the partition about both ends.
    /// What says whether an arc leaves its source's cell: a byte an arc where
    /// the instance keeps one, and the graph itself where the arcs carry it.
    type Borders: Borders;

    fn borders(&self) -> &Self::Borders;

    fn levels(&self) -> usize;

    fn cells_on_level(&self, level: usize) -> usize;

    /// The distances across a cell, and `None` for a cell with no border node
    /// and so no table.
    ///
    /// Held for as long as the caller holds what comes back. A store that
    /// pages may read and decompress here; one that does not is a load.
    fn distances_of(&self, level: usize, cell: CellId) -> Option<Self::Table<'_>>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path_unpacking::{cost_of_way, unpack};
    use crate::{
        customization::{CellDistances, Customization},
        edge::InputEdge,
        grid_graph::grid_directory,
        mld_query::MldQuery,
        static_graph::StaticGraph,
    };
    use std::cell::Cell;

    /// A store shaped the way a paged one is: it hands out a guard rather than
    /// a reference, and counts what was asked of it.
    ///
    /// The tables are in memory here, which is not the point. The point is
    /// that [`Overlay::Table`] is not a reference, so a store whose tables sit
    /// in a cache that may evict them can hold one open for as long as the
    /// search reads it, and the search cannot tell the difference. If this
    /// compiles and answers the same, a disk-backed store fits the same search.
    struct Paged {
        held: Customization,
        asked: Cell<usize>,
    }

    struct Guard<'a> {
        table: &'a CellDistances,
    }

    impl CellTable for Guard<'_> {
        fn border_nodes(&self) -> &[u32] {
            self.table.border_nodes_of()
        }
        fn row(&self, source: usize) -> &[u32] {
            self.table.row(source)
        }
        fn column(&self, target: usize) -> &[u32] {
            self.table.column(target)
        }
        fn place_of(&self, node: NodeID) -> Option<usize> {
            self.table.place_of(node)
        }
    }

    impl Overlay for Paged {
        type Graph = StaticGraph<u32>;
        type Table<'a>
            = Guard<'a>
        where
            Self: 'a;

        fn graph(&self) -> &Self::Graph {
            Overlay::graph(&self.held)
        }
        fn partition(&self) -> &PackedPartition {
            Overlay::partition(&self.held)
        }
        type Borders = crate::border_levels::BorderLevels;

        fn borders(&self) -> &Self::Borders {
            Overlay::borders(&self.held)
        }
        fn levels(&self) -> usize {
            Overlay::levels(&self.held)
        }
        fn cells_on_level(&self, level: usize) -> usize {
            Overlay::cells_on_level(&self.held, level)
        }
        fn distances_of(&self, level: usize, cell: CellId) -> Option<Self::Table<'_>> {
            self.asked.set(self.asked.get() + 1);
            Overlay::distances_of(&self.held, level, cell).map(|table| Guard { table })
        }
    }

    fn grid(side: usize) -> Customization {
        let mut edges = Vec::new();
        for row in 0..side {
            for column in 0..side {
                let node = row * side + column;
                if column + 1 < side {
                    edges.push(InputEdge::new(node, node + 1, 1_u32));
                    edges.push(InputEdge::new(node + 1, node, 1_u32));
                }
                if row + 1 < side {
                    edges.push(InputEdge::new(node, node + side, 1_u32));
                    edges.push(InputEdge::new(node + side, node, 1_u32));
                }
            }
        }
        Customization::new(StaticGraph::new(edges), grid_directory(side))
    }

    /// The one that matters: the same search, unchanged, over a store that
    /// lends and one that guards, answering the same.
    #[test]
    fn the_same_search_runs_over_a_store_that_hands_out_guards() {
        let side = 16;
        let lending = grid(side);
        let guarding = Paged {
            held: grid(side),
            asked: Cell::new(0),
        };

        let mut over_lending = MldQuery::new();
        let mut over_guarding = MldQuery::new();
        let mut pairs = 0;
        for source in (0..side * side).step_by(7) {
            for target in (0..side * side).step_by(11) {
                over_lending.clear();
                over_guarding.clear();
                let reached = over_lending.run(&lending, source, &[target]);
                assert_eq!(reached, over_guarding.run(&guarding, source, &[target]));
                assert_eq!(
                    over_lending.distance(target),
                    over_guarding.distance(target),
                    "from {source} to {target}"
                );
                pairs += 1;
            }
        }
        assert!(pairs > 100, "the sweep is worth running");
        assert!(
            guarding.asked.get() > 0,
            "the guarding store was never asked for a table"
        );
    }

    /// And the same again for the way rather than the cost: unpacking asks the
    /// store for tables of its own, level by level down, and has to get the
    /// same way out of a store that guards as out of one that lends.
    #[test]
    fn the_same_way_is_unpacked_over_a_store_that_hands_out_guards() {
        let side = 16;
        let lending = grid(side);
        let guarding = Paged {
            held: grid(side),
            asked: Cell::new(0),
        };

        let mut over_lending = MldQuery::new();
        let mut over_guarding = MldQuery::new();
        let mut unpacked = 0;
        for source in (0..side * side).step_by(7) {
            for target in (0..side * side).step_by(11) {
                over_lending.clear();
                over_guarding.clear();
                over_lending.run(&lending, source, &[target]);
                over_guarding.run(&guarding, source, &[target]);

                let Some(packed) = over_lending.retrieve_packed_path(target) else {
                    continue;
                };
                assert_eq!(
                    Some(packed.clone()),
                    over_guarding.retrieve_packed_path(target),
                    "the packed path differs from {source} to {target}"
                );

                let over_one = unpack(&lending, &packed).expect("a way over the lending store");
                let over_other = unpack(&guarding, &packed).expect("a way over the guarding one");
                assert_eq!(over_one, over_other, "from {source} to {target}");

                // and the way is worth what the search said it would be
                assert_eq!(
                    cost_of_way(lending.graph(), &over_one),
                    Some(over_lending.distance(target)),
                    "from {source} to {target}"
                );
                unpacked += 1;
            }
        }
        assert!(unpacked > 100, "the sweep is worth running");
    }
}
