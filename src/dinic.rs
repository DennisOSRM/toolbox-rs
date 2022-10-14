//! A Max-Flow computation implementing Cherkassky's variant of Dinitz' seminal
//! algorithm. The implementation at hand is distinguished by three factors:
//! 1) Computing the layer graph in a single BFS starting in t.
//! 2) Omitting maintenance of the layer graph.
//! 3) Running the augmentation phase as a single DFS.
//!
//! The DFS restarts after it found an augmenting path on the tail of the
//! saturated edge that is closest to the source.
use crate::{
    edge::{Edge, InputEdge},
    graph::{Graph, NodeID},
    max_flow::{MaxFlow, ResidualCapacity},
    static_graph::StaticGraph,
};
use bitvec::vec::BitVec;
use core::cmp::min;
use log::debug;
use std::{
    collections::VecDeque,
    sync::{
        atomic::{AtomicI32, Ordering},
        Arc,
    },
    time::Instant,
};

pub struct Dinic {
    residual_graph: StaticGraph<ResidualCapacity>,
    max_flow: i32,
    finished: bool,
    level: Vec<usize>,
    parents: Vec<NodeID>,
    stack: Vec<(NodeID, i32)>,
    dfs_count: usize,
    bfs_count: usize,
    queue: VecDeque<NodeID>,
    source: NodeID,
    target: NodeID,
    bound: Option<Arc<AtomicI32>>,
}

impl Dinic {
    // todo(dl): add closure parameter to derive edge data
    pub fn from_generic_edge_list(
        input_edges: &[impl Edge<ID = NodeID>],
        source: NodeID,
        target: NodeID,
    ) -> Self {
        debug_assert!(!input_edges.is_empty());
        debug!("instantiating max-flow solver");
        let edge_list: Vec<InputEdge<ResidualCapacity>> = input_edges
            .iter()
            .map(move |edge| InputEdge {
                source: edge.source(),
                target: edge.target(),
                data: ResidualCapacity::new(1),
            })
            .collect();

        debug!("created {} ff edges", edge_list.len());
        Dinic::from_edge_list(edge_list, source, target)
    }

    pub fn from_edge_list(
        mut edge_list: Vec<InputEdge<ResidualCapacity>>,
        source: usize,
        target: usize,
    ) -> Self {
        debug_assert!(!edge_list.is_empty());
        let number_of_edges = edge_list.len();

        debug!("extending {} edges", edge_list.len());
        // blindly generate reverse edges for all edges with zero capacity
        edge_list.extend_from_within(..);
        debug!("into {} edges", edge_list.len());

        edge_list.iter_mut().skip(number_of_edges).for_each(|edge| {
            edge.reverse();
            edge.data.capacity = 0;
        });
        debug!("sorting after reversing");

        // dedup-merge edge set, by using the following trick: not the dedup(.) call
        // below takes the second argument as mut. When deduping equivalent values
        // a and b, then a is accumulated onto b.
        edge_list.sort_unstable_by_key(|a| {
            // returning a tuple yields a lexical sort
            (a.source, a.target)
        });
        debug!("start dedup");
        edge_list.dedup_by(|a, mut b| {
            // edges a and b are assumed to be equivalent in the residual graph if
            // (and only if) they are parallel. In other words, this removes parallel
            // edges in the residual graph and accumulates capacities on the remaining
            // egde.
            let edges_are_parallel = a.is_parallel_to(b);
            if edges_are_parallel {
                b.data.capacity += a.data.capacity;
            }
            edges_are_parallel
        });
        edge_list.shrink_to_fit();
        debug!("dedup done");

        // at this point the edge set of the residual graph doesn't have any
        // duplicates anymore. Note that this is fine, as we are looking to
        // compute a node partition.
        Self {
            residual_graph: StaticGraph::new_from_sorted_list(edge_list),
            max_flow: 0,
            finished: false,
            level: Vec::new(),
            parents: Vec::new(),
            stack: Vec::new(),
            dfs_count: 0,
            bfs_count: 0,
            queue: VecDeque::new(),
            source,
            target,
            bound: None,
        }
    }

    // create layer graph L^(s,t) by doing a reverse BFS from target to source.
    // All paths from s to t in the layer graph are then 'downhill'
    fn bfs(&mut self) -> bool {
        let start = Instant::now();
        self.bfs_count += 1;
        // init
        self.level.fill(usize::MAX);
        self.queue.clear();
        self.queue.push_back(self.target);
        self.level[self.target] = 0;

        let duration = start.elapsed();
        debug!("BFS init: {:?}", duration);

        // label residual graph nodes in BFS order, but in reverse starting from the target
        while let Some(u) = self.queue.pop_front() {
            for edge in self.residual_graph.edge_range(u) {
                let v = self.residual_graph.target(edge);
                if v != self.source && self.level[v] != usize::MAX {
                    // node v is not the source, but it is already visited. the source might be reached multiple times
                    continue;
                }

                // fetch capacity of reverse edge
                let rev_edge = self.residual_graph.find_edge_unchecked(v, u);
                let edge_capacity = self.residual_graph.data(rev_edge).capacity;
                if edge_capacity < 1 {
                    // no capacity to use on this edge
                    continue;
                }
                self.level[v] = self.level[u] + 1;
                if v != self.source {
                    self.queue.push_back(v);
                }
            }
        }
        let duration = start.elapsed();
        debug!(
            "BFS took: {:?}, upper bound on path length: {}",
            duration, self.level[self.source]
        );
        self.level[self.source] != usize::MAX
    }

