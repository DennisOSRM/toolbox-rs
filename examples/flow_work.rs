//! What a push-relabel run spends itself on, with and without a bound.
use std::sync::{Arc, atomic::AtomicI32};

use rand::{RngExt, SeedableRng, rngs::StdRng};
use toolbox_rs::{
    dinic::Dinic,
    edge::InputEdge,
    max_flow::{MaxFlow, ResidualEdgeData},
    push_relabel::PushRelabel,
};

fn grid(side: usize, rng: &mut StdRng) -> (Vec<InputEdge<ResidualEdgeData>>, usize, usize) {
    let node = |row: usize, column: usize| 2 + row * side + column;
    let mut edges = Vec::new();
    for row in 0..side {
        for column in 0..side {
            let mut capacity = || ResidualEdgeData::new(rng.random_range(1..=4));
            if column + 1 < side {
                edges.push(InputEdge::new(
                    node(row, column),
                    node(row, column + 1),
                    capacity(),
                ));
                edges.push(InputEdge::new(
                    node(row, column + 1),
                    node(row, column),
                    capacity(),
                ));
            }
            if row + 1 < side {
                edges.push(InputEdge::new(
                    node(row, column),
                    node(row + 1, column),
                    capacity(),
                ));
                edges.push(InputEdge::new(
                    node(row + 1, column),
                    node(row, column),
                    capacity(),
                ));
            }
        }
        edges.push(InputEdge::new(
            0,
            node(row, 0),
            ResidualEdgeData::new(i32::MAX / 4),
        ));
        edges.push(InputEdge::new(
            node(row, side - 1),
            1,
            ResidualEdgeData::new(i32::MAX / 4),
        ));
    }
    (edges, 0, 1)
}

fn main() {
    println!(
        "{:>6} {:>8} {:>8} {:>7} {:>12} {:>10} {:>8} {:>9}",
        "side", "nodes", "pick", "bound", "pushes", "relabels", "global", "flow"
    );
    for side in [32_usize, 64, 96] {
        let mut rng = StdRng::seed_from_u64(0x5EED);
        let (edges, source, sink) = grid(side, &mut rng);
        let nodes = 2 + side * side;

        let mut dinic = Dinic::from_edge_list(edges.clone(), source, sink);
        dinic.run();
        let full = dinic.max_flow().expect("dinic did not run");

        for lowest in [false, true] {
            for bound in [None, Some(4)] {
                let mut solver = PushRelabel::from_edge_list(edges.clone(), source, sink);
                solver.by_lowest_label(lowest);
                match bound {
                    None => solver.run(),
                    Some(at) => solver.run_with_upper_bound(Arc::new(AtomicI32::new(at))),
                }
                let (pushes, relabels, global) = solver.work();
                let said = solver
                    .max_flow()
                    .map_or_else(|_| "gave up".to_string(), |flow| flow.to_string());
                println!(
                    "{side:>6} {nodes:>8} {:>8} {:>7} {pushes:>12} {relabels:>10} {global:>8} {said:>9}",
                    if lowest { "lowest" } else { "highest" },
                    bound.map_or("none".to_string(), |at| at.to_string()),
                );
            }
        }
        println!(
            "{side:>6} {nodes:>8} {:>8} {:>7} {:>12} {:>10} {:>8} {full:>9}",
            "dinic", "", "", "", ""
        );
    }
}
