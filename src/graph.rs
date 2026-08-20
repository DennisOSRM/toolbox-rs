use core::ops::Range;

pub type NodeID = usize;
pub type EdgeID = usize;
pub const INVALID_NODE_ID: NodeID = NodeID::MAX;
pub const INVALID_EDGE_ID: EdgeID = EdgeID::MAX;
pub const UNREACHABLE: usize = usize::MAX;

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
