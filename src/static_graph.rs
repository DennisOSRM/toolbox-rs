use crate::{
    edge::{Edge, EdgeData},
    graph::{EdgeArrayEntry, EdgeID, Graph, NodeID},
};
use core::{cmp::max, ops::Range};

pub struct NodeArrayEntry {
    pub first_edge: EdgeID,
}

impl NodeArrayEntry {
    pub fn new(first_edge: EdgeID) -> NodeArrayEntry {
        NodeArrayEntry { first_edge }
    }
}
pub struct StaticGraph<T: Ord + Clone> {
    node_array: Vec<NodeArrayEntry>,
    edge_array: Vec<EdgeArrayEntry<T>>,
}

impl<T: Ord + Clone> Default for StaticGraph<T> {
    fn default() -> Self {
        Self {
            node_array: Vec::new(),
            edge_array: Vec::new(),
        }
    }
}

impl<T: Ord + Copy> StaticGraph<T> {
    // In time O(V+E) check that the following invariants hold:
    // a) the target node of each edge is smaller than the number of nodes
    // b) index values for nodes first_edges are strictly increasing
    pub fn check_integrity(&self) -> bool {
        self.edge_array
            .iter()
            .all(|edge_entry| (edge_entry.target) < self.number_of_nodes())
            && self
                .node_array
                .windows(2)
                .all(|pair| pair[0].first_edge <= pair[1].first_edge)
    }

    pub fn new(mut input: Vec<impl Edge<ID = NodeID> + EdgeData<DATA = T> + Ord>) -> Self {
        // sort input edges by source/target/data
        // TODO(dl): sorting by source suffices to construct adjacency array
        input.sort();

        Self::new_from_sorted_list(input)
    }

    pub fn new_from_sorted_list(
        input: Vec<impl Edge<ID = NodeID> + EdgeData<DATA = T> + Ord>,
    ) -> Self {
        // TODO: renumber IDs if necessary
        let number_of_edges = input.len();
        let mut number_of_nodes = 0;
        for edge in &input {
            number_of_nodes = max(edge.source(), number_of_nodes);
            number_of_nodes = max(edge.target(), number_of_nodes);
        }

        let mut graph = Self::default();
        // +1 as we are going to add one sentinel node at the end
        graph.node_array.reserve(number_of_nodes + 1);
        graph.edge_array.reserve(number_of_edges);

        // add first entry manually, rest will be computed
        graph.node_array.push(NodeArrayEntry::new(0));
        let mut offset = 0;
        for i in 0..(number_of_nodes) {
            while offset != input.len() && input[offset].source() == i {
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
            .map(move |edge| EdgeArrayEntry {
                target: edge.target(),
                data: *edge.data(),
            })
            .collect();
        debug_assert!(graph.check_integrity());
        graph
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
        self.node_array[n].first_edge
    }

    fn end_edges(&self, n: NodeID) -> EdgeID {
        self.node_array[(n + 1)].first_edge
    }

    fn out_degree(&self, n: NodeID) -> usize {
        let up = self.end_edges(n);
        let down = self.begin_edges(n);
        up - down
    }

    fn target(&self, e: EdgeID) -> NodeID {
        self.edge_array[e].target
    }

    fn data(&self, e: EdgeID) -> &T {
        &self.edge_array[e].data
    }

    fn data_mut(&mut self, e: EdgeID) -> &mut T {
        &mut self.edge_array[e].data
    }

    fn find_edge(&self, s: NodeID, t: NodeID) -> Option<EdgeID> {
        if s > self.number_of_nodes() {
            return None;
        }
        self.edge_range(s).find(|&edge| self.target(edge) == t)
    }

    fn find_edge_unchecked(&self, s: NodeID, t: NodeID) -> EdgeID {
        if s > self.number_of_nodes() {
            return EdgeID::MAX;
        }
        for edge in self.edge_range(s) {
            if self.target(edge) == t {
                return edge;
            }
        }
        EdgeID::MAX
    }
}

#[cfg(test)]
mod tests {
    use crate::edge::InputEdge;

    use crate::graph::EdgeID;
    use crate::{graph::Graph, static_graph::StaticGraph};

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
            sum += graph.out_degree(i);
        }
        assert_eq!(sum, graph.number_of_edges());
    }

    #[test]
    fn find_edge() {
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

        // existing edges
        assert!(graph.find_edge_unchecked(0, 1) != EdgeID::MAX);
        assert!(graph.find_edge_unchecked(1, 2) != EdgeID::MAX);
        assert!(graph.find_edge_unchecked(4, 2) != EdgeID::MAX);
        assert!(graph.find_edge_unchecked(2, 3) != EdgeID::MAX);
        assert!(graph.find_edge_unchecked(0, 4) != EdgeID::MAX);
        assert!(graph.find_edge_unchecked(4, 5) != EdgeID::MAX);
        assert!(graph.find_edge_unchecked(5, 3) != EdgeID::MAX);
        assert!(graph.find_edge_unchecked(1, 5) != EdgeID::MAX);
        assert!(graph.find_edge(0, 1).is_some());
        assert!(graph.find_edge(1, 2).is_some());
        assert!(graph.find_edge(4, 2).is_some());
        assert!(graph.find_edge(2, 3).is_some());
        assert!(graph.find_edge(0, 4).is_some());
        assert!(graph.find_edge(4, 5).is_some());
        assert!(graph.find_edge(5, 3).is_some());
        assert!(graph.find_edge(1, 5).is_some());

        // non-existing edge within ranges
        assert_eq!(graph.find_edge_unchecked(0, 0), EdgeID::MAX);
        assert!(graph.find_edge(0, 0).is_none());

        // non-existing edge out of range
        assert_eq!(graph.find_edge_unchecked(16, 17), EdgeID::MAX);
        assert!(graph.find_edge(16, 17).is_none());
    }
}
