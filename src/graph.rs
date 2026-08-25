use core::ops::Range;

pub type NodeID = usize;
pub type EdgeID = usize;
pub const INVALID_NODE_ID: NodeID = NodeID::MAX;
pub const INVALID_EDGE_ID: EdgeID = EdgeID::MAX;
pub const UNREACHABLE: usize = usize::MAX;

/// What a search asks of a graph.
///
/// # Why this is not [`Graph`]
///
/// The weight comes back by value, and that is the whole of the difference. A
/// graph holding its arcs in memory can lend one out and the reference stays
/// good for as long as the graph does. A graph reading its arcs off a file in
/// blocks cannot: the block a reference would point into has to be free to be
/// let go of the moment the room it takes is wanted for another, and a
/// reference handed out earlier would still be pointing at it.
///
/// So a search that means to run over either asks for this, and every
/// [`Graph`] answers it without being written to.
pub trait Arcs<T> {
    fn node_range(&self) -> Range<NodeID>;
    fn edge_range(&self, n: NodeID) -> Range<EdgeID>;
    fn number_of_nodes(&self) -> usize;
    fn number_of_edges(&self) -> usize;
    fn target(&self, e: EdgeID) -> NodeID;
    /// What the arc costs, by value.
    fn weight(&self, e: EdgeID) -> T;

    /// Every arc leaving a node, as where it goes and what it costs.
    ///
    /// # Why a search should use this and not the three above
    ///
    /// For a graph held in memory the two are the same and this is written in
    /// terms of them. For one reading off a file they are not: asked arc by
    /// arc, it has to find the block each arc is in every time, and a search
    /// relaxing tens of thousands of arcs pays that tens of thousands of times
    /// over. Asked node by node it finds the block once and walks it.
    ///
    /// It is the same answer either way. The difference was measured at about
    /// an order of magnitude on a continent.
    fn for_each_arc(&self, n: NodeID, mut f: impl FnMut(NodeID, T)) {
        for edge in self.edge_range(n) {
            f(self.target(edge), self.weight(edge));
        }
    }
}

/// Every graph that keeps its arcs answers by lending one and copying it out.
impl<T: Copy, G: Graph<T>> Arcs<T> for G {
    #[inline]
    fn node_range(&self) -> Range<NodeID> {
        Graph::node_range(self)
    }

    #[inline]
    fn edge_range(&self, n: NodeID) -> Range<EdgeID> {
        Graph::edge_range(self, n)
    }

    #[inline]
    fn number_of_nodes(&self) -> usize {
        Graph::number_of_nodes(self)
    }

    #[inline]
    fn number_of_edges(&self) -> usize {
        Graph::number_of_edges(self)
    }

    #[inline]
    fn target(&self, e: EdgeID) -> NodeID {
        Graph::target(self, e)
    }

    #[inline]
    fn weight(&self, e: EdgeID) -> T {
        *Graph::data(self, e)
    }
}

pub trait Graph<T> {
    fn node_range(&self) -> Range<NodeID>;
    fn edge_range(&self, n: NodeID) -> Range<EdgeID>;
    fn number_of_nodes(&self) -> usize;
    fn number_of_edges(&self) -> usize;
    fn begin_edges(&self, n: NodeID) -> EdgeID;
    fn end_edges(&self, n: NodeID) -> EdgeID;
    fn out_degree(&self, n: NodeID) -> usize;
    fn target(&self, e: EdgeID) -> NodeID;
    fn data(&self, e: EdgeID) -> &T;
    fn data_mut(&mut self, e: EdgeID) -> &mut T;
    fn find_edge(&self, s: NodeID, t: NodeID) -> Option<EdgeID>;
    fn find_edge_unchecked(&self, s: NodeID, t: NodeID) -> EdgeID;
}
/// One arc of an adjacency array: where it goes, and what it costs.
///
/// The target is four bytes rather than eight, and the cost is meant to be
/// four as well. Both together is the point: a four byte target beside an
/// eight byte cost is padded back out to sixteen, so narrowing one without the
/// other buys nothing at all. Narrowing both takes a continent's arcs from six
/// hundred and seventy five megabytes to three hundred and thirty eight, and a
/// search that walks them moves half the memory for the same answers.
///
/// Four bytes reach four thousand million, which is more nodes than this crate
/// can hold anyway: the cell tables address them with four bytes too.
#[derive(Clone, Copy)]
pub struct EdgeArrayEntry<EdgeDataT: Clone> {
    pub target: u32,
    pub data: EdgeDataT,
}
