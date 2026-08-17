//! A square grid of nodes with a partition cut over it, for whatever needs a
//! graph with cells in it and no file to read.
//!
//! A test wants a graph small enough to work out by hand and a benchmark wants
//! one large enough to mean something, and neither can reach for a road network
//! on disk: a benchmark of this crate has nothing but the crate. A grid is what
//! is left, and it is not a bad stand-in. It has a boundary between every pair
//! of neighbouring cells, which is what a query spends its time on, and asking
//! for arcs that run one way makes the distances across a cell asymmetric, the
//! way a road network does and a symmetric toy does not.
//!
//! What it does not have is the shape of a road network: no motorways, no
//! ferries, no dead ends, and every cell of a size. A number measured here says
//! how something scales, not what it costs on a continent.
//!
//! # Examples
//!
//! ```rust
//! use toolbox_rs::grid_graph::grid;
//! use toolbox_rs::graph::Graph;
//!
//! let (graph, directory) = grid(8, true);
//! assert_eq!(graph.number_of_nodes(), 64);
//! // eight by eight, cut into squares of two, then of four, then the lot
//! assert_eq!(directory.levels(), 3);
//! assert_eq!(directory.cells_on_level(0), 16);
//! assert_eq!(directory.cells_on_level(1), 4);
//! assert_eq!(directory.cells_on_level(2), 1);
//! ```

use crate::{
    edge::InputEdge,
    graph::NodeID,
    level_directory::{CellId, LevelDirectory},
    static_graph::StaticGraph,
};

/// The node at a place on the grid, numbered along the rows.
#[must_use]
pub fn node_at(side: usize, row: usize, column: usize) -> NodeID {
    row * side + column
}

/// The arcs of a `side` by `side` grid, each of weight one.
///
/// The arcs of a column always run both ways. Those of a row run one way only
/// unless `both_ways` is asked for, which is what leaves a cell costing a
/// different amount to cross in each direction. A query that assumed otherwise
/// would be right on a grid of the other kind and wrong on a road.
#[must_use]
pub fn grid_edges(side: usize, both_ways: bool) -> Vec<InputEdge<usize>> {
    let mut edges = Vec::new();
    for row in 0..side {
        for column in 0..side {
            if column + 1 < side {
                edges.push(InputEdge::new(
                    node_at(side, row, column),
                    node_at(side, row, column + 1),
                    1,
                ));
                if both_ways {
                    edges.push(InputEdge::new(
                        node_at(side, row, column + 1),
                        node_at(side, row, column),
                        1,
                    ));
                }
            }
            if row + 1 < side {
                edges.push(InputEdge::new(
                    node_at(side, row, column),
                    node_at(side, row + 1, column),
                    1,
                ));
                edges.push(InputEdge::new(
                    node_at(side, row + 1, column),
                    node_at(side, row, column),
                    1,
                ));
            }
        }
    }
    edges
}

/// The partition of a `side` by `side` grid: squares of two by two on the
/// finest level, and each level above it squares of twice the side of the one
/// below, up to the one cell that holds the lot.
///
/// A partition of a road network has cells that grow like this, and a search
/// over it is only worth anything where there is a cell large enough to be
/// worth stepping over. Cutting to two levels and then a lid instead leaves
/// the topmost cell holding every node, so it holds the ends of every query
/// and can never be stepped over, and what is left to step over is a square of
/// sixteen nodes whose border is nearly all of it.
///
/// # Panics
///
/// Panics unless the side is a power of two of at least four, as every level
/// halves the number of cells across and a side that does not halve evenly
/// would leave a cell straddling the edge of the grid.
#[must_use]
pub fn grid_directory(side: usize) -> LevelDirectory {
    assert!(
        side >= 4 && side.is_power_of_two(),
        "a grid is cut into squares that double each level, so the side has to be a power of two"
    );

    // the finest cells are squares of two, so a node's cell is its place
    // divided by two, counted along the rows of cells
    let across = side / 2;
    let base = (0..side * side)
        .map(|index| {
            let (row, column) = (index / side, index % side);
            ((row / 2) * across + column / 2) as CellId
        })
        .collect::<Vec<_>>();

    // and each level above holds four cells of the one below, so a cell's
    // parent is its place among them divided by two, both ways
    let mut parents = Vec::new();
    let mut across = across;
    while across > 1 {
        parents.push(
            (0..across * across)
                .map(|cell| {
                    let (row, column) = (cell / across, cell % across);
                    ((row / 2) * (across / 2) + column / 2) as CellId
                })
                .collect::<Vec<_>>(),
        );
        across /= 2;
    }

    LevelDirectory::new(base, parents)
}

/// A `side` by `side` grid and the partition cut over it.
///
/// # Panics
///
/// Panics unless the side is a power of two of at least four. See
/// [`grid_directory`].
#[must_use]
pub fn grid(side: usize, both_ways: bool) -> (StaticGraph<usize>, LevelDirectory) {
    (
        StaticGraph::new(grid_edges(side, both_ways)),
        grid_directory(side),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::Graph;

    #[test]
    fn a_grid_holds_a_node_for_every_place_on_it() {
        let (graph, _) = grid(8, true);
        assert_eq!(graph.number_of_nodes(), 64);
        assert_eq!(node_at(8, 0, 0), 0);
        assert_eq!(node_at(8, 3, 5), 29);
    }

    /// A grid of arcs both ways has one for each direction of every pair of
    /// neighbours; one of arcs one way round has the columns both ways and the
    /// rows only once.
    #[test]
    fn the_rows_run_one_way_unless_both_are_asked_for() {
        let side = 8;
        let pairs = side * (side - 1);

        assert_eq!(grid_edges(side, true).len(), 2 * pairs + 2 * pairs);
        assert_eq!(grid_edges(side, false).len(), pairs + 2 * pairs);
    }

    #[test]
    fn every_node_of_a_cell_is_a_neighbour_of_another() {
        let directory = grid_directory(8);
        // the four nodes of the top left cell of the finest level are the
        // corners of a square of two by two
        for (row, column) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
            assert_eq!(directory.cell_of(node_at(8, row, column), 0), 0);
        }
        // and the node next door on the right is in the cell next door
        assert_eq!(directory.cell_of(node_at(8, 0, 2), 0), 1);
    }

    #[test]
    fn a_cell_of_a_level_is_built_of_cells_of_the_one_below() {
        let directory = grid_directory(8);
        assert_eq!(directory.levels(), 3);
        assert_eq!(directory.cells_on_level(0), 16);
        assert_eq!(directory.cells_on_level(1), 4);
        assert_eq!(directory.cells_on_level(2), 1);

        // the four finest cells of the top left quarter share a cell above
        for cell in [0, 1, 4, 5] {
            assert_eq!(directory.parents_on_level(0)[cell], 0);
        }
    }

    #[test]
    #[should_panic(expected = "power of two")]
    fn a_side_that_does_not_halve_is_refused() {
        let _ = grid_directory(12);
    }

    /// Every level holds squares of twice the side of the one below, so the
    /// count of cells falls by four each time and the top holds one.
    #[test]
    fn the_cells_double_in_side_up_to_the_one_that_holds_the_lot() {
        let directory = grid_directory(64);
        assert_eq!(directory.levels(), 6);
        for (level, cells) in [1024, 256, 64, 16, 4, 1].into_iter().enumerate() {
            assert_eq!(
                directory.cells_on_level(level),
                cells,
                "level {level} of a grid of 64"
            );
        }
    }
}
