/// Implementation of Kruskal's Minimum Spanning Tree algorithm.
///
/// This module provides an implementation of Kruskal's algorithm for finding
/// the minimum spanning tree (MST) of an undirected weighted graph.
use crate::{edge::SimpleEdge, union_find::UnionFind};
use core::cmp::{Reverse, max};
use std::collections::BinaryHeap;

/// Computes the minimum spanning tree using Kruskal's algorithm.
///
/// # Arguments
///
/// * `input_edges` - A slice of `SimpleEdge`s representing the graph's edge
///  Each edge contains source and target vertices and a weight.
///
/// # Returns
///
/// Returns a tuple containing:
/// * The total cost of the minimum spanning tree
/// * A vector of edges that form the minimum spanning tree
///
/// # Example
///
/// ```
/// use toolbox_rs::edge::SimpleEdge;
/// use toolbox_rs::kruskal::kruskal;
///
/// let edges = vec![
///     SimpleEdge::new(0, 1, 7),
///     SimpleEdge::new(0, 3, 5),
///     SimpleEdge::new(1, 2, 8)
/// ];
///
/// let (cost, mst) = kruskal(&edges);
/// ```
pub fn kruskal(input_edges: &[SimpleEdge]) -> (u32, Vec<SimpleEdge>) {
    // find max node id
    let mut number_of_nodes = 0;
    let mut heap = BinaryHeap::new();
    for edge in input_edges {
        number_of_nodes = max(edge.source, number_of_nodes);
        number_of_nodes = max(edge.target, number_of_nodes);
        heap.push((Reverse(edge.data), heap.len()));
    }

    let mut mst = Vec::new();
    let mut uf = UnionFind::new(number_of_nodes + 1);
    let mut mst_cost = 0;

    while mst.len() < number_of_nodes && !heap.is_empty() {
        // pop the smallest edge
        // we use the index to avoid having to sort the edges
        // in the heap
        // this is a bit of a hack, but it works
        // and is faster than sorting the edges
        // in the heap
        let (_, idx) = heap.pop().unwrap();
        let edge = input_edges[idx];
        let x = uf.find(edge.source);
        let y = uf.find(edge.target);

        if x == y {
            continue;
        }

        mst.push(edge);
        uf.union(x, y);
        mst_cost += edge.data;
    }
    (mst_cost, mst)
}

#[cfg(test)]
mod tests {
    use crate::{edge::SimpleEdge, kruskal::kruskal};

    #[test]
    fn wiki_example() {
        let edges = vec![
            SimpleEdge::new(0, 1, 7),
            SimpleEdge::new(0, 3, 5),
            SimpleEdge::new(1, 3, 9),
            SimpleEdge::new(1, 2, 8),
            SimpleEdge::new(1, 4, 7),
            SimpleEdge::new(2, 4, 5),
            SimpleEdge::new(3, 4, 15),
            SimpleEdge::new(3, 5, 6),
            SimpleEdge::new(5, 4, 8),
            SimpleEdge::new(6, 4, 9),
            SimpleEdge::new(5, 6, 11),
        ];

        let (cost, mst) = kruskal(&edges);
        assert_eq!(cost, 39);

        // Verify the expected edges in the MST
        let expected_edges: Vec<(usize, usize)> =
            vec![(0, 3), (2, 4), (3, 5), (0, 1), (1, 4), (4, 6)];

        assert_eq!(mst.len(), expected_edges.len());
        for (src, tgt) in expected_edges {
            assert!(
                mst.iter().any(|e| (e.source == src && e.target == tgt)
                    || (e.source == tgt && e.target == src))
            );
        }
    }

    #[test]
    fn clr_example() {
        let edges = vec![
            SimpleEdge::new(0, 1, 16),
            SimpleEdge::new(0, 2, 13),
            SimpleEdge::new(1, 2, 10),
            SimpleEdge::new(1, 3, 12),
            SimpleEdge::new(2, 1, 4),
            SimpleEdge::new(2, 4, 14),
            SimpleEdge::new(3, 2, 9),
            SimpleEdge::new(3, 5, 20),
            SimpleEdge::new(4, 3, 7),
            SimpleEdge::new(4, 5, 4),
        ];

        let (cost, mst) = kruskal(&edges);
        assert_eq!(cost, 37);

        // Verify the expected edges in the MST
        let expected_edges: Vec<(usize, usize)> = vec![(2, 1), (4, 5), (4, 3), (0, 2), (2, 3)];

        // Check if the MST contains the expected edges
        // Note: The order of edges in the MST may vary, so we check for presence
        // rather than exact order

        assert_eq!(mst.len(), expected_edges.len());
        for (src, tgt) in expected_edges {
            assert!(
                mst.iter().any(|e| (e.source == src && e.target == tgt)
                    || (e.source == tgt && e.target == src))
            );
        }
    }

    #[test]
    fn empty_graph() {
        let edges = vec![];
        let (cost, mst) = kruskal(&edges);
        assert_eq!(cost, 0);
        assert!(mst.is_empty());
    }

    #[test]
    fn single_edge() {
        let edges = vec![SimpleEdge::new(0, 1, 5)];
        let (cost, mst) = kruskal(&edges);
        assert_eq!(cost, 5);
        assert_eq!(mst.len(), 1);
        assert_eq!(mst[0].data, 5);
    }

    #[test]
    fn disconnected_graph() {
        let edges = vec![
            SimpleEdge::new(0, 1, 1),
            SimpleEdge::new(2, 3, 2),
            // No edges between components (0,1) and (2,3)
        ];
        let (cost, mst) = kruskal(&edges);
        assert_eq!(cost, 3);
        assert_eq!(mst.len(), 2);
    }
}
