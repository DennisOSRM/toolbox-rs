use crate::edge::Edge;
use crate::edge::InputEdge;
use crate::graph::{Graph, NodeID};
use crate::static_graph::StaticGraph;
use bitvec::vec::BitVec;
use core::cmp::min;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct EdgeCapacity {
    pub capacity: i32,
    pub flow: i32,
}

impl EdgeCapacity {
    pub fn new(capacity: i32) -> EdgeCapacity {
        EdgeCapacity { capacity, flow: 0 }
    }

    pub fn remaining_capacity(&self) -> i32 {
        self.capacity - self.flow
    }
}

pub struct PushRelabel {
    residual_graph: StaticGraph<EdgeCapacity>,
    max_flow: i32,
    finished: bool,
    height: Vec<i32>, // TODO: limits the graph to 2bn edges
    excess: Vec<i32>,
    excess_nodes: Vec<NodeID>,
    source_set: BitVec,
    target_set: BitVec,
}

impl PushRelabel {
    fn push(&mut self, u: NodeID, v: NodeID) {
        let fwd_edge = self
            .residual_graph
            .find_edge(u, v)
            .expect("Graph broken, could not find expected fwd_edge");
        let mut fwd_edge_data = self.residual_graph.data_mut(fwd_edge);
        let d = min(self.excess[u], fwd_edge_data.remaining_capacity());
        fwd_edge_data.flow += d;
        println!(
            "  pushing flow({},{})={} - fwd_edge_data.flow={}",
            u, v, d, fwd_edge_data.flow
        );

        let rev_edge = self
            .residual_graph
            .find_edge(v, u)
            .expect("Graph broken, could not find expected rev_edge");
        let mut rev_edge_data = self.residual_graph.data_mut(rev_edge);
        rev_edge_data.flow -= d;
        println!(
            "  pushing flow({},{})={} - rev_edge_data.flow={}",
            v, u, d, rev_edge_data.flow
        );

        self.excess[u] -= d;
        println!("  self.excess[{}]={}", u, self.excess[u]);
        debug_assert!(self.excess[u] >= 0);
        self.excess[v] += d;
        println!("  self.excess[{}]={}", v, self.excess[v]);
        debug_assert!(self.excess[v] >= 0);

        // if d >= 0 && self.excess[v] == d {
        //     println!("  pushing {} to queue", v);
        //     self.excess_nodes.push(v);
        // }
        // IF w ≠ s,t AND w ∉ Q THEN Q.add(w)
        if !self.source_set[v] && !self.target_set[v] && !self.excess_nodes.contains(&v) {
            self.excess_nodes.push(v);
        }
    }

    fn relabel(&mut self, v: NodeID) {
        let mut d = i32::MAX;
        for edge in self.residual_graph.edge_range(v) {
            let edge_data = self.residual_graph.data_mut(edge);
            if edge_data.remaining_capacity() > 0 {
                let t = self.residual_graph.target(edge);
                d = min(d, self.height[t]);
            }
        }

        if d < i32::MAX {
            println!("  relabel height[{}]={}", v, d + 1);
            self.height[v] = d + 1;
            println!("  pushing {} to queue", v);
            self.excess_nodes.push(v);
        }
    }

    fn discharge(&mut self, u: NodeID) {
        println!("popped {} from queue", u);
        for edge in self.residual_graph.edge_range(u) {
            if self.excess[u] <= 0 {
                println!("node {} doesn't have excess flow anymore", u);
                break;
            }
            let v = self.residual_graph.target(edge);
            let edge_data = self.residual_graph.data(edge);

            if edge_data.remaining_capacity() > 0 && self.height[u] == self.height[v] + 1 {
                println!("pushing edge ({},{})", u, v);
                self.push(u, v);
            } else {
                println!(
                    "ignoring edge ({},{}), height[u]={}, height[v]={}, capacity={}",
                    u,
                    v,
                    self.height[u],
                    self.height[v],
                    edge_data.remaining_capacity()
                );
            }
        }
        if self.excess[u] > 0 {
            println!("relabel {}", u);
            self.relabel(u);
        }
    }

