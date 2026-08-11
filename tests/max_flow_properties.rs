//! Property and differential tests for the max-flow solvers, built for the
//! incremental level repair work of issue #545.
//!
//! Three things are checked for every generated instance:
//!
//! 1. all solvers agree on the flow value
//! 2. the set each solver returns is a valid minimum cut, which is the property
//!    the partitioner actually depends on, since it never looks at the flow
//! 3. the answer does not change between two runs on the same input
//!
//! Instances whose answer is known analytically are checked against it as well,
//! so the suite does not only test the solvers against each other.
use std::sync::mpsc;
use std::time::Duration;
use toolbox_rs::{
    dinic::Dinic,
    edge::InputEdge,
    edmonds_karp::EdmondsKarp,
    ford_fulkerson::FordFulkerson,
    graph::NodeID,
    max_flow::{MaxFlow, ResidualEdgeData},
};

type Edges = Vec<InputEdge<ResidualEdgeData>>;

/// A small deterministic generator, so that a failing case can be reproduced
/// from its seed alone without pulling in a dependency.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Self {
        Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1)
    }
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

fn edge(source: NodeID, target: NodeID, capacity: i32) -> InputEdge<ResidualEdgeData> {
    InputEdge::new(source, target, ResidualEdgeData::new(capacity))
}

/// Checks that `reachable` really describes a minimum cut of value `flow`:
/// the source is inside, the target is outside, and the total capacity of the
/// arcs leaving the set matches the flow value.
fn assert_valid_min_cut(
    case: &str,
    solver: &str,
    edges: &Edges,
    reachable: &bitvec::vec::BitVec,
    source: NodeID,
    target: NodeID,
    flow: i32,
) {
    assert!(
        reachable[source],
        "{case}/{solver}: source is not on the source side of the cut"
    );
    assert!(
        !reachable[target],
        "{case}/{solver}: target is on the source side of the cut"
    );

    let mut crossing = 0;
    for edge in edges {
        if reachable[edge.source] && !reachable[edge.target] {
            crossing += edge.data.capacity;
        }
    }
    assert_eq!(
        crossing, flow,
        "{case}/{solver}: capacity leaving the cut is {crossing} but the flow is {flow}"
    );
}

/// How long a single instance of this size may take. These graphs are tiny, so
/// anything approaching this means the solver is not converging.
const SOLVER_TIMEOUT: Duration = Duration::from_secs(20);

/// Runs a solver on its own thread and fails the test if it does not finish.
///
/// A solver that loses track of its residual capacities does not usually return
/// a wrong answer, it stops terminating, and incremental level repair has an
/// orphan loop that can do exactly that. Without this the suite hangs instead
/// of reporting a failure, which is useless in CI.
fn solve<T: MaxFlow + Send + 'static>(
    case: &str,
    solver_name: &str,
    edges: &Edges,
    source: NodeID,
    target: NodeID,
) -> (i32, bitvec::vec::BitVec) {
    let (sender, receiver) = mpsc::channel();
    let edges = edges.clone();
    let worker = std::thread::spawn(move || {
        let mut solver = T::from_edge_list(edges, source, target);
        solver.run();
        let flow = solver.max_flow().expect("solver did not run");
        let assignment = solver.assignment(source).expect("solver did not run");
        let _ = sender.send((flow, assignment));
    });
    match receiver.recv_timeout(SOLVER_TIMEOUT) {
        Ok(result) => {
            worker.join().expect("solver thread panicked");
            result
        }
        Err(_) => panic!(
            "{case}/{solver_name}: did not finish within {SOLVER_TIMEOUT:?}, \
             the solver is not converging"
        ),
    }
}

/// Runs every solver on one instance and checks agreement, cut validity and
/// determinism. `expected` is the analytically known flow where there is one.
fn check(case: &str, edges: &Edges, source: NodeID, target: NodeID, expected: Option<i32>) {
    let (dinic_flow, dinic_cut) = solve::<Dinic>(case, "Dinic", edges, source, target);
    let (karp_flow, karp_cut) = solve::<EdmondsKarp>(case, "Edmonds-Karp", edges, source, target);
    let (fulkerson_flow, fulkerson_cut) =
        solve::<FordFulkerson>(case, "Ford-Fulkerson", edges, source, target);

    assert_eq!(
        dinic_flow, karp_flow,
        "{case}: Dinic says {dinic_flow}, Edmonds-Karp says {karp_flow}"
    );
    assert_eq!(
        dinic_flow, fulkerson_flow,
        "{case}: Dinic says {dinic_flow}, Ford-Fulkerson says {fulkerson_flow}"
    );
    if let Some(expected) = expected {
        assert_eq!(dinic_flow, expected, "{case}: known flow is {expected}");
    }

    assert_valid_min_cut(case, "Dinic", edges, &dinic_cut, source, target, dinic_flow);
    assert_valid_min_cut(
        case,
        "Edmonds-Karp",
        edges,
        &karp_cut,
        source,
        target,
        karp_flow,
    );
    assert_valid_min_cut(
        case,
        "Ford-Fulkerson",
        edges,
        &fulkerson_cut,
        source,
        target,
        fulkerson_flow,
    );

    // the partitioner compares partitions between runs, so a solver that is
    // not reproducible would make every later measurement meaningless
    let (again_flow, again_cut) = solve::<Dinic>(case, "Dinic", edges, source, target);
    assert_eq!(dinic_flow, again_flow, "{case}: Dinic is not deterministic");
    assert_eq!(
        dinic_cut, again_cut,
        "{case}: Dinic's cut is not deterministic"
    );
}

