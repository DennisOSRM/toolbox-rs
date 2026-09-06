//! What an edge-based graph worked out as it is walked costs, and what it
//! changes.
//!
//!     edge_based_probe <graph> <coordinates> [source] [rounds]
//!
//! Each round is one search to exhaustion from a single source, four times
//! over: on the
//! node-based graph, and on the edge-based graph with free turns, with
//! reversals refused, and with a penalty by angle. One search reaches
//! everything the source can reach, so nothing here rests on which pairs were
//! picked. The rounds run one after another and the least time each way took
//! is the one reported, since a machine with anything else on it can only ever
//! make a run slower than it is.
//!
//! An edge-based search leaves a cost against each arc. What it cost to reach a
//! node is the least of those over the arcs running into it, which is what the
//! two are compared as.
use std::{env::args, time::Instant};

use toolbox_rs::{
    edge_based::{AnglePenalty, EdgeBasedGraph, FreeTurns, NoUTurns, TurnCost, between_nodes},
    geometry::FPCoordinate,
    graph::{Arcs, NodeID},
    io,
    static_graph::StaticGraph,
    unidirectional_dijkstra::TrackedUnidirectionalDijkstra,
};

const UNREACHED: usize = usize::MAX;

/// What one way of costing turns did, and what it left behind.
struct Run {
    what: &'static str,
    seconds: f64,
    /// nodes the queue took in, which for an edge-based run counts arcs
    search_space: usize,
    /// offsets read looking for tails, over tails asked for
    steps: usize,
    lookups: usize,
    /// what it cost to reach each node of the underlying graph
    distance: Vec<usize>,
}

impl Run {
    fn reached(&self) -> usize {
        self.distance.iter().filter(|&&at| at != UNREACHED).count()
    }

    /// How this run's answers differ from the ones free turns gave.
    ///
    /// Free turns forbid nothing and cost nothing, so they answer what the
    /// node-based graph answers, and every difference here is something the
    /// turn cost did.
    fn against(&self, free: &Run) -> (usize, usize, f64, usize) {
        let (mut dearer, mut shut_out, mut sum, mut worst) = (0, 0, 0.0, 0);
        for (mine, theirs) in self.distance.iter().zip(&free.distance) {
            if *theirs == UNREACHED {
                continue;
            }
            if *mine == UNREACHED {
                shut_out += 1;
            } else if mine > theirs {
                dearer += 1;
                sum += (mine - theirs) as f64 / *theirs as f64;
                worst = worst.max(mine - theirs);
            }
        }
        let share = if dearer == 0 {
            0.0
        } else {
            sum / dearer as f64
        };
        (dearer, shut_out, 100.0 * share, worst)
    }
}

/// A search to exhaustion over the node-based graph.
fn over_nodes(graph: &StaticGraph<u32>, source: NodeID, rounds: usize) -> Run {
    let mut search = TrackedUnidirectionalDijkstra::new();
    let nowhere = graph.number_of_nodes();

    let mut seconds = f64::MAX;
    for _ in 0..rounds {
        let at = Instant::now();
        search.run(graph, source, nowhere);
        seconds = seconds.min(at.elapsed().as_secs_f64());
    }

    let distance = (0..graph.number_of_nodes())
        .map(|node| search.distance(node))
        .collect();
    Run {
        what: "node-based",
        seconds,
        search_space: search.search_space_len(),
        steps: 0,
        lookups: 0,
        distance,
    }
}

/// A search to exhaustion over the edge-based graph, folded back onto nodes.
fn over_edges<T: TurnCost>(
    what: &'static str,
    graph: &StaticGraph<u32>,
    coordinates: &[FPCoordinate],
    turns: T,
    source: NodeID,
    rounds: usize,
) -> Run {
    let expanded = EdgeBasedGraph::new(graph, coordinates, turns);
    let mut search = TrackedUnidirectionalDijkstra::new();
    let nowhere = graph.number_of_nodes();

    let mut seconds = f64::MAX;
    for _ in 0..rounds {
        let at = Instant::now();
        between_nodes(&mut search, &expanded, source, nowhere);
        seconds = seconds.min(at.elapsed().as_secs_f64());
    }

    // what it cost to reach a node is the least over the arcs that run into it
    let mut distance = vec![UNREACHED; graph.number_of_nodes()];
    for arc in 0..graph.number_of_edges() {
        let reached = search.distance(arc);
        if reached != UNREACHED {
            let head = graph.target(arc);
            distance[head] = distance[head].min(reached);
        }
    }
    // the source itself is where the run began, at no cost
    distance[source] = 0;

    let (steps, lookups) = expanded.tail_steps();
    Run {
        what,
        seconds,
        search_space: search.search_space_len(),
        steps,
        lookups,
        distance,
    }
}

fn main() {
    let graph = args().nth(1).expect("a graph");
    let coordinates = args().nth(2).expect("coordinates");
    let source: NodeID = args()
        .nth(3)
        .map_or(1_000_003, |given| given.parse().expect("a source node"));
    let rounds: usize = args()
        .nth(4)
        .map_or(3, |given| given.parse().expect("a round count"));

    let edges = io::read_edges_from_file(&graph);
    let coordinates = io::read_vec_from_file::<FPCoordinate>(&coordinates);
    let graph = StaticGraph::new(edges);
    println!(
        "{} nodes, {} arcs, {} coordinates, source {source}, {rounds} rounds",
        graph.number_of_nodes(),
        graph.number_of_edges(),
        coordinates.len()
    );

    let plain = over_nodes(&graph, source, rounds);
    let free = over_edges("free", &graph, &coordinates, FreeTurns, source, rounds);
    let none = over_edges("no u-turns", &graph, &coordinates, NoUTurns, source, rounds);
    let angle = over_edges(
        "angle 30/100",
        &graph,
        &coordinates,
        AnglePenalty::new(30., 100),
        source,
        rounds,
    );

    println!(
        "\n{:>14} {:>9} {:>14} {:>22} {:>8} {:>12}",
        "turns", "time", "search space", "offsets read", "slower", "reached"
    );
    for run in [&plain, &free, &none, &angle] {
        let steps = if run.lookups == 0 {
            "-".to_string()
        } else {
            format!("{} in {}", run.steps, run.lookups)
        };
        println!(
            "{:>14} {:>8.3}s {:>14} {:>22} {:>7.2}x {:>12}",
            run.what,
            run.seconds,
            run.search_space,
            steps,
            run.seconds / plain.seconds,
            run.reached()
        );
    }

    println!(
        "\n{:>14} {:>12} {:>10} {:>10} {:>10}",
        "turns", "dearer", "shut out", "mean rise", "worst rise"
    );
    for run in [&none, &angle] {
        let (dearer, shut_out, share, worst) = run.against(&free);
        println!(
            "{:>14} {:>12} {:>10} {:>9.2}% {:>10}",
            run.what, dearer, shut_out, share, worst
        );
    }

    // the check that matters: free turns forbid nothing and cost nothing
    let wrong = free
        .distance
        .iter()
        .zip(&plain.distance)
        .filter(|(mine, theirs)| mine != theirs)
        .count();
    println!(
        "\nfree turns answered {} of {} nodes exactly as the node-based graph did",
        graph.number_of_nodes() - wrong,
        graph.number_of_nodes()
    );
    assert_eq!(wrong, 0, "the expansion changed a distance it must not");
}
