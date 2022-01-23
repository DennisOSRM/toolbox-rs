use crate::graph::NodeID;
use bitvec::vec::BitVec;

pub trait MaxFlow {
    fn run(&mut self);
    fn max_flow(&self) -> Result<i32, String>;
    fn assignment(&self, source: NodeID) -> Result<BitVec, String>;
}