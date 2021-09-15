use std::mem::swap;
use std::ops::Range;

pub type NodeID = u32;
pub type EdgeID = u32;
pub const INVALID_NODE_ID: NodeID = NodeID::MAX;
pub const INVALID_EDGE_ID: EdgeID = EdgeID::MAX;

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

pub trait Graph<T> {
    fn node_range(&self) -> Range<NodeID>;
    fn edge_range(&self, n: NodeID) -> Range<EdgeID>;
    fn number_of_nodes(&self) -> usize;
    fn number_of_edges(&self) -> usize;
    fn begin_edges(&self, n: NodeID) -> EdgeID;
    fn end_edges(&self, n: NodeID) -> EdgeID;
    fn get_out_degree(&self, n: NodeID) -> usize;
    fn target(&self, e: EdgeID) -> NodeID;
    fn data(&self, e: EdgeID) -> &T;
    fn data_mut(&mut self, e: EdgeID) -> &mut T;
    fn find_edge(&self, s: NodeID, t: NodeID) -> Option<EdgeID>;
}
