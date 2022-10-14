use crate::{
    dfs::DFS,
    edge::{Edge, InputEdge},
    graph::{Graph, NodeID},
    max_flow::{MaxFlow, ResidualCapacity},
    static_graph::StaticGraph,
};
use bitvec::vec::BitVec;
use itertools::Itertools;
use log::{debug, warn};
use std::{
    sync::{
        atomic::{AtomicI32, Ordering},
        Arc,
    },
    time::Instant,
};

pub struct EdmondsKarp {
    residual_graph: StaticGraph<ResidualCapacity>,
    max_flow: i32,
    finished: bool,
    source: NodeID,
    target: NodeID,
    bound: Option<Arc<AtomicI32>>,
}

impl EdmondsKarp {
    // todo(dl): add closure parameter to derive edge data
    pub fn from_generic_edge_list(
        input_edges: Vec<impl Edge<ID = NodeID>>,
        source: usize,
        target: usize,
    ) -> Self {
        let edge_list: Vec<InputEdge<ResidualCapacity>> = input_edges
            .into_iter()
            .map(move |edge| InputEdge {
                source: edge.source(),
                target: edge.target(),
                data: ResidualCapacity::new(1),
            })
            .collect();

        debug!("created {} ff edges", edge_list.len());
        EdmondsKarp::from_edge_list(edge_list, source, target)
    }

    pub fn from_edge_list(
        mut edge_list: Vec<InputEdge<ResidualCapacity>>,
        source: usize,
        target: usize,
    ) -> Self {
        let number_of_edges = edge_list.len();

        debug!("extending {} edges", edge_list.len());
        // blindly generate reverse edges for all edges with zero capacity
        edge_list.extend_from_within(..);
        edge_list.iter_mut().skip(number_of_edges).for_each(|edge| {
            edge.reverse();
            edge.data.capacity = 0;
        });
        debug!("into {} edges", edge_list.len());

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
        debug!("dedup-merged {} edges", edge_list.len());

        // at this point the edge set of the residual graph doesn't have any
        // duplicates anymore. note that this is fine, as we are looking to
        // compute a node partition.
        Self {
            residual_graph: StaticGraph::new(edge_list),
            max_flow: 0,
            finished: false,
            source,
            target,
            bound: None,
        }
    }
}

impl MaxFlow for EdmondsKarp {
    fn run_with_upper_bound(&mut self, bound: Arc<AtomicI32>) {
        warn!("Upper bound {} is discarded", bound.load(Ordering::Relaxed));
        self.bound = Some(bound);
        self.run()
    }

    fn run(&mut self) {
        let mut dfs = DFS::new(
            &[self.source],
            &[self.target],
            self.residual_graph.number_of_nodes(),
        );
        let filter = |graph: &StaticGraph<ResidualCapacity>, edge| graph.data(edge).capacity <= 0;
        // let mut iteration = 0;
        while dfs.run_with_filter(&self.residual_graph, filter) {
            let start = Instant::now();
            // retrieve node path. The path is unambiguous, as we removed all duplicate edges
            // find min capacity on edges of the path
            let bootleneck_head_tail = dfs
                .path_iter()
                .tuple_windows()
                .min_by_key(|(a, b)| {
                    let edge = self.residual_graph.find_edge_unchecked(*b, *a);
                    self.residual_graph.data(edge).capacity
                })
                .expect("graph is broken, couldn't find min edge");
            let duration = start.elapsed();
            debug!(" flow assignment1 took: {:?} (done)", duration);

            let bottleneck_edge = self
                .residual_graph
                .find_edge_unchecked(bootleneck_head_tail.1, bootleneck_head_tail.0);
            debug!("  bottleneck edge: {}", bottleneck_edge);
            let path_flow = self.residual_graph.data(bottleneck_edge).capacity;
            debug_assert!(path_flow > 0);
            debug!("min edge: {}, capacity: {}", bottleneck_edge, path_flow);
            // sum up flow
            self.max_flow += path_flow;
            let duration = start.elapsed();
            debug!(" flow assignment2 took: {:?} (done)", duration);

            // assign flow to residual graph
            for (a, b) in dfs.path_iter().tuple_windows() {
                let rev_edge = self.residual_graph.find_edge_unchecked(a, b);
                let fwd_edge = self.residual_graph.find_edge_unchecked(b, a);

                self.residual_graph.data_mut(fwd_edge).capacity -= path_flow;
                self.residual_graph.data_mut(rev_edge).capacity += path_flow;
            }
            let duration = start.elapsed();
            debug!(" flow assignment3 took: {:?} (done)", duration);
        }

        self.finished = true;
    }

