//! The cells of a partitioned graph, level by level.
//!
//! A partition on its own says which cell each node sits in. What a caller
//! wants is the other way round as well: the nodes of each cell, which of them
//! sit on a border, and which cells of the level below a cell is built from.
//! Working that out means a walk of the whole graph, so it is done once per
//! level and kept.
//!
//! # Border nodes
//!
//! A node is on the border of its cell while an arc leaves it or reaches it
//! from outside. Both count. A road network is directed, and a node that can
//! only be entered from another cell is still a way into the cell, which a
//! path through the level above may take.

use crate::{
    graph::{Graph, NodeID},
    level_directory::{CellId, LevelDirectory},
    static_graph::StaticGraph,
};
use rustc_hash::FxHashMap;
use std::sync::{Arc, Mutex};

/// The cells of one level: which one each node sits in, which nodes each of
/// them holds, and which of those nodes sit on a border.
pub struct Level {
    pub of_node: Vec<CellId>,
    pub nodes_of_cell: Vec<Vec<NodeID>>,
    /// A node is on the border of its cell while an arc leaves it or reaches it
    /// from outside. Both count: a road network is directed, and a node that
    /// can only be entered from another cell is a way in that a path through
    /// the cell above may take.
    pub on_border: Vec<bool>,
    /// the cells of the level below that each cell of this one is built from,
    /// and empty on the finest level, which is built from the graph itself
    pub built_from: Vec<Vec<CellId>>,
}

/// The cells of a partition, worked out level by level as they are asked for.
pub struct Customization {
    graph: StaticGraph<usize>,
    directory: LevelDirectory,
    /// The cells of a level, and the nodes of each of them, worked out the
    /// first time that level is asked about. Walking the directory per node
    /// per arc would otherwise be paid on every request.
    levels: Mutex<FxHashMap<usize, Arc<Level>>>,
}

impl Customization {
    #[must_use]
    pub fn new(graph: StaticGraph<usize>, directory: LevelDirectory) -> Self {
        assert_eq!(
            graph.number_of_nodes(),
            directory.number_of_nodes(),
            "the directory was built over another graph"
        );
        Self {
            graph,
            directory,
            levels: Mutex::new(FxHashMap::default()),
        }
    }

    /// the graph the partition was built over
    pub const fn graph(&self) -> &StaticGraph<usize> {
        &self.graph
    }

    /// which cell each node sits in on each level
    pub const fn directory(&self) -> &LevelDirectory {
        &self.directory
    }

