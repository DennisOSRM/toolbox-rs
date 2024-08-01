use std::sync::{atomic::AtomicI32, Arc};

use crate::graph::NodeID;
use bitvec::vec::BitVec;

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
}