    fn max_flow(&self) -> Result<i32, String> {
        if !self.finished {
            return Err("Assigment was not computed.".to_string());
        }
        Ok(self.max_flow)
    }

    fn assignment(&self, source: NodeID) -> Result<BitVec, String> {
        if !self.finished {
            return Err("Assigment was not computed.".to_string());
        }

        // run a reachability analysis
        let mut reachable = BitVec::with_capacity(self.residual_graph.number_of_nodes());
        reachable.resize(self.residual_graph.number_of_nodes(), false);
        let mut stack = vec![source];
        stack.reserve(self.residual_graph.number_of_nodes());
        reachable.set(source, true);
        while let Some(node) = stack.pop() {
            for edge in self.residual_graph.edge_range(node) {
                let target = self.residual_graph.target(edge);
                let reached = reachable.get(target).unwrap();
                if !reached && self.residual_graph.data(edge).capacity > 0 {
                    stack.push(target);
                    reachable.set(target, true);
                }
            }
        }
        Ok(reachable)
    }
}

#[cfg(test)]
mod tests {

    use crate::edge::InputEdge;
    use crate::edmonds_karp::EdmondsKarp;
    use crate::max_flow::MaxFlow;
    use crate::max_flow::ResidualCapacity;
    use bitvec::bits;
    use bitvec::prelude::Lsb0;

    #[test]
    fn max_flow_clr() {
        let edges = vec![
            InputEdge::new(0, 1, ResidualCapacity::new(16)),
            InputEdge::new(0, 2, ResidualCapacity::new(13)),
            InputEdge::new(1, 2, ResidualCapacity::new(10)),
            InputEdge::new(1, 3, ResidualCapacity::new(12)),
            InputEdge::new(2, 1, ResidualCapacity::new(4)),
            InputEdge::new(2, 4, ResidualCapacity::new(14)),
            InputEdge::new(3, 2, ResidualCapacity::new(9)),
            InputEdge::new(3, 5, ResidualCapacity::new(20)),
            InputEdge::new(4, 3, ResidualCapacity::new(7)),
            InputEdge::new(4, 5, ResidualCapacity::new(4)),
        ];

        let source = 0;
        let target = 5;
        let mut max_flow_solver = EdmondsKarp::from_edge_list(edges, source, target);
        max_flow_solver.run();

        // it's OK to expect the solver to have run
        let max_flow = max_flow_solver
            .max_flow()
            .expect("max flow computation did not run");
        assert_eq!(23, max_flow);

        // it's OK to expect the solver to have run
        let assignment = max_flow_solver
            .assignment(source)
            .expect("assignment computation did not run");

        assert_eq!(assignment, bits![1, 1, 1, 0, 1, 0]);
    }

    #[test]
    fn max_flow_ita() {
        let edges = vec![
            InputEdge::new(0, 1, ResidualCapacity::new(5)),
            InputEdge::new(0, 4, ResidualCapacity::new(7)),
            InputEdge::new(0, 5, ResidualCapacity::new(6)),
            InputEdge::new(1, 2, ResidualCapacity::new(4)),
            InputEdge::new(1, 7, ResidualCapacity::new(3)),
            InputEdge::new(4, 7, ResidualCapacity::new(4)),
            InputEdge::new(4, 6, ResidualCapacity::new(1)),
            InputEdge::new(5, 6, ResidualCapacity::new(5)),
            InputEdge::new(2, 3, ResidualCapacity::new(3)),
            InputEdge::new(7, 3, ResidualCapacity::new(7)),
            InputEdge::new(6, 7, ResidualCapacity::new(1)),
            InputEdge::new(6, 3, ResidualCapacity::new(6)),
        ];

        let source = 0;
        let target = 3;
        let mut max_flow_solver = EdmondsKarp::from_edge_list(edges, source, target);
        max_flow_solver.run();

        // it's OK to expect the solver to have run
        let max_flow = max_flow_solver
            .max_flow()
            .expect("max flow computation did not run");
        assert_eq!(15, max_flow);

        // it's OK to expect the solver to have run
        let assignment = max_flow_solver
            .assignment(source)
            .expect("assignment computation did not run");
        assert_eq!(assignment, bits![1, 0, 0, 0, 1, 1, 0, 0]);
    }

