use crate::graph::NodeID;
use std::mem::swap;

#[derive(Clone, Copy, Debug, Default, Eq, PartialOrd, Ord, PartialEq)]
pub struct InputEdge<EdgeDataT: Eq> {
    pub source: NodeID,
    pub target: NodeID,
    pub data: EdgeDataT,
}

impl<EdgeDataT: Eq> InputEdge<EdgeDataT> {
    pub fn new(source: NodeID, target: NodeID, data: EdgeDataT) -> Self {
        Self {
            source,
            target,
            data,
        }
    }

    pub fn reverse(&mut self) {
        swap(&mut self.source, &mut self.target);
    }
}
pub type SimpleEdge = InputEdge<u32>;
