//! Replays a corpus of real solver inputs, so that a change to the max-flow
//! solver can be measured in seconds instead of by a full partitioning run.
//! Built for gate G1 of issue #545.
//!
//! Usage: replay_cells <corpus directory> [repeats]
//!
//! Prints the flow of every cell, which is also the correctness check: the
//! numbers have to stay the same across solver changes, because the minimum cut
//! value of a fixed graph is unique even when the cut itself is not.
use std::time::Instant;
use toolbox_rs::{
    dinic::Dinic, incremental_dinic::IncrementalDinic, max_flow::MaxFlow, solver_stats::read_cell,
};

fn main() {
    let mut args = std::env::args().skip(1);
    let directory = args
        .next()
        .expect("usage: replay_cells <directory> [repeats]");
    let repeats: usize = args.next().map_or(1, |r| r.parse().expect("repeats"));
    // which solver to replay with, so the two can be compared on the same corpus
    let solver_name = std::env::var("TOOLBOX_SOLVER").unwrap_or_else(|_| "dinic".to_string());

    let mut cells: Vec<_> = std::fs::read_dir(&directory)
        .expect("could not read corpus directory")
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|path| path.extension().is_some_and(|e| e == "bin"))
        .collect();
    cells.sort();

    println!(
        "{:>12}  {:>10}  {:>8}  {:>10}",
        "arcs", "nodes", "flow", "millis"
    );
    let mut total_flow: i64 = 0;
    let mut total_millis = 0.;
    for path in &cells {
        let (edges, source, target) = read_cell(path);
        let arcs = edges.len();
        // outside the repeat loop, so that it is not charged to the solver
        let nodes = edges
            .iter()
            .map(|edge| edge.source.max(edge.target))
            .max()
            .unwrap_or(0)
            + 1;
        for _ in 0..repeats {
            let start = Instant::now();
            let flow = match solver_name.as_str() {
                "incremental" => {
                    let mut solver =
                        IncrementalDinic::from_edge_list(edges.clone(), source, target);
                    solver.run();
                    solver.max_flow().expect("solver did not run")
                }
                _ => {
                    let mut solver = Dinic::from_edge_list(edges.clone(), source, target);
                    solver.run();
                    solver.max_flow().expect("solver did not run")
                }
            };
            let elapsed = start.elapsed().as_secs_f64() * 1000.;
            total_flow += flow as i64;
            total_millis += elapsed;
            println!("{arcs:>12}  {nodes:>10}  {flow:>8}  {elapsed:>10.1}");
        }
    }
    println!(
        "solver {solver_name}, cells {}, repeats {repeats}",
        cells.len()
    );
    println!("flow checksum {total_flow}");
    println!("total {total_millis:.1} ms");
}