    #[test]
    fn max_flow_yt() {
        let edges = vec![
            InputEdge::new(9, 0, ResidualCapacity::new(5)),
            InputEdge::new(9, 1, ResidualCapacity::new(10)),
            InputEdge::new(9, 2, ResidualCapacity::new(15)),
            InputEdge::new(0, 3, ResidualCapacity::new(10)),
            InputEdge::new(1, 0, ResidualCapacity::new(15)),
            InputEdge::new(1, 4, ResidualCapacity::new(20)),
            InputEdge::new(2, 5, ResidualCapacity::new(25)),
            InputEdge::new(3, 4, ResidualCapacity::new(25)),
            InputEdge::new(3, 6, ResidualCapacity::new(10)),
            InputEdge::new(4, 2, ResidualCapacity::new(5)),
            InputEdge::new(4, 7, ResidualCapacity::new(30)),
            InputEdge::new(5, 7, ResidualCapacity::new(20)),
            InputEdge::new(5, 8, ResidualCapacity::new(10)),
            InputEdge::new(7, 8, ResidualCapacity::new(15)),
            InputEdge::new(6, 10, ResidualCapacity::new(5)),
            InputEdge::new(7, 10, ResidualCapacity::new(15)),
            InputEdge::new(8, 10, ResidualCapacity::new(10)),
        ];

        let source = 9;
        let target = 10;
        let mut max_flow_solver = EdmondsKarp::from_edge_list(edges, source, target);
        max_flow_solver.run();

        // it's OK to expect the solver to have run
        let max_flow = max_flow_solver
            .max_flow()
            .expect("max flow computation did not run");
        assert_eq!(30, max_flow);

        // it's OK to expect the solver to have run
        let assignment = max_flow_solver
            .assignment(source)
            .expect("assignment computation did not run");
        assert_eq!(assignment, bits![0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0]);
    }

    #[test]
    fn max_flow_ff() {
        let edges = vec![
            InputEdge::new(0, 1, ResidualCapacity::new(7)),
            InputEdge::new(0, 2, ResidualCapacity::new(3)),
            InputEdge::new(1, 2, ResidualCapacity::new(1)),
            InputEdge::new(1, 3, ResidualCapacity::new(6)),
            InputEdge::new(2, 4, ResidualCapacity::new(8)),
            InputEdge::new(3, 5, ResidualCapacity::new(2)),
            InputEdge::new(3, 2, ResidualCapacity::new(3)),
            InputEdge::new(4, 3, ResidualCapacity::new(2)),
            InputEdge::new(4, 5, ResidualCapacity::new(8)),
        ];

        let source = 0;
        let target = 5;
        let mut max_flow_solver = EdmondsKarp::from_edge_list(edges, source, target);
        max_flow_solver.run();

        // it's OK to expect the solver to have run
        let max_flow = max_flow_solver
            .max_flow()
            .expect("max flow computation did not run");
        assert_eq!(9, max_flow);

        // it's OK to expect the solver to have run
        let assignment = max_flow_solver
            .assignment(source)
            .expect("assignment computation did not run");
        assert_eq!(assignment, bits![1, 1, 0, 1, 0, 0]);
    }

    #[test]
    #[should_panic]
    fn flow_not_computed() {
        let edges = vec![
            InputEdge::new(0, 1, ResidualCapacity::new(7)),
            InputEdge::new(0, 2, ResidualCapacity::new(3)),
            InputEdge::new(1, 2, ResidualCapacity::new(1)),
            InputEdge::new(1, 3, ResidualCapacity::new(6)),
            InputEdge::new(2, 4, ResidualCapacity::new(8)),
            InputEdge::new(3, 5, ResidualCapacity::new(2)),
            InputEdge::new(3, 2, ResidualCapacity::new(3)),
            InputEdge::new(4, 3, ResidualCapacity::new(2)),
            InputEdge::new(4, 5, ResidualCapacity::new(8)),
        ];

        // the expect(.) call is being tested
        EdmondsKarp::from_edge_list(edges, 0, 1)
            .max_flow()
            .expect("max flow computation did not run");
    }

    #[test]
    #[should_panic]
    fn assignment_not_computed() {
        let edges = vec![
            InputEdge::new(0, 1, ResidualCapacity::new(7)),
            InputEdge::new(0, 2, ResidualCapacity::new(3)),
            InputEdge::new(1, 2, ResidualCapacity::new(1)),
            InputEdge::new(1, 3, ResidualCapacity::new(6)),
            InputEdge::new(2, 4, ResidualCapacity::new(8)),
            InputEdge::new(3, 5, ResidualCapacity::new(2)),
            InputEdge::new(3, 2, ResidualCapacity::new(3)),
            InputEdge::new(4, 3, ResidualCapacity::new(2)),
            InputEdge::new(4, 5, ResidualCapacity::new(8)),
        ];

        // the expect(.) call is being tested
        EdmondsKarp::from_edge_list(edges, 0, 1)
            .assignment(0)
            .expect("assignment computation did not run");
    }
}