    // todo(dl): add closure parameter to derive edge data
    pub fn from_generic_edge_list(input_edges: Vec<impl Edge<ID = NodeID>>) -> Self {
        let edge_list: Vec<InputEdge<EdgeCapacity>> = input_edges
            .into_iter()
            .map(|edge| InputEdge {
                source: edge.source(),
                target: edge.target(),
                data: EdgeCapacity::new(1),
            })
            .collect();

        // println!("created {} ff edges", edge_list.len());
        PushRelabel::from_edge_list(edge_list)
    }

    pub fn from_edge_list(mut edge_list: Vec<InputEdge<EdgeCapacity>>) -> Self {
        let number_of_edges = edge_list.len();

        // println!("extending {} edges", edge_list.len());
        // blindly generate reverse edges for all edges with zero capacity
        edge_list.extend_from_within(..);
        edge_list.iter_mut().skip(number_of_edges).for_each(|edge| {
            edge.reverse();
            edge.data.capacity = 0;
        });
        // println!("into {} edges", edge_list.len());

        // dedup-merge edge set, by using the following trick: not the dedup(.) call
        // below takes the second argument as mut. When deduping equivalent values
        // a and b, then a is accumulated onto b.
        edge_list.sort_unstable();
        edge_list.dedup_by(|a, mut b| {
            // edges a and b are assumed to be equivalent in the residual graph if
            // (and only if) they are parallel. In other words, this removes parallel
            // edges in the residual graph and accumulates capacities on the remaining
            // egde.
            let result = a.source == b.source && a.target == b.target;
            if result {
                b.data.capacity += a.data.capacity;
            }
            result
        });
        // println!("dedup-merged {} edges", edge_list.len());

        // at this point the edge set of the residual graph doesn't have any
        // duplicates anymore. note that this is fine, as we are looking to
        // compute a node partition.

        Self {
            residual_graph: StaticGraph::new(edge_list),
            max_flow: 0,
            finished: false,
            height: Vec::new(),
            excess: Vec::new(),
            excess_nodes: Vec::new(),
            source_set: BitVec::new(),
            target_set: BitVec::new(),
        }
    }

    pub fn run(&mut self, sources: &[NodeID], targets: &[NodeID]) {
        let number_of_nodes = self.residual_graph.number_of_nodes();

        // init
        self.height = vec![0; number_of_nodes];
        self.excess = vec![0; number_of_nodes];

        // let mut target_set: BitVec = BitVec::with_capacity(number_of_nodes);
        self.target_set.resize(number_of_nodes, false);
        for i in targets {
            // println!("setting target {}", i);
            self.target_set.set(*i as usize, true);
        }

        // let mut source_set: BitVec = BitVec::with_capacity(number_of_nodes);
        self.source_set.resize(number_of_nodes, false);
        for i in sources {
            // println!("setting source {}", i);
            self.source_set.set(*i as usize, true);
        }

        // TODO: this is not tight, should first init all sources, then start traversal
        println!("source set: {:#?}", sources);
        println!("target set: {:#?}", targets);
        for s in sources {
            self.height[*s] = number_of_nodes as i32;
            println!("source height[{}]={}", s, number_of_nodes);
            self.excess[*s] = i32::MAX;

            for edge in self.residual_graph.edge_range(*s) {
                let t = self.residual_graph.target(edge);
                if self.source_set[t] {
                    println!("ignoring edge ({},{})", s, t);
                    // don't push edges inside source set
                    continue;
                }
                println!("pushing edge ({},{})", s, t);
                self.push(*s, t);
            }
        }

        println!("<== init done ==>");

        while let Some(u) = self.excess_nodes.pop() {
            if !self.source_set[u] && !self.target_set[u] {
                self.discharge(u);
            }
        }

        let mut flow = 0;
        for t in targets {
            for rev_edge in self.residual_graph.edge_range(*t) {
                let s = self.residual_graph.target(rev_edge);
                let fwd_edge = self
                    .residual_graph
                    .find_edge(s, *t)
                    .expect("Graph broken. Could not find expected edge");
                // println!("flow({},{})={}", s, t, self.residual_graph.data(fwd_edge).flow);
                flow += self.residual_graph.data(fwd_edge).flow;
            }
        }

        self.max_flow = flow;
        self.finished = true;
    }

