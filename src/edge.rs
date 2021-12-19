use crate::graph::NodeID;
use std::mem::swap;

pub trait Edge {
    type ID;
    type DATA;
    fn source(&self) -> Self::ID;
    fn target(&self) -> Self::ID;
    fn data(&self) -> &Self::DATA;
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialOrd, Ord, PartialEq)]
pub struct InputEdge<EdgeDataT: Eq> {
    pub source: NodeID,
    pub target: NodeID,
    pub data: EdgeDataT,
}

impl<EdgeDataT: std::cmp::Eq> Edge for InputEdge<EdgeDataT> {
    type ID = NodeID;
    type DATA = EdgeDataT;
    fn source(&self) -> Self::ID {
        self.source
    }
    fn target(&self) -> Self::ID {
        self.target
    }
    fn data(&self) -> &Self::DATA {
        &self.data
    }
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
