pub mod binary_heap;

use std::cmp::max;

pub type NodeID = u32;
pub type EdgeID = u32;

#[derive(Eq, PartialOrd, Ord, PartialEq)]
pub struct InputEdge<EdgeDataT: Eq> {
    source: NodeID,
    target: NodeID,
    edge_data: EdgeDataT,
}

impl<EdgeDataT: Eq> InputEdge<EdgeDataT> {
    pub fn new(s: NodeID, t: NodeID, d: EdgeDataT) -> Self {
        Self {
            source: s,
            target: t,
            edge_data: d,
        }
    }
}

pub struct NodeArrayEntry {
    first_edge: EdgeID,
}

impl NodeArrayEntry {
    pub fn new(e: EdgeID) -> NodeArrayEntry {
        NodeArrayEntry { first_edge: e }
    }
}

pub struct EdgeArrayEntry<EdgeDataT> {
    target: NodeID,
    data: EdgeDataT,
}

pub struct StaticGraph<T: Ord> {
    node_array: Vec<NodeArrayEntry>,
    edge_array: Vec<EdgeArrayEntry<T>>,
}

impl<T: Ord + Copy> StaticGraph<T> {
    pub const INVALID_NODE_ID: NodeID = NodeID::MAX;
    pub const INVALID_EDGE_ID: EdgeID = EdgeID::MAX;

    pub fn default() -> Self {
        Self {
            node_array: Vec::new(),
            edge_array: Vec::new(),
        }
    }
    pub fn new(mut input: Vec<InputEdge<T>>) -> Self {
        // TODO: renumber IDs if necessary
        let number_of_edges = input.len();
        let mut number_of_nodes = 0;
        for edge in &input {
            number_of_nodes = max(edge.source, number_of_nodes);
            number_of_nodes = max(edge.target, number_of_nodes);
        }

        let mut graph = Self::default();
        // +1 as we are going to add one sentinel node at the end
        graph.node_array.reserve(number_of_nodes as usize + 1);
        graph.edge_array.reserve(number_of_edges);

        // sort input edges by source/target/data
        // TODO(dl): sorting by source suffices to construct adjacency array
        input.sort();

        // add first entry manually, rest will be computed
        graph.node_array.push(NodeArrayEntry::new(0));
        let mut offset = 0;
        for i in 0..(number_of_nodes) {
            while offset != input.len() && input[offset].source == i {
                offset += 1;
            }
            graph.node_array.push(NodeArrayEntry::new(offset as EdgeID));
        }

        //add sentinel at the end of the node array
        graph
            .node_array
            .push(NodeArrayEntry::new((graph.node_array.len() - 1) as EdgeID));

        for edge in &input {
            graph.edge_array.push(EdgeArrayEntry {
                target: edge.target,
                data: edge.edge_data,
            });
        }
        graph
    }

    pub fn node_range(&self) -> std::ops::Range<NodeID> {
        std::ops::Range {
            start: 0,
            end: self.number_of_nodes() as NodeID,
        }
    }

    pub fn edge_range(&self, n: NodeID) -> std::ops::Range<EdgeID> {
        std::ops::Range {
            start: self.begin_edges(n),
            end: self.end_edges(n),
        }
    }

    pub fn number_of_nodes(&self) -> usize {
        // minus two because of off-by-one _and_ a sentinel at the end
        // TODO(DL): there must be a more elegant way
        self.node_array.len() - 1
    }

    pub fn number_of_edges(&self) -> usize {
        self.edge_array.len()
    }

    pub fn begin_edges(&self, n: NodeID) -> EdgeID {
        self.node_array[n as usize].first_edge
    }

    pub fn end_edges(&self, n: NodeID) -> EdgeID {
        self.node_array[(n + 1) as usize].first_edge
    }

    pub fn get_out_degree(&self, n: NodeID) -> usize {
        (self.end_edges(n) - self.begin_edges(n)) as usize
    }

    pub fn target(&self, e: EdgeID) -> NodeID {
        self.edge_array[e as usize].target
    }

    pub fn data(&self, e: EdgeID) -> &T {
        &self.edge_array[e as usize].data
    }

