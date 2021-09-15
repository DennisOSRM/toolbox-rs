use crate::graph::NodeID;
use crate::union_find::UnionFind;
use core::cmp::max;
use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

#[derive(Clone, Copy, Debug)]
pub struct KruskalEdge {
    source: NodeID,
    target: NodeID,
    weight: u32,
}

impl KruskalEdge {
    pub fn new(source: NodeID, target: NodeID, weight: u32) -> Self {
        Self {
            source,
            target,
            weight,
        }
    }
}

impl Eq for KruskalEdge {}
impl PartialEq for KruskalEdge {
    fn eq(&self, other: &Self) -> bool {
        self.weight == other.weight
    }
}
impl PartialOrd for KruskalEdge {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other)) // Delegate to the implementation in `Ord`.
    }
}
impl Ord for KruskalEdge {
    fn cmp(&self, other: &Self) -> Ordering {
        Reverse(self.weight).cmp(&Reverse(other.weight))
    }
}

pub fn kruskal(input_edges: &Vec<KruskalEdge>) -> (u32, Vec<KruskalEdge>) {
    // find max node id
    let mut number_of_nodes = 0;
    for edge in input_edges {
        number_of_nodes = max(edge.source, number_of_nodes);
        number_of_nodes = max(edge.target, number_of_nodes);
    }
    // create heap
    let mut heap = BinaryHeap::new();
    for edge in input_edges {
        heap.push(*edge);
    }

    let mut mst = Vec::new();
    let mut uf = UnionFind::new(number_of_nodes + 1);
    let mut mst_cost = 0;

    while mst.len() < number_of_nodes as usize {
        let edge = heap.pop().unwrap();
        println!("min edge: {:?}", edge);
        let x = uf.find(edge.source);
        let y = uf.find(edge.target);

        println!("{:?} in {}, {}", edge, x, y);
        if x == y {
            continue;
        }

        mst.push(edge);
        uf.union(x, y);
        mst_cost += edge.weight;
    }
    (mst_cost, mst)
}

#[cfg(test)]
mod tests {
    use crate::kruskal::{kruskal, KruskalEdge};

    #[test]
    fn wiki_example() {
        let edges = vec![
            KruskalEdge::new(0, 1, 7),
            KruskalEdge::new(0, 3, 5),
            KruskalEdge::new(1, 3, 9),
            KruskalEdge::new(1, 2, 8),
            KruskalEdge::new(1, 4, 7),
            KruskalEdge::new(2, 4, 5),
            KruskalEdge::new(3, 4, 13),
            KruskalEdge::new(3, 5, 6),
            KruskalEdge::new(5, 4, 8),
            KruskalEdge::new(6, 4, 9),
            KruskalEdge::new(5, 6, 11),
        ];

        let (cost, _mst) = kruskal(&edges);
        assert_eq!(cost, 39);
    }
}
