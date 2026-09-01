//! A Max-Flow computation implementing Cherkassky's variant of Dinitz' seminal
//! algorithm. The implementation at hand is distinguished by three factors:
//! 1) Computing the layer graph in a single BFS starting in t.
//! 2) Omitting maintenance of the layer graph.
//! 3) Running the augmentation phase as a single DFS.
//!
//! The DFS restarts after it found an augmenting path on the tail of the
//! saturated edge that is closest to the source.
use crate::{
    edge::InputEdge,
    graph::{EdgeID, Graph, NodeID},
    max_flow::{MaxFlow, ResidualArcData, ResidualEdgeData, residual_graph_of},
    static_graph::StaticGraph,
};
use bitvec::vec::BitVec;
use core::cmp::{max, min};
use log::debug;
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicI32, Ordering},
    },
};

pub struct Dinic {
    residual_graph: StaticGraph<ResidualArcData>,
    max_flow: i32,
    finished: bool,
    level: Vec<usize>,
    parents: Vec<NodeID>,
    /// arc on which the DFS entered a node, i.e. the arc (parents[v], v)
    parent_edge: Vec<u32>,
    stack: Vec<(NodeID, i32)>,
    dfs_count: usize,
    bfs_count: usize,
    queue: VecDeque<NodeID>,
    source: NodeID,
    target: NodeID,
    bound: Option<Arc<AtomicI32>>,
}

