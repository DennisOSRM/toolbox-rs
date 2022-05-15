use std::sync::{atomic::AtomicI32, Arc};

use crate::graph::NodeID;
use bitvec::vec::BitVec;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResidualCapacity {
    pub capacity: i32,
}

impl ResidualCapacity {
    pub fn new(capacity: i32) -> ResidualCapacity {
        ResidualCapacity { capacity }
    }
}

impl From<i32> for ResidualCapacity {
    fn from(item: i32) -> Self {
        ResidualCapacity { capacity: item }
    }
}

pub trait MaxFlow {
    fn run(&mut self);
    fn run_with_upper_bound(&mut self, bound: Arc<AtomicI32>);
    fn max_flow(&self) -> Result<i32, String>;
    fn assignment(&self, source: NodeID) -> Result<BitVec, String>;
}
