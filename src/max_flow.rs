use std::sync::{atomic::AtomicI32, Arc};

use crate::{
    edge::{EdgeWithData, InputEdge},
    graph::NodeID,
};
use bitvec::vec::BitVec;
use log::debug;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResidualEdgeData {
    pub capacity: i32,
    pub reverse_is_admissable: bool,
}

impl ResidualEdgeData {
    pub fn new(capacity: i32) -> ResidualEdgeData {
        ResidualEdgeData {
            capacity,
            reverse_is_admissable: false,
        }
    }
}

pub trait MaxFlow {
    fn run(&mut self);
    fn run_with_upper_bound(&mut self, bound: Arc<AtomicI32>);
    fn max_flow(&self) -> Result<i32, String>;
    fn assignment(&self, source: NodeID) -> Result<BitVec, String>;
    fn from_edge_list(
        edges: Vec<InputEdge<ResidualEdgeData>>,
        source: NodeID,
        sink: NodeID,
    ) -> Self;
    fn from_generic_edge_list<E: EdgeWithData>(
        input_edges: &[E],
        source: NodeID,
        target: NodeID,
        function: impl Fn(&E) -> ResidualEdgeData,
    ) -> Self
    where
        Self: Sized,
    {
        debug_assert!(!input_edges.is_empty());
        debug!("instantiating max-flow solver");
        let edge_list: Vec<InputEdge<ResidualEdgeData>> = input_edges
            .iter()
            .map(move |edge| InputEdge {
                source: edge.source(),
                target: edge.target(),
                data: function(edge),
            })
            .collect();

        debug!("created {} ff edges", edge_list.len());
        Self::from_edge_list(edge_list, source, target)
    }
}
