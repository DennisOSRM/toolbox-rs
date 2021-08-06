use std::{cmp::max, mem::swap, ops::Range};

use crate::graph::{EdgeID, Graph, NodeID};

#[derive(Clone, Debug, Eq, PartialOrd, Ord, PartialEq)]
pub struct InputEdge<EdgeDataT: Eq> {
    pub source: NodeID,
    pub target: NodeID,
    pub data: EdgeDataT,
}

impl<EdgeDataT: Eq> InputEdge<EdgeDataT> {
    pub fn new(s: NodeID, t: NodeID, d: EdgeDataT) -> Self {
        Self {
            source: s,
            target: t,
            data: d,
        }
    }

    pub fn reverse(&mut self) {
        swap(&mut self.source, &mut self.target);
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

        // add sentinel at the end of the node array
        graph
            .node_array
            .push(NodeArrayEntry::new((input.len()) as EdgeID));

        graph.edge_array = input
            .iter()
            .map(|edge| EdgeArrayEntry {
                target: edge.target,
                data: edge.data,
            })
            .collect();
        debug_assert!(graph.check_integrity());
        graph
    }

    // In time O(V+E) check that the following invariants hold:
    // a) the target node of each edge is smaller than the number of nodes
    // b) index values for nodes first_edges are strictly increasing
    pub fn check_integrity(&self) -> bool {
        self.edge_array
            .iter()
            .all(|edge_entry| (edge_entry.target as usize) < self.number_of_nodes())
            && self
                .node_array
                .windows(2)
                .all(|pair| pair[0].first_edge <= pair[1].first_edge)
    }
}

impl<T: Ord + Copy> Graph<T> for StaticGraph<T> {
    fn node_range(&self) -> Range<NodeID> {
        Range {
            start: 0,
            end: self.number_of_nodes() as NodeID,
        }
    }

    fn edge_range(&self, n: NodeID) -> Range<EdgeID> {
        Range {
            start: self.begin_edges(n),
            end: self.end_edges(n),
        }
    }

    fn number_of_nodes(&self) -> usize {
        self.node_array.len() - 1
    }

    fn number_of_edges(&self) -> usize {
        self.edge_array.len()
    }

    fn begin_edges(&self, n: NodeID) -> EdgeID {
        self.node_array[n as usize].first_edge
    }

    fn end_edges(&self, n: NodeID) -> EdgeID {
        self.node_array[(n + 1) as usize].first_edge
    }

    fn get_out_degree(&self, n: NodeID) -> usize {
        let up = self.end_edges(n);
        let down = self.begin_edges(n);
        (up - down) as usize
    }

    fn target(&self, e: EdgeID) -> NodeID {
        self.edge_array[e as usize].target
    }

    fn data(&self, e: EdgeID) -> &T {
        &self.edge_array[e as usize].data
    }

    fn data_mut(&mut self, e: EdgeID) -> &mut T {
        &mut self.edge_array[e as usize].data
    }

    fn find_edge(&self, s: NodeID, t: NodeID) -> Option<EdgeID> {
        for edge in self.edge_range(s) {
            if self.target(edge) == t {
                return Some(edge);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        graph::Graph,
        static_graph::{InputEdge, StaticGraph},
    };

    #[test]
    fn size() {
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
    fn degree() {
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
        let mut sum = 0;
        for i in graph.node_range() {
            sum += graph.get_out_degree(i);
        }
        assert_eq!(sum, graph.number_of_edges());
    }
}