    pub fn max_flow(&self) -> Result<i32, String> {
        if !self.finished {
            return Err("Assigment was not computed.".to_string());
        }
        Ok(self.max_flow)
    }

    pub fn assignment(&self, sources: &[NodeID]) -> Result<BitVec, String> {
        if !self.finished {
            return Err("Assigment was not computed.".to_string());
        }

        // run a reachability analysis
        let mut reachable = BitVec::with_capacity(self.residual_graph.number_of_nodes());
        reachable.resize(self.residual_graph.number_of_nodes(), false);
        let mut stack: Vec<usize> = sources.iter().copied().collect();
        while let Some(node) = stack.pop() {
            // TODO: looks like this following is superflous work?
            if *reachable.get(node as usize).unwrap() {
                continue;
            }
            reachable.set(node as usize, true);
            // println!("reached {}", node);
            for edge in self.residual_graph.edge_range(node) {
                let target = self.residual_graph.target(edge);
                let reached = reachable.get(target as usize).unwrap();
                if !reached && self.residual_graph.data(edge).remaining_capacity() > 0 {
                    stack.push(self.residual_graph.target(edge));
                }
            }
        }

        // println!("done.");
        Ok(reachable)
    }
}

#[cfg(test)]
mod tests {

    use crate::edge::InputEdge;
    use crate::push_relabel::EdgeCapacity;
    use crate::push_relabel::PushRelabel;
    use bitvec::bits;
    use bitvec::prelude::Lsb0;

    #[test]
    fn max_flow_clr_single_source_target() {
        let edges = vec![
            InputEdge::new(0, 1, EdgeCapacity::new(16)),
            InputEdge::new(0, 2, EdgeCapacity::new(13)),
            InputEdge::new(1, 2, EdgeCapacity::new(10)),
            InputEdge::new(1, 3, EdgeCapacity::new(12)),
            InputEdge::new(2, 1, EdgeCapacity::new(4)),
            InputEdge::new(2, 4, EdgeCapacity::new(14)),
            InputEdge::new(3, 2, EdgeCapacity::new(9)),
            InputEdge::new(3, 5, EdgeCapacity::new(20)),
            InputEdge::new(4, 3, EdgeCapacity::new(7)),
            InputEdge::new(4, 5, EdgeCapacity::new(4)),
        ];

        let mut max_flow_solver = PushRelabel::from_edge_list(edges);
        let sources = [0];
        let targets = [5];
        max_flow_solver.run(&sources, &targets);

        // it's OK to expect the solver to have run
        let max_flow = max_flow_solver
            .max_flow()
            .expect("max flow computation did not run");
        assert_eq!(23, max_flow);

        // it's OK to expect the solver to have run
        let assignment = max_flow_solver
            .assignment(&sources)
            .expect("assignment computation did not run");

        assert_eq!(assignment, bits![1, 1, 1, 0, 1, 0]);
    }

