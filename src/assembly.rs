//! Assembling the cells of a partition into levels of a wanted size.
//!
//! A recursive bisection that stops once a cell is small enough leaves cells
//! far finer than any level one wants to route on. Building the levels one
//! actually asked for, say of 50, 250 and 1000 nodes, means merging those cells
//! until each is as large as the level wants, and doing it so that a cell of
//! the result still holds together.
//!
//! That last part is why this is not simply a matter of counting: a search that
//! crosses a cell has to be able to do it without leaving the cell, and a cell
//! that falls into pieces cannot promise that.

use crate::{edge::TrivialEdge, level_directory::CellId};

/// Splits the cells of a partition into the pieces they consist of, so that
/// every piece is in one piece.
///
/// A cell that a bisection leaves behind need not hold together: a minimum cut
/// puts everything the source cannot reach on the other side, whether it hangs
/// together with the rest or not. Merging such a cell into a larger one carries
/// the split upwards, so the pieces have to be taken apart before anything is
/// built on top of them.
///
/// Two nodes end up in the same piece exactly when an arc of their own cell
/// joins them, directly or through other nodes of it. The arcs are walked
/// whichever way round they run, as a cell is crossed by a path that may take
/// either.
///
/// # Panics
///
/// Panics if `cell_of_node` does not hold a cell for every node, if an arc
/// reaches past the nodes, or if the graph holds more nodes than a [`CellId`]
/// can number, as every node can end up a piece of its own.
#[must_use]
pub fn fragments(nodes: usize, arcs: &[TrivialEdge], cell_of_node: &[CellId]) -> Vec<CellId> {
    assert_eq!(
        cell_of_node.len(),
        nodes,
        "the partition does not cover the graph"
    );
    assert!(
        nodes <= CellId::MAX as usize,
        "more nodes than pieces can be numbered"
    );

    let mut union = crate::union_find::UnionFind::new(nodes);
    for arc in arcs {
        if cell_of_node[arc.source] == cell_of_node[arc.target] {
            union.union(arc.source, arc.target);
        }
    }

    // number the pieces in the order their nodes come
    let mut piece_of_root = vec![CellId::MAX; nodes];
    let mut pieces: CellId = 0;
    (0..nodes)
        .map(|node| {
            let root = union.find(node);
            if piece_of_root[root] == CellId::MAX {
                piece_of_root[root] = pieces;
                pieces += 1;
            }
            piece_of_root[root]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edge(source: usize, target: usize) -> TrivialEdge {
        TrivialEdge { source, target }
    }

    #[test]
    #[should_panic(expected = "the partition does not cover the graph")]
    fn a_partition_that_leaves_a_node_out_is_caught() {
        let _ = fragments(3, &[edge(0, 1)], &[0, 0]);
    }

    #[test]
    fn a_cell_in_two_pieces_is_taken_apart() {
        // one cell of four nodes, but only 0-1 and 2-3 are joined
        let arcs = [edge(0, 1), edge(1, 0), edge(2, 3), edge(3, 2)];
        let pieces = fragments(4, &arcs, &[0, 0, 0, 0]);
        assert_eq!(pieces[0], pieces[1]);
        assert_eq!(pieces[2], pieces[3]);
        assert_ne!(pieces[0], pieces[2], "the two halves are not joined");
    }

    #[test]
    fn an_arc_leaving_a_cell_does_not_join_its_pieces() {
        // 0 and 2 are joined by an arc, but they sit in different cells
        let arcs = [edge(0, 2), edge(2, 0), edge(1, 3), edge(3, 1)];
        let pieces = fragments(4, &arcs, &[0, 1, 0, 1]);
        assert_eq!(pieces[0], pieces[2]);
        assert_eq!(pieces[1], pieces[3]);
        assert_ne!(pieces[0], pieces[1]);
    }

    #[test]
    fn a_node_no_arc_of_its_cell_reaches_is_a_piece_of_its_own() {
        let arcs = [edge(0, 1), edge(1, 0)];
        let pieces = fragments(3, &arcs, &[0, 0, 0]);
        assert_eq!(pieces[0], pieces[1]);
        assert_ne!(pieces[2], pieces[0]);
    }

    #[test]
    fn a_cell_that_holds_together_is_left_whole() {
        let arcs = [edge(0, 1), edge(1, 0), edge(1, 2), edge(2, 1)];
        let pieces = fragments(3, &arcs, &[0, 0, 0]);
        assert_eq!(pieces, vec![0, 0, 0]);
    }

    #[test]
    fn a_one_way_arc_joins_a_cell_all_the_same() {
        // 0 -> 1 and nothing back, which still leaves the cell in one piece as
        // far as the assembly is concerned
        let arcs = [edge(0, 1)];
        let pieces = fragments(2, &arcs, &[0, 0]);
        assert_eq!(pieces, vec![0, 0]);
    }

    #[test]
    fn the_pieces_are_numbered_from_zero_in_the_order_of_their_nodes() {
        // three cells, the middle one in two pieces
        let arcs = [edge(0, 1), edge(3, 4)];
        let pieces = fragments(5, &arcs, &[0, 0, 1, 1, 1]);
        assert_eq!(pieces, vec![0, 0, 1, 2, 2]);
    }
}
