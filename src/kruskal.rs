use crate::{edge::SimpleEdge, union_find::UnionFind};
use core::cmp::{max, Reverse};
use std::collections::BinaryHeap;

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

    while mst.len() < number_of_nodes {
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

        let (cost, _mst) = kruskal(&edges);
        assert_eq!(cost, 39);

        // TODO(dluxen): check for expected edges in set
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

        let (cost, _mst) = kruskal(&edges);
        assert_eq!(cost, 37);

        // TODO(dluxen): check for expected edges in set
    }
}