    #[test]
    fn _single_source_target_multi_target_set() {
        let edges = vec![
            InputEdge::new(0, 1, EdgeCapacity::new(16)),
            InputEdge::new(0, 2, EdgeCapacity::new(13)),
            InputEdge::new(1, 2, EdgeCapacity::new(10)),
            InputEdge::new(1, 3, EdgeCapacity::new(12)),
            InputEdge::new(2, 1, EdgeCapacity::new(4)),
            InputEdge::new(2, 4, EdgeCapacity::new(14)),
            InputEdge::new(3, 2, EdgeCapacity::new(9)),
            InputEdge::new(3, 5, EdgeCapacity::new(20)),
            InputEdge::new(4, 3, EdgeCapacity::new(7)),
            InputEdge::new(4, 5, EdgeCapacity::new(4)),
            InputEdge::new(5, 6, EdgeCapacity::new(1)),
            InputEdge::new(6, 1, EdgeCapacity::new(1)),
            InputEdge::new(0, 7, EdgeCapacity::new(1)),
            InputEdge::new(7, 1, EdgeCapacity::new(1)),
        ];

        let mut max_flow_solver = PushRelabel::from_edge_list(edges);
        let sources = [0];
        let targets = [5, 6];
        max_flow_solver.run(&sources, &targets);

        // it's OK to expect the solver to have run
        let max_flow = max_flow_solver
            .max_flow()
            .expect("max flow computation did not run");
        assert_eq!(23, max_flow);

        // it's OK to expect the solver to have run
        let assignment = max_flow_solver
            .assignment(&sources)
            .expect("assignment computation did not run");

        assert_eq!(assignment, bits![1, 1, 1, 0, 1, 0, 0, 1]);
    }

    #[test]
    fn max_flow_ita() {
        let edges = vec![
            InputEdge::new(0, 1, EdgeCapacity::new(5)),
            InputEdge::new(0, 4, EdgeCapacity::new(7)),
            InputEdge::new(0, 5, EdgeCapacity::new(6)),
            InputEdge::new(1, 2, EdgeCapacity::new(4)),
            InputEdge::new(1, 7, EdgeCapacity::new(3)),
            InputEdge::new(4, 7, EdgeCapacity::new(4)),
            InputEdge::new(4, 6, EdgeCapacity::new(1)),
            InputEdge::new(5, 6, EdgeCapacity::new(5)),
            InputEdge::new(2, 3, EdgeCapacity::new(3)),
            InputEdge::new(7, 3, EdgeCapacity::new(7)),
            InputEdge::new(6, 7, EdgeCapacity::new(1)),
            InputEdge::new(6, 3, EdgeCapacity::new(6)),
        ];

        let mut max_flow_solver = PushRelabel::from_edge_list(edges);
        let sources = [0];
        let targets = [3];
        max_flow_solver.run(&sources, &targets);

        // it's OK to expect the solver to have run
        let max_flow = max_flow_solver
            .max_flow()
            .expect("max flow computation did not run");
        assert_eq!(15, max_flow);

        // it's OK to expect the solver to have run
        let assignment = max_flow_solver
            .assignment(&sources)
            .expect("assignment computation did not run");
        assert_eq!(assignment, bits![1, 0, 0, 0, 1, 1, 0, 0]);
    }

    #[test]
    fn max_flow_yt() {
        let edges = vec![
            InputEdge::new(9, 0, EdgeCapacity::new(5)),
            InputEdge::new(9, 1, EdgeCapacity::new(10)),
            InputEdge::new(9, 2, EdgeCapacity::new(15)),
            InputEdge::new(0, 3, EdgeCapacity::new(10)),
            InputEdge::new(1, 0, EdgeCapacity::new(15)),
            InputEdge::new(1, 4, EdgeCapacity::new(20)),
            InputEdge::new(2, 5, EdgeCapacity::new(25)),
            InputEdge::new(3, 4, EdgeCapacity::new(25)),
            InputEdge::new(3, 6, EdgeCapacity::new(10)),
            InputEdge::new(4, 2, EdgeCapacity::new(5)),
            InputEdge::new(4, 7, EdgeCapacity::new(30)),
            InputEdge::new(5, 7, EdgeCapacity::new(20)),
            InputEdge::new(5, 8, EdgeCapacity::new(10)),
            InputEdge::new(7, 8, EdgeCapacity::new(15)),
            InputEdge::new(6, 10, EdgeCapacity::new(5)),
            InputEdge::new(7, 10, EdgeCapacity::new(15)),
            InputEdge::new(8, 10, EdgeCapacity::new(10)),
        ];

        let mut max_flow_solver = PushRelabel::from_edge_list(edges);
        let sources = [9];
        let targets = [10];
        max_flow_solver.run(&sources, &targets);

        // it's OK to expect the solver to have run
        let max_flow = max_flow_solver
            .max_flow()
            .expect("max flow computation did not run");
        assert_eq!(30, max_flow);

        // it's OK to expect the solver to have run
        let assignment = max_flow_solver
            .assignment(&sources)
            .expect("assignment computation did not run");
        assert_eq!(assignment, bits![0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0]);
    }

