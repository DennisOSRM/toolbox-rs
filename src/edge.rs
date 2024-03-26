use serde::{Deserialize, Serialize};

use crate::graph::NodeID;
use core::mem::swap;

pub trait Edge {
    type ID;
    fn source(&self) -> Self::ID;
    fn target(&self) -> Self::ID;
}

pub trait EdgeData {
    type DATA;
    fn data(&self) -> &Self::DATA;
}

#[derive(Clone, Copy)]
pub struct TrivialEdge {
    pub source: usize,
    pub target: usize,
}

impl Edge for TrivialEdge {
    type ID = NodeID;
    fn source(&self) -> Self::ID {
        self.source
    }
    fn target(&self) -> Self::ID {
        self.target
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialOrd, Ord, PartialEq, Serialize)]
pub struct InputEdge<EdgeDataT: Eq> {
    pub source: NodeID,
    pub target: NodeID,
    pub data: EdgeDataT,
}

impl<EdgeDataT: std::cmp::Eq> Edge for InputEdge<EdgeDataT> {
    type ID = NodeID;
    fn source(&self) -> Self::ID {
        self.source
    }
    fn target(&self) -> Self::ID {
        self.target
    }
}

impl<EdgeDataT: std::cmp::Eq> EdgeData for InputEdge<EdgeDataT> {
    type DATA = EdgeDataT;
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

    pub fn is_parallel_to(&self, other: &Self) -> bool {
        self.source == other.source && self.target == other.target
    }

    pub fn reverse(&mut self) {
        swap(&mut self.source, &mut self.target);
    }
}
pub type SimpleEdge = InputEdge<u32>;

#[test]
fn simple_edge_parallel() {
    let edge1 = SimpleEdge::new(1, 2, 3);
    let edge2 = SimpleEdge::new(1, 2, 6);

    assert!(edge1.is_parallel_to(&edge1));
    assert!(edge1.is_parallel_to(&edge2));
    assert!(edge2.is_parallel_to(&edge1));
    assert!(edge2.is_parallel_to(&edge2));
}

#[test]
fn trivial_edge_accessor() {
    let edge = TrivialEdge {
        source: 1,
        target: 2,
    };

    assert_eq!(edge.source(), 1);
    assert_eq!(edge.target(), 2);
}