    /// The cells of a level, worked out on the first request for it and kept.
    pub fn level(&self, level: usize) -> Arc<Level> {
        if let Some(cells) = self
            .levels
            .lock()
            .expect("the level cache is poisoned")
            .get(&level)
        {
            return cells.clone();
        }

        let of_node = (0..self.directory.number_of_nodes())
            .map(|node| self.directory.cell_of(node, level))
            .collect::<Vec<_>>();
        let mut nodes_of_cell = vec![Vec::new(); self.directory.cells_on_level(level)];
        for (node, &cell) in of_node.iter().enumerate() {
            nodes_of_cell[cell as usize].push(node);
        }

        // one walk of the arcs marks both ends of every arc that leaves a cell,
        // which saves holding the arcs of the graph the other way round
        let mut on_border = vec![false; of_node.len()];
        for source in self.graph.node_range() {
            for edge in self.graph.edge_range(source) {
                let target = self.graph.target(edge);
                if of_node[source] != of_node[target] {
                    on_border[source] = true;
                    on_border[target] = true;
                }
            }
        }

        let built_from = if level == 0 {
            Vec::new()
        } else {
            let mut children = vec![Vec::new(); self.directory.cells_on_level(level)];
            for (below, &above) in self
                .directory
                .parents_on_level(level - 1)
                .iter()
                .enumerate()
            {
                children[above as usize].push(below as CellId);
            }
            children
        };

        let cells = Arc::new(Level {
            of_node,
            nodes_of_cell,
            on_border,
            built_from,
        });
        self.levels
            .lock()
            .expect("the level cache is poisoned")
            .insert(level, cells.clone());
        cells
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::edge::InputEdge;

    /// A square grid of `side` by `side` nodes, cut into squares of two by two
    /// on the finest level and of four by four above it. The arcs of a row run
    /// one way round when `both_ways` is not asked for, which is what a road
    /// network does and what makes the distances of a cell asymmetric.
    fn grid_with(side: usize, both_ways: bool) -> Customization {
        let node = |row: usize, column: usize| row * side + column;
        let mut edges = Vec::new();
        for row in 0..side {
            for column in 0..side {
                if column + 1 < side {
                    edges.push(InputEdge::new(node(row, column), node(row, column + 1), 1));
                    if both_ways {
                        edges.push(InputEdge::new(node(row, column + 1), node(row, column), 1));
                    }
                }
                if row + 1 < side {
                    edges.push(InputEdge::new(node(row, column), node(row + 1, column), 1));
                    edges.push(InputEdge::new(node(row + 1, column), node(row, column), 1));
                }
            }
        }

        let finest = (0..side * side)
            .map(|index| {
                let (row, column) = (index / side, index % side);
                ((row / 2) * (side / 2) + column / 2) as CellId
            })
            .collect::<Vec<_>>();
        let coarser = (0..(side / 2) * (side / 2))
            .map(|cell| {
                let (row, column) = (cell / (side / 2), cell % (side / 2));
                ((row / 2) * (side / 4) + column / 2) as CellId
            })
            .collect::<Vec<_>>();
        let top = vec![0; (side / 4) * (side / 4)];

        Customization::new(
            StaticGraph::new(edges),
            LevelDirectory::new(finest, vec![coarser, top]),
        )
    }

    fn grid(side: usize) -> Customization {
        grid_with(side, true)
    }

    #[test]
    fn a_border_node_is_one_an_arc_reaches_as_well_as_one_it_leaves() {
        // 0 -> 1 only, and the two sit in different cells. Node 1 can only be
        // entered from outside, and is a way into its cell all the same.
        let edges = vec![InputEdge::new(0, 1, 1_usize)];
        let directory = LevelDirectory::new(vec![0, 1], vec![vec![0, 0]]);
        let customization = Customization::new(StaticGraph::new(edges), directory);

        let cells = customization.level(0);
        assert!(cells.on_border[0], "the node the arc leaves");
        assert!(cells.on_border[1], "the node the arc reaches");
    }

    #[test]
    fn a_cell_knows_the_cells_it_is_built_from() {
        let customization = grid(8);
        let cells = customization.level(1);
        // four cells of the finest level make up one of the level above
        for children in &cells.built_from {
            assert_eq!(children.len(), 4);
        }
        // and the finest level is built from the graph rather than from cells
        assert!(customization.level(0).built_from.is_empty());
    }

    #[test]
    fn a_level_is_worked_out_once_and_kept() {
        let customization = grid(8);
        let first = customization.level(1);
        let second = customization.level(1);
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn a_cell_holds_the_nodes_the_directory_puts_in_it() {
        let customization = grid(8);
        let cells = customization.level(0);
        assert_eq!(cells.nodes_of_cell.len(), 16, "squares of two by two");
        for (cell, nodes) in cells.nodes_of_cell.iter().enumerate() {
            assert_eq!(nodes.len(), 4);
            for &node in nodes {
                assert_eq!(cells.of_node[node] as usize, cell);
            }
        }
    }

    #[test]
    #[should_panic(expected = "the directory was built over another graph")]
    fn a_directory_of_another_graph_is_caught() {
        let edges = vec![InputEdge::new(0, 1, 1_usize), InputEdge::new(1, 0, 1_usize)];
        let directory = LevelDirectory::new(vec![0, 0, 1], Vec::new());
        let _ = Customization::new(StaticGraph::new(edges), directory);
    }
}
