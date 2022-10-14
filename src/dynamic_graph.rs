/// Text book implementation of a dynamic graph data structure based on
/// adjacency arrays. Nodes record their degree and edge slices are moved to
/// the end of the edge array if there is insufficient space  when adding an
/// edge.
use crate::{
    edge::{Edge, EdgeData},
    graph::{EdgeArrayEntry, EdgeID, Graph, NodeID, INVALID_EDGE_ID, INVALID_NODE_ID},
};
use core::ops::Range;

pub struct NodeArrayEntry {
    pub first_edge: EdgeID,
    pub edge_count: usize,
}

impl NodeArrayEntry {
    pub fn new(first_edge: EdgeID) -> NodeArrayEntry {
        NodeArrayEntry {
            first_edge,
            edge_count: 0,
        }
    }

    /// Get index past the end of the edge slice
    pub fn slice_end(&self) -> EdgeID {
        self.edge_count + self.first_edge
    }
}

const GROWTH_FACTOR: f64 = 1.1;

pub struct DynamicGraph<T: Clone> {
    node_array: Vec<NodeArrayEntry>,
    edge_array: Vec<EdgeArrayEntry<T>>,

    number_of_nodes: usize,
    number_of_edges: usize,
}

impl<T: Clone + Copy> Default for DynamicGraph<T> {
    fn default() -> Self {
        Self {
            node_array: Vec::new(),
            edge_array: Vec::new(),

            number_of_nodes: 0,
            number_of_edges: 0,
        }
    }
}

impl<T: Clone + Copy> DynamicGraph<T> {
    /// In time O(V+E) check that the following invariants hold:
    /// a) the target node of each non-spare edge is smaller than the number of nodes, and
    /// b) the number of non-spare edges add up to the number of edges, and
    /// c) the first_edge id at each node is valid
    /// d) the number of nodes is consistent with the node array size
    pub fn check_integrity(&self) -> bool {
        self.edge_array
            .iter()
            .filter(|edge_entry| edge_entry.target != usize::MAX)
            .all(|edge_entry| (edge_entry.target) < self.number_of_nodes())
            && self
                .edge_array
                .iter()
                .filter(|edge_entry| edge_entry.target != usize::MAX)
                .count()
                == self.number_of_edges
            && self.node_array[..self.number_of_nodes]
                .iter()
                .filter(|entry| entry.edge_count > 0)
                .all(|entry| entry.first_edge < self.number_of_edges)
            && 2 + self.number_of_nodes == self.node_array.len()
    }

    pub fn new(
        node_count: usize,
        mut input: Vec<impl Edge<ID = NodeID> + EdgeData<DATA = T> + Ord>,
    ) -> Self {
        // sort input edges by source/target/data
        // TODO(dl): sorting by source suffices to construct adjacency array
        input.sort();
        Self::new_from_sorted_list(node_count, &input)
    }