/// A random sparse digraph on `n` nodes with unit capacities, the shape the
/// partitioner actually feeds the solver.
fn random_sparse(n: usize, arcs: usize, seed: u64) -> Edges {
    let mut rng = Rng::new(seed);
    let mut edges = Vec::new();
    // a path from 0 to n-1 keeps the instance interesting rather than trivially
    // disconnected
    for u in 0..n - 1 {
        edges.push(edge(u, u + 1, 1));
        edges.push(edge(u + 1, u, 1));
    }
    for _ in 0..arcs {
        let u = rng.below(n);
        let v = rng.below(n);
        if u != v {
            edges.push(edge(u, v, 1));
            edges.push(edge(v, u, 1));
        }
    }
    edges
}

/// A grid with unit capacities in both directions, the closest small stand-in
/// for a road network.
fn grid(width: usize, height: usize) -> Edges {
    let id = |x: usize, y: usize| y * width + x;
    let mut edges = Vec::new();
    for y in 0..height {
        for x in 0..width {
            if x + 1 < width {
                edges.push(edge(id(x, y), id(x + 1, y), 1));
                edges.push(edge(id(x + 1, y), id(x, y), 1));
            }
            if y + 1 < height {
                edges.push(edge(id(x, y), id(x, y + 1), 1));
                edges.push(edge(id(x, y + 1), id(x, y), 1));
            }
        }
    }
    edges
}

/// Two cliques joined by exactly `bridges` unit arcs. Internal arcs are far too
/// expensive to cut, so the minimum cut is the bridge count and the answer is
/// known without consulting another solver.
fn two_clusters(size: usize, bridges: usize) -> (Edges, NodeID, NodeID) {
    assert!(bridges <= size);
    let heavy = 1000;
    let mut edges = Vec::new();
    for a in 0..size {
        for b in 0..size {
            if a != b {
                edges.push(edge(a, b, heavy));
                edges.push(edge(size + a, size + b, heavy));
            }
        }
    }
    for i in 0..bridges {
        edges.push(edge(i, size + i, 1));
        edges.push(edge(size + i, i, 1));
    }
    (edges, 0, size)
}

#[test]
fn known_answer_two_clusters() {
    for size in [3, 5, 8] {
        for bridges in 1..=size {
            let (edges, source, target) = two_clusters(size, bridges);
            check(
                &format!("two clusters size {size} bridges {bridges}"),
                &edges,
                source,
                target,
                Some(bridges as i32),
            );
        }
    }
}

#[test]
fn grids() {
    for (width, height) in [(2, 2), (3, 3), (4, 6), (7, 5), (10, 10)] {
        let edges = grid(width, height);
        // the corner opposite the source has two incident arcs in a grid, so
        // for any grid larger than one cell the cut is at most two
        check(
            &format!("grid {width}x{height}"),
            &edges,
            0,
            width * height - 1,
            Some(2.min(width * height - 1) as i32),
        );
    }
}

#[test]
fn random_sparse_graphs() {
    for seed in 0..64 {
        let n = 12 + (seed as usize % 40);
        let edges = random_sparse(n, n * 2, seed);
        check(&format!("random n {n} seed {seed}"), &edges, 0, n - 1, None);
    }
}

#[test]
fn degenerate_shapes() {
    // a single arc
    check("single arc", &vec![edge(0, 1, 1)], 0, 1, Some(1));

    // parallel arcs, which the solver merges internally
    check(
        "parallel arcs",
        &vec![edge(0, 1, 1), edge(0, 1, 1), edge(0, 1, 1)],
        0,
        1,
        Some(3),
    );

    // a self loop carries no flow and must not be counted
    check(
        "self loop",
        &vec![edge(0, 0, 5), edge(0, 1, 1), edge(1, 1, 7)],
        0,
        1,
        Some(1),
    );

    // source and target in disconnected components
    check(
        "disconnected",
        &vec![edge(0, 2, 1), edge(2, 0, 1), edge(1, 3, 1), edge(3, 1, 1)],
        0,
        1,
        Some(0),
    );

    // a bottleneck of one in the middle of a wide graph
    check(
        "bottleneck",
        &vec![
            edge(0, 1, 5),
            edge(0, 2, 5),
            edge(1, 3, 1),
            edge(2, 3, 5),
            edge(3, 4, 1),
            edge(4, 5, 5),
        ],
        0,
        5,
        Some(1),
    );
}

#[test]
fn unit_capacity_random_graphs_only() {
    // the partitioner only ever hands the solver unit capacities, so this is
    // the case that has to be airtight
    for seed in 100..140 {
        let n = 20 + (seed as usize % 30);
        let mut edges = random_sparse(n, n * 3, seed);
        for e in &mut edges {
            assert_eq!(e.data.capacity, 1);
        }
        edges.push(edge(0, n - 1, 1));
        edges.push(edge(n - 1, 0, 1));
        check(&format!("unit n {n} seed {seed}"), &edges, 0, n - 1, None);
    }
}