impl Dinic {
    fn bfs(&mut self) -> bool {
        self.bfs_count += 1;
        // init
        self.level.fill(usize::MAX);
        self.level[self.target] = 0;

        self.queue.clear();
        self.queue.push_back(self.target);

        // label residual graph nodes in BFS order, but in reverse starting from the target
        while let Some(u) = self.queue.pop_front() {
            for edge in self.residual_graph.edge_range(u) {
                let v = self.residual_graph.target(edge);
                if v != self.source && self.level[v] != usize::MAX {
                    // node v is not the source, and is already visited. Note the source can be reached multiple times
                    continue;
                }

                // check capacity of reverse edge
                let edge_capacity = self.residual_graph.data(edge).reverse_capacity;
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
        debug!(
            "BFS run {}, upper bound on path length: {}",
            self.bfs_count, self.level[self.source]
        );
        self.level[self.source] != usize::MAX
    }

    fn dfs(&mut self) -> i32 {
        self.dfs_count += 1;
        self.stack.clear();
        self.stack.push((self.source, i32::MAX));

        self.parents.fill(NodeID::MAX);
        self.parents[self.source] = self.source;

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
                if available_capacity == 0 {
                    // no capacity to use on this edge
                    continue;
                }
                self.parents[v] = u;
                self.parent_edge[v] = edge as u32;
                let flow = min(flow, available_capacity);
                if v == self.target {
                    // The bottleneck that the stack carries is an upper bound
                    // rather than the capacity of the path: an earlier
                    // augmentation of this very DFS may have taken capacity off
                    // the prefix that both paths share. Walk the path once to
                    // find what it has left before assigning it, or the arcs of
                    // the prefix end up oversubscribed.
                    let mut flow = flow; // mutable shadow
                    let mut node = v;
                    while self.parents[node] != node {
                        let arc = self.parent_edge[node] as EdgeID;
                        flow = min(flow, self.residual_graph.data(arc).capacity);
                        node = self.parents[node];
                    }
                    debug_assert!(flow > 0, "the augmenting path carries no flow");

                    // reached a target. Unpack path in reverse order, assign flow
                    let mut v = v; // mutable shadow
                    let mut closest_tail = u;
                    loop {
                        let u = self.parents[v];
                        if u == v {
                            break;
                        }
                        let fwd_edge = self.parent_edge[v] as EdgeID;
                        let residual = self.residual_graph.data_mut(fwd_edge);
                        residual.capacity -= flow;
                        residual.reverse_capacity += flow;
                        if 0 == residual.capacity {
                            closest_tail = u;
                        }
                        // keep the cached capacities of the arc pair in sync
                        let rev_edge = self
                            .residual_graph
                            .find_edge_sorted(v, u)
                            .expect("residual graph is not symmetric");
                        let residual = self.residual_graph.data_mut(rev_edge);
                        residual.capacity += flow;
                        residual.reverse_capacity -= flow;
                        v = u;
                    }

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

        blocking_flow
    }
}

impl MaxFlow for Dinic {
    fn from_edge_list(
        edge_list: Vec<InputEdge<ResidualEdgeData>>,
        source: NodeID,
        target: NodeID,
    ) -> Self {
        debug_assert!(!edge_list.is_empty());
        let number_of_nodes = 1 + edge_list
            .iter()
            .map(|edge| max(edge.source, edge.target))
            .max()
            .expect("edge list is empty");
        let residual_graph = residual_graph_of(edge_list);

        Self {
            residual_graph,
            max_flow: 0,
            finished: false,
            level: Vec::with_capacity(number_of_nodes),
            parents: Vec::with_capacity(number_of_nodes),
            parent_edge: Vec::with_capacity(number_of_nodes),
            stack: Vec::with_capacity(number_of_nodes),
            dfs_count: 0,
            bfs_count: 0,
            queue: VecDeque::with_capacity(number_of_nodes),
            source,
            target,
            bound: None,
        }
    }

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
        self.parent_edge.resize(number_of_nodes, 0);
        self.level.resize(number_of_nodes, usize::MAX);

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
            return Err("Assignment was not computed.".to_string());
        }
        debug!(
            "finished in {} DFS, and {} BFS runs",
            self.dfs_count, self.bfs_count
        );
        Ok(self.max_flow)
    }

    fn assignment(&self, source: NodeID) -> Result<BitVec, String> {
        if !self.finished {
            return Err("Assignment was not computed.".to_string());
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
    use crate::edge::EdgeData;
    use crate::edge::InputEdge;
    use crate::edmonds_karp::EdmondsKarp;
    use crate::max_flow::MaxFlow;
    use crate::max_flow::ResidualEdgeData;
    use bitvec::bits;
    use bitvec::prelude::Lsb0;
    use rand::{RngExt, SeedableRng, prelude::StdRng};

    /// A random layered graph. Its depth forces the solver through a number of
    /// phases, and the arcs that skip and lead back a layer keep the layer graph
    /// from being the layering the graph was built with.
    fn layered_graph(
        rng: &mut StdRng,
        width: usize,
        depth: usize,
    ) -> (Vec<InputEdge<ResidualEdgeData>>, usize, usize) {
        let source = 0;
        let target = 1 + width * depth;
        let node = |layer: usize, index: usize| 1 + layer * width + index;

        let mut edges = Vec::new();
        for index in 0..width {
            edges.push(InputEdge::new(
                source,
                node(0, index),
                ResidualEdgeData::new(rng.random_range(1..=8)),
            ));
            edges.push(InputEdge::new(
                node(depth - 1, index),
                target,
                ResidualEdgeData::new(rng.random_range(1..=8)),
            ));
        }
        for layer in 0..depth - 1 {
            for index in 0..width {
                for other in 0..width {
                    if rng.random_range(0..100) < 40 {
                        edges.push(InputEdge::new(
                            node(layer, index),
                            node(layer + 1, other),
                            ResidualEdgeData::new(rng.random_range(1..=5)),
                        ));
                    }
                }
                if layer + 2 < depth && rng.random_range(0..100) < 20 {
                    edges.push(InputEdge::new(
                        node(layer, index),
                        node(layer + 2, rng.random_range(0..width)),
                        ResidualEdgeData::new(rng.random_range(1..=5)),
                    ));
                }
                if layer > 0 && rng.random_range(0..100) < 20 {
                    edges.push(InputEdge::new(
                        node(layer, index),
                        node(layer - 1, rng.random_range(0..width)),
                        ResidualEdgeData::new(rng.random_range(1..=5)),
                    ));
                }
            }
        }
        (edges, source, target)
    }

    /// The capacity of the cut that `assignment` induces on `edges`.
    fn cut_capacity(
        edges: &[InputEdge<ResidualEdgeData>],
        assignment: &bitvec::vec::BitVec,
    ) -> i32 {
        edges
            .iter()
            .filter(|edge| assignment[edge.source] && !assignment[edge.target])
            .map(|edge| edge.data.capacity)
            .sum()
    }

    /// Capacities above one are what makes the bottleneck of an augmenting path
    /// a quantity of its own, and a solver that hands a path more flow than it
    /// has left overstates the result. The small graphs below all carry a
    /// bottleneck of one and cannot tell.
    #[test]
    fn max_flow_matches_edmonds_karp_on_random_graphs() {
        let mut rng = StdRng::seed_from_u64(0x5EED);
        for round in 0..25 {
            let (edges, source, target) = layered_graph(&mut rng, 4 + round % 5, 4 + round % 7);

            let mut reference = EdmondsKarp::from_edge_list(edges.clone(), source, target);
            reference.run();
            let expected = reference
                .max_flow()
                .expect("max flow computation did not run");

            let mut solver = Dinic::from_edge_list(edges.clone(), source, target);
            solver.run();
            let max_flow = solver.max_flow().expect("max flow computation did not run");

            assert_eq!(max_flow, expected, "round {round}");

            // the assignment has to be a minimum cut, i.e. one of capacity equal
            // to the value of the flow
            let assignment = solver
                .assignment(source)
                .expect("assignment computation did not run");
            assert!(assignment[source], "round {round}");
            assert!(!assignment[target], "round {round}");
            assert_eq!(cut_capacity(&edges, &assignment), max_flow, "round {round}");
        }
    }

    /// The shape that chipper hands to the solver: a grid whose extreme rows are
    /// contracted into the source and the target, which turns the arcs of those
    /// rows into arcs of a capacity well above one.
    #[test]
    fn max_flow_matches_edmonds_karp_on_contracted_grids() {
        for (width, height) in [(6, 8), (9, 12), (13, 7)] {
            let contracted = height / 4;
            let id = |row: usize, column: usize| {
                if row < contracted {
                    0
                } else if row >= height - contracted {
                    1
                } else {
                    2 + (row - contracted) * width + column
                }
            };

            let mut edges = Vec::new();
            let mut push = |s: usize, t: usize| {
                if s != t {
                    edges.push(InputEdge::new(s, t, ResidualEdgeData::new(1)));
                    edges.push(InputEdge::new(t, s, ResidualEdgeData::new(1)));
                }
            };
            for row in 0..height {
                for column in 0..width {
                    if column + 1 < width {
                        push(id(row, column), id(row, column + 1));
                    }
                    if row + 1 < height {
                        push(id(row, column), id(row + 1, column));
                    }
                }
            }

            let mut reference = EdmondsKarp::from_edge_list(edges.clone(), 0, 1);
            reference.run();
            let expected = reference
                .max_flow()
                .expect("max flow computation did not run");

            let mut solver = Dinic::from_edge_list(edges.clone(), 0, 1);
            solver.run();
            let max_flow = solver.max_flow().expect("max flow computation did not run");

            assert_eq!(max_flow, expected, "grid {width}x{height}");
            let assignment = solver
                .assignment(0)
                .expect("assignment computation did not run");
            assert_eq!(
                cut_capacity(&edges, &assignment),
                max_flow,
                "grid {width}x{height}"
            );
        }
    }

    #[test]
    fn max_flow_clr() {
        let edges = vec![
            InputEdge::new(0, 1, ResidualEdgeData::new(16)),
            InputEdge::new(0, 2, ResidualEdgeData::new(13)),
            InputEdge::new(1, 2, ResidualEdgeData::new(10)),
            InputEdge::new(1, 3, ResidualEdgeData::new(12)),
            InputEdge::new(2, 1, ResidualEdgeData::new(4)),
            InputEdge::new(2, 4, ResidualEdgeData::new(14)),
            InputEdge::new(3, 2, ResidualEdgeData::new(9)),
            InputEdge::new(3, 5, ResidualEdgeData::new(20)),
            InputEdge::new(4, 3, ResidualEdgeData::new(7)),
            InputEdge::new(4, 5, ResidualEdgeData::new(4)),
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
    fn max_flow_ita_from_generic_edge_list() {
        let edges = vec![
            InputEdge::new(0, 1, 5),
            InputEdge::new(0, 4, 7),
            InputEdge::new(0, 5, 6),
            InputEdge::new(1, 2, 4),
            InputEdge::new(1, 7, 3),
            InputEdge::new(4, 7, 4),
            InputEdge::new(4, 6, 1),
            InputEdge::new(5, 6, 5),
            InputEdge::new(2, 3, 3),
            InputEdge::new(7, 3, 7),
            InputEdge::new(6, 7, 1),
            InputEdge::new(6, 3, 6),
        ];

        let source = 0;
        let target = 3;
        let mut max_flow_solver = Dinic::from_generic_edge_list(&edges, source, target, |edge| {
            ResidualEdgeData::new(*edge.data())
        });
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
    fn max_flow_ita() {
        let edges = vec![
            InputEdge::new(0, 1, ResidualEdgeData::new(5)),
            InputEdge::new(0, 4, ResidualEdgeData::new(7)),
            InputEdge::new(0, 5, ResidualEdgeData::new(6)),
            InputEdge::new(1, 2, ResidualEdgeData::new(4)),
            InputEdge::new(1, 7, ResidualEdgeData::new(3)),
            InputEdge::new(4, 7, ResidualEdgeData::new(4)),
            InputEdge::new(4, 6, ResidualEdgeData::new(1)),
            InputEdge::new(5, 6, ResidualEdgeData::new(5)),
            InputEdge::new(2, 3, ResidualEdgeData::new(3)),
            InputEdge::new(7, 3, ResidualEdgeData::new(7)),
            InputEdge::new(6, 7, ResidualEdgeData::new(1)),
            InputEdge::new(6, 3, ResidualEdgeData::new(6)),
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
            InputEdge::new(9, 0, ResidualEdgeData::new(5)),
            InputEdge::new(9, 1, ResidualEdgeData::new(10)),
            InputEdge::new(9, 2, ResidualEdgeData::new(15)),
            InputEdge::new(0, 3, ResidualEdgeData::new(10)),
            InputEdge::new(1, 0, ResidualEdgeData::new(15)),
            InputEdge::new(1, 4, ResidualEdgeData::new(20)),
            InputEdge::new(2, 5, ResidualEdgeData::new(25)),
            InputEdge::new(3, 4, ResidualEdgeData::new(25)),
            InputEdge::new(3, 6, ResidualEdgeData::new(10)),
            InputEdge::new(4, 2, ResidualEdgeData::new(5)),
            InputEdge::new(4, 7, ResidualEdgeData::new(30)),
            InputEdge::new(5, 7, ResidualEdgeData::new(20)),
            InputEdge::new(5, 8, ResidualEdgeData::new(10)),
            InputEdge::new(7, 8, ResidualEdgeData::new(15)),
            InputEdge::new(6, 10, ResidualEdgeData::new(5)),
            InputEdge::new(7, 10, ResidualEdgeData::new(15)),
            InputEdge::new(8, 10, ResidualEdgeData::new(10)),
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
            InputEdge::new(0, 1, ResidualEdgeData::new(7)),
            InputEdge::new(0, 2, ResidualEdgeData::new(3)),
            InputEdge::new(1, 2, ResidualEdgeData::new(1)),
            InputEdge::new(1, 3, ResidualEdgeData::new(6)),
            InputEdge::new(2, 4, ResidualEdgeData::new(8)),
            InputEdge::new(3, 5, ResidualEdgeData::new(2)),
            InputEdge::new(3, 2, ResidualEdgeData::new(3)),
            InputEdge::new(4, 3, ResidualEdgeData::new(2)),
            InputEdge::new(4, 5, ResidualEdgeData::new(8)),
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
            InputEdge::new(0, 1, ResidualEdgeData::new(7)),
            InputEdge::new(0, 2, ResidualEdgeData::new(3)),
            InputEdge::new(1, 2, ResidualEdgeData::new(1)),
            InputEdge::new(1, 3, ResidualEdgeData::new(6)),
            InputEdge::new(2, 4, ResidualEdgeData::new(8)),
            InputEdge::new(3, 5, ResidualEdgeData::new(2)),
            InputEdge::new(3, 2, ResidualEdgeData::new(3)),
            InputEdge::new(4, 3, ResidualEdgeData::new(2)),
            InputEdge::new(4, 5, ResidualEdgeData::new(8)),
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
            InputEdge::new(0, 1, ResidualEdgeData::new(7)),
            InputEdge::new(0, 2, ResidualEdgeData::new(3)),
            InputEdge::new(1, 2, ResidualEdgeData::new(1)),
            InputEdge::new(1, 3, ResidualEdgeData::new(6)),
            InputEdge::new(2, 4, ResidualEdgeData::new(8)),
            InputEdge::new(3, 5, ResidualEdgeData::new(2)),
            InputEdge::new(3, 2, ResidualEdgeData::new(3)),
            InputEdge::new(4, 3, ResidualEdgeData::new(2)),
            InputEdge::new(4, 5, ResidualEdgeData::new(8)),
        ];

        // the expect(.) call is being tested
        Dinic::from_edge_list(edges, 1, 2)
            .assignment(1)
            .expect("assignment computation did not run");
    }
}