    #[test]
    fn max_flow_ff() {
        let edges = vec![
            InputEdge::new(0, 1, EdgeCapacity::new(7)),
            InputEdge::new(0, 2, EdgeCapacity::new(3)),
            InputEdge::new(1, 2, EdgeCapacity::new(1)),
            InputEdge::new(1, 3, EdgeCapacity::new(6)),
            InputEdge::new(2, 4, EdgeCapacity::new(8)),
            InputEdge::new(3, 5, EdgeCapacity::new(2)),
            InputEdge::new(3, 2, EdgeCapacity::new(3)),
            InputEdge::new(4, 3, EdgeCapacity::new(2)),
            InputEdge::new(4, 5, EdgeCapacity::new(8)),
        ];

        let mut max_flow_solver = PushRelabel::from_edge_list(edges);
        let sources = [0];
        let targets = [5];
        max_flow_solver.run(&sources, &targets);

        // it's OK to expect the solver to have run
        let max_flow = max_flow_solver
            .max_flow()
            .expect("max flow computation did not run");
        assert_eq!(9, max_flow);

        // it's OK to expect the solver to have run
        let assignment = max_flow_solver
            .assignment(&sources)
            .expect("assignment computation did not run");
        assert_eq!(assignment, bits![1, 1, 0, 1, 0, 0]);
    }

    #[test]
    #[should_panic]
    fn flow_not_computed() {
        let edges = vec![
            InputEdge::new(0, 1, EdgeCapacity::new(7)),
            InputEdge::new(0, 2, EdgeCapacity::new(3)),
            InputEdge::new(1, 2, EdgeCapacity::new(1)),
            InputEdge::new(1, 3, EdgeCapacity::new(6)),
            InputEdge::new(2, 4, EdgeCapacity::new(8)),
            InputEdge::new(3, 5, EdgeCapacity::new(2)),
            InputEdge::new(3, 2, EdgeCapacity::new(3)),
            InputEdge::new(4, 3, EdgeCapacity::new(2)),
            InputEdge::new(4, 5, EdgeCapacity::new(8)),
        ];

        // the expect(.) call is being tested
        PushRelabel::from_edge_list(edges)
            .max_flow()
            .expect("max flow computation did not run");
    }

    #[test]
    #[should_panic]
    fn assignment_not_computed() {
        let edges = vec![
            InputEdge::new(0, 1, EdgeCapacity::new(7)),
            InputEdge::new(0, 2, EdgeCapacity::new(3)),
            InputEdge::new(1, 2, EdgeCapacity::new(1)),
            InputEdge::new(1, 3, EdgeCapacity::new(6)),
            InputEdge::new(2, 4, EdgeCapacity::new(8)),
            InputEdge::new(3, 5, EdgeCapacity::new(2)),
            InputEdge::new(3, 2, EdgeCapacity::new(3)),
            InputEdge::new(4, 3, EdgeCapacity::new(2)),
            InputEdge::new(4, 5, EdgeCapacity::new(8)),
        ];

        // the expect(.) call is being tested
        PushRelabel::from_edge_list(edges)
            .assignment(&[0])
            .expect("assignment computation did not run");
    }
}