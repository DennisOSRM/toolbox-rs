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
    /// Ask for the offsets of a node, which is where a scan of its arcs begins.
    ///
    /// A node taken off a queue is a random place in the node array, so this is
    /// the first of the two misses a walk of its arcs costs.
    #[inline(always)]
    pub fn prefetch_node(&self, node: NodeID) {
        if let Some(entry) = self.node_array.get(node) {
            crate::prefetch::hint(std::ptr::from_ref(entry));
        }
    }

    /// Ask for the block of arcs a node owns, which is the second.
    #[inline(always)]
    pub fn prefetch_arcs(&self, edge: EdgeID) {
        if let Some(entry) = self.edge_array.get(edge) {
            crate::prefetch::hint(std::ptr::from_ref(entry));
        }
    }

    // In time O(V+E) check that the following invariants hold:
    // a) the node array spans the edge array, from zero up to its length. An
    //    empty node array fails here, as it lacks even the sentinel.
    // b) the target node of each edge is smaller than the number of nodes
    // c) index values for nodes first_edges are non-decreasing, as a node
    //    without any outgoing edge shares its offset with the next node
    // d) the targets within each adjacency block are sorted ascendingly
    pub fn check_integrity(&self) -> bool {
        self.node_array
            .first()
            .is_some_and(|entry| entry.first_edge == 0)
            && self
                .node_array
                .last()
                .is_some_and(|entry| entry.first_edge == self.edge_array.len())
            && self
                .edge_array
                .iter()
                .all(|edge_entry| (edge_entry.target as usize) < self.number_of_nodes())
            && self
                .node_array
                .windows(2)
                .all(|pair| pair[0].first_edge <= pair[1].first_edge)
            && self.node_range().all(|node| {
                // an offset that points past the edge array is a failed check
                // and not a reason to panic
                self.edge_array
                    .get(self.edge_range(node))
                    .is_some_and(|block| {
                        block
                            .windows(2)
                            .all(|pair| pair[0].target <= pair[1].target)
                    })
            })
    }

    /// Finds the edge (s,t) by a binary search over the adjacency block of s.
    /// This requires the targets within a block to be sorted, which holds for
    /// all of this type's constructors.
    pub fn find_edge_sorted(&self, s: NodeID, t: NodeID) -> Option<EdgeID> {
        if s >= self.number_of_nodes() {
            return None;
        }
        let range = self.edge_range(s);
        self.edge_array[range.clone()]
            .binary_search_by_key(&t, |entry| entry.target as NodeID)
            .ok()
            .map(|offset| range.start + offset)
    }

    pub fn new(mut input: Vec<impl Edge<ID = NodeID> + EdgeData<DATA = T> + Ord>) -> Self {
        // sort input edges by source/target/data
        // TODO(dl): sorting by source suffices to construct adjacency array
        input.sort();

        Self::new_from_sorted_list(input)
    }

    /// Assembles a graph that holds at least `nodes` nodes, whether or not an
    /// arc reaches the last of them.
    ///
    /// [`Self::new`] counts the nodes off the arcs it is handed, which is one
    /// short of what a caller means whenever the highest numbered node has no
    /// arc at all. Such a node is not a curiosity: a node of a cell whose only
    /// arcs leave the cell has none inside it, and a search started there would
    /// read past the end of the node array.
    pub fn new_with_nodes(
        nodes: usize,
        mut input: Vec<impl Edge<ID = NodeID> + EdgeData<DATA = T> + Ord>,
    ) -> Self {
        input.sort();
        Self::assemble(nodes, &input)
    }

    /// Assembles a graph from a prebuilt adjacency array. The caller has to
    /// guarantee that `node_array` is non-decreasing, starts at zero, ends at
    /// `edge_array.len()` and that the targets within each adjacency block are
    /// sorted ascendingly.
    pub fn from_adjacency_array(
        node_array: Vec<EdgeID>,
        edge_array: Vec<EdgeArrayEntry<T>>,
    ) -> Self {
        let graph = Self {
            // the layouts of EdgeID and NodeArrayEntry match, thus the
            // allocation is reused instead of copied
            node_array: node_array.into_iter().map(NodeArrayEntry::new).collect(),
            edge_array,
        };
        debug_assert!(graph.check_integrity());
        graph
    }

    pub fn new_from_sorted_list(
        input: Vec<impl Edge<ID = NodeID> + EdgeData<DATA = T> + Ord>,
    ) -> Self {
        Self::assemble(0, &input)
    }

    /// Builds the adjacency array out of a sorted arc list, over as many nodes
    /// as the arcs reach or as many as were asked for, whichever is more.
    /// Builds the arrays from a sorted list the caller keeps.
    ///
    /// A customization builds one graph per cell and a continent has six
    /// hundred thousand cells, so the list the graph is made of is worth
    /// refilling rather than making anew each time. Nothing here needs to own
    /// it: the arrays are built by reading it once.
    pub fn from_sorted_slice(
        nodes: usize,
        input: &[impl Edge<ID = NodeID> + EdgeData<DATA = T> + Ord],
    ) -> Self {
        Self::assemble(nodes, input)
    }

    fn assemble(nodes: usize, input: &[impl Edge<ID = NodeID> + EdgeData<DATA = T> + Ord]) -> Self {
        // TODO: renumber IDs if necessary
        let number_of_edges = input.len();
        let mut number_of_nodes = nodes.saturating_sub(1);
        for edge in input {
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

        // extended rather than collected into: collecting throws away the
        // room reserved for it just above and asks for the same room again
        graph
            .edge_array
            .extend(input.iter().map(|edge| EdgeArrayEntry {
                target: u32::try_from(edge.target()).expect("the graph is too large to hold"),
                data: *edge.data(),
            }));
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
        self.node_array[n + 1].first_edge
    }

    fn out_degree(&self, n: NodeID) -> usize {
        let up = self.end_edges(n);
        let down = self.begin_edges(n);
        up - down
    }

    fn target(&self, e: EdgeID) -> NodeID {
        self.edge_array[e].target as NodeID
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
    fn integrity_check_reports_broken_offsets() {
        use crate::graph::EdgeArrayEntry;
        use crate::static_graph::NodeArrayEntry;

        // a single node whose block claims five arcs while there are only two
        let graph = StaticGraph::<i32> {
            node_array: vec![NodeArrayEntry::new(0), NodeArrayEntry::new(5)],
            edge_array: vec![
                EdgeArrayEntry { target: 0, data: 1 },
                EdgeArrayEntry { target: 0, data: 1 },
            ],
        };
        assert!(!graph.check_integrity());

        // a default constructed graph has no sentinel at all
        assert!(!StaticGraph::<i32>::default().check_integrity());
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

    #[test]
    fn a_graph_holds_the_nodes_it_was_asked_for() {
        // an arc list that reaches nodes 0 and 1 only, over a graph of five
        let edges = vec![InputEdge::new(0, 1, 3), InputEdge::new(1, 0, 3)];
        let graph = StaticGraph::<i32>::new_with_nodes(5, edges);

        assert_eq!(graph.number_of_nodes(), 5);
        assert_eq!(graph.number_of_edges(), 2);
        // the nodes no arc reaches are there and hold none
        for node in 2..5 {
            assert_eq!(graph.edge_range(node).count(), 0);
        }
        assert_eq!(graph.out_degree(0), 1);
    }

    #[test]
    fn asking_for_fewer_nodes_than_the_arcs_reach_changes_nothing() {
        let edges = vec![InputEdge::new(0, 4, 3), InputEdge::new(4, 0, 3)];
        let asked = StaticGraph::<i32>::new_with_nodes(2, edges.clone());
        let counted = StaticGraph::<i32>::new(edges);

        assert_eq!(asked.number_of_nodes(), counted.number_of_nodes());
        assert_eq!(asked.number_of_nodes(), 5);
    }

    #[test]
    fn a_graph_of_no_arcs_still_holds_its_nodes() {
        let graph = StaticGraph::<i32>::new_with_nodes(3, Vec::<InputEdge<i32>>::new());
        assert_eq!(graph.number_of_nodes(), 3);
        assert_eq!(graph.number_of_edges(), 0);
        assert_eq!(graph.edge_range(2).count(), 0);
    }
}
