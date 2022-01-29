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

pub trait MaxFlow {
    fn run(&mut self);
    fn max_flow(&self) -> Result<i32, String>;
    fn assignment(&self, source: NodeID) -> Result<BitVec, String>;
}