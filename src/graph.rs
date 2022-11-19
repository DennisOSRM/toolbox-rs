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
#[derive(Clone, Copy)]
pub struct EdgeArrayEntry<EdgeDataT: Clone> {
    pub target: NodeID,
    pub data: EdgeDataT,
}