    pub fn new_from_sorted_list(
        number_of_nodes: usize,
        input: &[impl Edge<ID = NodeID> + EdgeData<DATA = T> + Ord],
    ) -> Self {
        // TODO: renumber IDs if necessary
        let number_of_edges = input.len();

        let mut graph = DynamicGraph::<T> {
            number_of_nodes,
            number_of_edges,
            ..Default::default()
        };
        // +1 as we are going to add one sentinel node at the end
        graph.node_array.reserve(number_of_nodes + 1);
        graph
            .edge_array
            .reserve((number_of_edges as f64 * GROWTH_FACTOR) as usize);

        // add first entry manually, rest will be computed
        graph.node_array.push(NodeArrayEntry::new(0));
        let mut offset = 0;
        let mut prev_offset = 0;
        for i in 0..number_of_nodes {
            while offset != input.len() && input[offset].source() == i {
                offset += 1;
            }
            // record the edge count on the source node
            graph.node_array.last_mut().unwrap().edge_count = offset - prev_offset;
            prev_offset = offset;
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

    /// Inserts a node with an empty edge slice into the node array.
    pub fn insert_node(&mut self) {
        self.node_array.push(NodeArrayEntry::new(INVALID_EDGE_ID));
        self.number_of_nodes += 1;
    }

    /// Inserts an edge into the graph by making sure that there's room one
    /// index past the current edge slice. It does so by doing the following:
    /// If one past the slice is a spare edge, use it
    /// Else, if one before the slice is a spare, move the last element over
    /// Else, resize the edge array sufficiently and relocate the slice at
    /// the beginning of the newly added extension of the edge array.
    pub fn insert_edge(&mut self, source: NodeID, target: NodeID, data: T) {
        // if the source of target nodes don't exist yet, then add them.
        while self.number_of_nodes < source {
            self.insert_node();
        }
        while self.number_of_nodes < target {
            self.insert_node();
        }

        // check if array of outgoing edges needs to be moved to the end
        let NodeArrayEntry {
            first_edge,
            edge_count,
        } = self.node_array[source];

        let right_potential_edge_id = first_edge + edge_count;
        if right_potential_edge_id == self.edge_array.len()
            || !self.is_spare_edge(right_potential_edge_id)
        {
            // is there free space left of edge slice?
            if first_edge != 0 && self.is_spare_edge(first_edge - 1) {
                // move the right-most element of the slice one past the left end
                self.node_array[source].first_edge -= 1;
                self.edge_array
                    .swap(first_edge - 1, right_potential_edge_id - 1);
            } else {
                let new_first_edge = self.edge_array.len();
                let new_slice_len = (edge_count as f64 * GROWTH_FACTOR) as usize + 1;
                // do we need to resize the array?
                if self.edge_array.capacity() < (new_first_edge + new_slice_len) {
                    // reserve additional capacity to move data to the end
                    self.edge_array.reserve_exact(new_slice_len);
                }
                self.edge_array.resize(
                    new_first_edge + new_slice_len,
                    EdgeArrayEntry {
                        target: INVALID_NODE_ID,
                        data, // TODO: this is dummy data, should it be T::Default()?
                    },
                );
                // move the edges over and invalidate the old ones
                (0..edge_count).for_each(|i| {
                    self.edge_array.swap(new_first_edge + i, first_edge + i);
                });
                self.node_array[source].first_edge = new_first_edge;
            }
        }

        // At this stage, the entry past the edge slice is guaranteed to be a
        // spare edge. The following lines write the edge array entry there and
        // do the necessary book keeping to keep the graph integrity.
        let edge_id = self.node_array[source].slice_end();
        self.edge_array[edge_id] = EdgeArrayEntry { target, data };
        self.node_array[source].edge_count += 1;
        self.number_of_edges += 1;
    }

    /// Check whether the edge is unused.
    fn is_spare_edge(&self, edge: EdgeID) -> bool {
        self.edge_array[edge].target == usize::MAX
    }

    /// Make the edge unused.
    fn make_spare_edge(&mut self, edge: EdgeID) {
        self.edge_array[edge].target = usize::MAX
    }

    /// Removes an edge by adjusting counters, moving the edge-to-delete to the
    /// end of the edge slice and making it a spare edge
    pub fn remove_edge(&mut self, source: NodeID, edge_to_delete: EdgeID) {
        self.number_of_edges -= 1;
        self.node_array[source].edge_count -= 1;

        let NodeArrayEntry {
            first_edge,
            edge_count,
        } = self.node_array[source];
        let last_edge_at_node = first_edge + edge_count;
        self.edge_array.swap(last_edge_at_node, edge_to_delete);
        self.make_spare_edge(last_edge_at_node);
    }
}

impl<T: Copy> Graph<T> for DynamicGraph<T> {
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
        self.number_of_nodes
    }

    fn number_of_edges(&self) -> usize {
        self.number_of_edges
    }

    fn begin_edges(&self, n: NodeID) -> EdgeID {
        self.node_array[n].first_edge
    }

    fn end_edges(&self, n: NodeID) -> EdgeID {
        self.node_array[n].first_edge + self.out_degree(n)
    }

    fn out_degree(&self, n: NodeID) -> usize {
        self.node_array[n].edge_count
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
    use crate::{dynamic_graph::DynamicGraph, graph::Graph};

    const EDGES: [InputEdge<i32>; 8] = [
        InputEdge {
            source: 0,
            target: 1,
            data: 3,
        },
        InputEdge {
            source: 0,
            target: 4,
            data: 2,
        },
        InputEdge {
            source: 1,
            target: 2,
            data: 3,
        },
        InputEdge {
            source: 1,
            target: 5,
            data: 2,
        },
        InputEdge {
            source: 2,
            target: 3,
            data: 6,
        },
        InputEdge {
            source: 4,
            target: 2,
            data: 1,
        },
        InputEdge {
            source: 4,
            target: 5,
            data: 2,
        },
        InputEdge {
            source: 5,
            target: 3,
            data: 7,
        },
    ];

    #[test]
    fn size() {
        type Graph = DynamicGraph<i32>;
        let graph = Graph::new_from_sorted_list(6, &EDGES);
        assert_eq!(6, graph.number_of_nodes());
        assert_eq!(8, graph.number_of_edges());
    }

    #[test]
    fn degree() {
        type Graph = DynamicGraph<i32>;
        let graph = Graph::new_from_sorted_list(6, &EDGES);
        let mut sum = 0;
        for i in graph.node_range() {
            sum += graph.out_degree(i);
        }
        assert_eq!(sum, graph.number_of_edges());
    }

    #[test]
    fn find_edge() {
        type Graph = DynamicGraph<i32>;
        let graph = Graph::new_from_sorted_list(6, &EDGES);

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

    #[test]
    fn insert_edge() {
        type Graph = DynamicGraph<i32>;

        let mut graph = Graph::new_from_sorted_list(6, &EDGES);
        assert_eq!(6, graph.number_of_nodes());
        assert_eq!(8, graph.number_of_edges());

        // the node exists, but it's degree is 0, it will be created at the end
        assert_eq!(0, graph.out_degree(3));
        graph.insert_edge(3, 5, 123);

        // check that graph was expanded and edge exists
        assert_eq!(1, graph.out_degree(3));
        assert_eq!(6, graph.number_of_nodes());
        assert_eq!(9, graph.number_of_edges());
        assert!(graph.find_edge(3, 5).is_some());

        // check that all other edges exist as well
        for edge in &EDGES {
            let result = graph.find_edge(edge.source, edge.target);
            assert!(result.is_some());
            let edge_id = result.unwrap();
            let data = *graph.data(edge_id);
            assert_eq!(edge.data, data);
        }

        // insert edge that forces moving edge slice to end of array
        graph.insert_edge(4, 1, 7);
        // check that edge exists
        assert_eq!(6, graph.number_of_nodes());
        assert_eq!(10, graph.number_of_edges());
        assert!(graph.find_edge(4, 1).is_some());
        assert!(graph.check_integrity());

        // insert edge that finds a free slot left of its slice
        graph.insert_edge(5, 1, 1234);
        // check that edge exists
        assert_eq!(6, graph.number_of_nodes());
        assert_eq!(11, graph.number_of_edges());
        assert!(graph.find_edge(5, 1).is_some());
        assert!(graph.check_integrity());

        // insert edge that finds a free slot left of right slice
        assert!(graph.find_edge(2, 5).is_none());
        graph.insert_edge(2, 5, 1234);
        // check that edge exists
        assert_eq!(6, graph.number_of_nodes());
        assert_eq!(12, graph.number_of_edges());
        assert!(graph.find_edge(2, 5).is_some());
        assert!(graph.check_integrity());

        // check that all other edges exist as well
        for edge in &EDGES {
            let result = graph.find_edge(edge.source, edge.target);
            assert!(result.is_some());
            let edge_id = result.unwrap();
            let data = *graph.data(edge_id);
            assert_eq!(edge.data, data);
        }

        assert!(graph.find_edge(3, 5).is_some());
        assert!(graph.find_edge(4, 1).is_some());
        assert!(graph.find_edge(5, 1).is_some());
        assert!(graph.find_edge(2, 5).is_some());
    }
}