    fn dfs(&mut self) -> i32 {
        let start = Instant::now();
        self.dfs_count += 1;
        self.stack.clear();
        self.parents.fill(NodeID::MAX);

        self.stack.push((self.source, i32::MAX));
        self.parents[self.source] = self.source;

        let duration = start.elapsed();
        debug!(" DFS init2: {:?}", duration);
        let mut blocking_flow = 0;
        while let Some((u, flow)) = self.stack.pop() {
            for edge in self.residual_graph.edge_range(u) {
                let v = self.residual_graph.target(edge);
                if self.parents[v] != NodeID::MAX {
                    // v already in queue
                    continue;
                }
                if self.level[u] < self.level[v] {
                    // edge is not leading to target on a path in the BFS tree
                    continue;
                }
                let available_capacity = self.residual_graph.data(edge).capacity;
                if available_capacity < 1 {
                    // no capacity to use on this edge
                    continue;
                }
                self.parents[v] = u;
                let flow = min(flow, available_capacity);
                if v == self.target {
                    let duration = start.elapsed();
                    debug!(" reached target {}: {:?}", v, duration);
                    // reached a target. Unpack path in reverse order, assign flow
                    let mut v = v; // mutable shadow
                    let mut closest_tail = u;
                    loop {
                        let u = self.parents[v];
                        if u == v {
                            break;
                        }
                        let fwd_edge = self.residual_graph.find_edge_unchecked(u, v);
                        self.residual_graph.data_mut(fwd_edge).capacity -= flow;
                        if 0 == self.residual_graph.data_mut(fwd_edge).capacity {
                            closest_tail = u;
                        }
                        let rev_edge = self.residual_graph.find_edge_unchecked(v, u);
                        self.residual_graph.data_mut(rev_edge).capacity += flow;
                        v = u;
                    }
                    let duration = start.elapsed();
                    debug!(" augmentation took: {:?}", duration);

                    // unwind stack till tail node, then continue the search
                    let before = self.stack.len();
                    while let Some((node, _)) = self.stack.pop() {
                        if self.parents[node] == closest_tail {
                            break; // while let
                        }
                    }
                    blocking_flow += flow;
                    debug!(" stack len before: {before}, after: {}", self.stack.len());

                    // make target reachable again
                    self.parents[self.target] = NodeID::MAX;
                    self.dfs_count += 1;

                    break; // for edge
                } else {
                    self.stack.push((v, flow));
                }
            }
        }

        let duration = start.elapsed();
        debug!("DFS took: {:?} (unsuccessful)", duration);
        // None
        blocking_flow
    }
}

impl MaxFlow for Dinic {
    fn run_with_upper_bound(&mut self, bound: Arc<AtomicI32>) {
        debug!("upper bound: {}", bound.load(Ordering::Relaxed));

        self.bound = Some(bound);
        self.run()
    }

    fn run(&mut self) {
        debug!(
            "residual graph size: V {}, E {}",
            self.residual_graph.number_of_nodes(),
            self.residual_graph.number_of_edges()
        );

        let number_of_nodes = self.residual_graph.number_of_nodes();
        self.parents.resize(number_of_nodes, 0);
        self.level.resize(number_of_nodes, usize::MAX);
        self.queue.reserve(number_of_nodes);

        let mut flow = 0;
        while self.bfs() {
            flow += self.dfs();
            if let Some(bound) = &self.bound {
                // break early if an upper bound is known to the computation
                if flow > bound.load(Ordering::Relaxed) {
                    debug!("aborting max flow computation at {flow}");
                    self.max_flow = flow;
                    return;
                }
            }
        }
        if let Some(bound) = &self.bound {
            bound.fetch_min(flow, Ordering::Relaxed);
        }
        self.max_flow = flow;
        self.finished = true;
    }
    fn max_flow(&self) -> Result<i32, String> {
        if !self.finished {
            return Err("Assigment was not computed.".to_string());
        }
        debug!(
            "finished in {} DFS, and {} BFS runs",
            self.dfs_count, self.bfs_count
        );
        Ok(self.max_flow)
    }

    fn assignment(&self, source: NodeID) -> Result<BitVec, String> {
        if !self.finished {
            return Err("Assigment was not computed.".to_string());
        }

        // run a reachability analysis
        let mut reachable = BitVec::new();
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

    use crate::dinic::Dinic;
    use crate::edge::InputEdge;
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
        let mut max_flow_solver = Dinic::from_edge_list(edges, source, target);
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
        let mut max_flow_solver = Dinic::from_edge_list(edges, source, target);
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
        let mut max_flow_solver = Dinic::from_edge_list(edges, source, target);
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
        let mut max_flow_solver = Dinic::from_edge_list(edges, source, target);
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
        Dinic::from_edge_list(edges, 1, 2)
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
        Dinic::from_edge_list(edges, 1, 2)
            .assignment(1)
            .expect("assignment computation did not run");
    }
}