    /// Returns whether the graph contains a cycle by running a node
    /// coloring DFS
    pub fn cycle_check(&self) -> bool {
        #[derive(Clone, PartialEq)]
        enum Colors {
            White,
            Grey,
            Black,
        }

        let mut node_colors = Vec::new();
        node_colors.resize(self.number_of_nodes(), Colors::White);
        let mut stack = Vec::new();

        for root in self.node_range() {
            if node_colors[root as usize] != Colors::White {
                continue;
            }

            stack.push(root);
            while let Some(&node) = stack.last() {
                // pre-order traversal
                if node_colors[node as usize] != Colors::Grey {
                    node_colors[node as usize] = Colors::Grey;

                    for edge in self.edge_range(node) {
                        // push unvisited children to stack
                        let target = self.target(edge);

                        if node_colors[target as usize] == Colors::White {
                            stack.push(target);
                        } else if node_colors[target as usize] == Colors::Grey {
                            // cycle detected
                            return true;
                        }
                    }
                } else if node_colors[node as usize] == Colors::Grey {
                    // post-order traversal
                    stack.pop();
                    node_colors[node as usize] = Colors::Black;
                }
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use crate::binary_heap::BinaryHeap;
    use rand::{rngs::StdRng, Rng, SeedableRng};

    // Note this useful idiom: importing names from outer (for mod tests) scope.
    use super::*;

    #[test]
    fn size_of_graph() {
        type Graph = StaticGraph<i32>;
        let edges = vec![
            InputEdge::new(0, 1, 3),
            InputEdge::new(1, 2, 3),
            InputEdge::new(4, 2, 1),
            InputEdge::new(2, 3, 6),
            InputEdge::new(0, 4, 2),
            InputEdge::new(4, 5, 2),
            InputEdge::new(5, 3, 7),
            InputEdge::new(1, 5, 2),
        ];
        let graph = Graph::new(edges);

        assert_eq!(6, graph.number_of_nodes());
        assert_eq!(8, graph.number_of_edges());
    }

    #[test]
    fn cycle_check_no_cycle() {
        type Graph = StaticGraph<i32>;
        let edges = vec![
            InputEdge::new(0, 1, 3),
            InputEdge::new(1, 2, 3),
            InputEdge::new(4, 2, 1),
            InputEdge::new(2, 3, 6),
            InputEdge::new(0, 4, 2),
            InputEdge::new(4, 5, 2),
            InputEdge::new(5, 3, 7),
            InputEdge::new(1, 5, 2),
        ];
        let graph = Graph::new(edges);
        assert_eq!(false, graph.cycle_check());
    }

    #[test]
    fn cycle_check_cycle() {
        type Graph = StaticGraph<i32>;
        let edges = vec![
            InputEdge::new(0, 1, 3),
            InputEdge::new(2, 3, 3),
            InputEdge::new(3, 4, 1),
            InputEdge::new(4, 5, 6),
            InputEdge::new(5, 2, 2),
        ];
        let graph = Graph::new(edges);
        assert_eq!(true, graph.cycle_check());
    }

    #[test]
    fn binary_heap_empty() {
        type Heap = BinaryHeap<i32>;
        let heap = Heap::new();

        assert!(heap.is_empty());
    }

    #[test]
    fn binary_heap_insert_size() {
        type Heap = BinaryHeap<i32>;
        let mut heap = Heap::new();
        heap.insert(20);
        assert_eq!(20, *heap.min());
        assert!(!heap.is_empty());
    }

    #[test]
    fn binary_heap_sort() {
        type Heap = BinaryHeap<i32>;
        let mut heap = Heap::new();

        let mut input = vec![4, 1, 6, 7, 5];
        for i in &input {
            heap.insert(*i);
        }
        assert_eq!(1, *heap.min());
        assert!(!heap.is_empty());

        let mut result = Vec::new();
        while !heap.is_empty() {
            result.push(heap.delete_min());
        }
        assert_eq!(result.len(), 5);
        assert!(heap.is_empty());

        input.sort();
        assert_eq!(result, input);
    }

    #[test]
    fn binary_heap_sort_random() {
        type Heap = BinaryHeap<i32>;
        let mut heap = Heap::new();

        let mut rng = StdRng::seed_from_u64(0xAAaaAAaa);

        let mut input = Vec::new();
        for _ in 0..1000 {
            let number = rng.gen();
            input.push(number);
            heap.insert(number);
        }
        assert!(!heap.is_empty());
        assert_eq!(1000, heap.len());
        assert_eq!(1000, input.len());

        let mut result = Vec::new();
        while !heap.is_empty() {
            result.push(heap.delete_min());
        }
        assert_eq!(result.len(), 1000);
        assert!(heap.is_empty());

        input.sort();
        assert_eq!(result, input);
    }

    #[test]
    fn binary_heap_clear() {
        type Heap = BinaryHeap<i32>;
        let mut heap = Heap::new();

        let input = vec![4, 1, 6, 7, 5];
        for i in &input {
            heap.insert(*i);
        }
        assert_eq!(1, *heap.min());
        assert!(!heap.is_empty());
        assert_eq!(5, heap.len());

        heap.clear();
        assert_eq!(0, heap.len());
    }

    #[test]
    #[should_panic]
    fn binary_heap_empty_min_panic() {
        type Heap = BinaryHeap<i32>;
        let heap = Heap::new();
        heap.min();
    }
}
