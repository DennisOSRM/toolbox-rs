//! What it costs to customize again when some weights change.
//!
//!   on_device <graph> <directory> [share ...]
//!
//! A device holding a map is told that some roads are slower than the map
//! says: a jam, a closure, a fresh traffic feed. What that costs is not a
//! customization -- the tables that do not depend on those roads are still
//! right -- and the point of this is to say how much less it is, against the
//! full run it is being spared.
//!
//! A share is a percentage of the arcs of the graph, so `0.1 1 10 100` asks
//! about a thousandth, a hundredth, a tenth and all of them.
//!
//! The arcs are drawn at random from the whole graph, which is the worst case
//! for this: a real feed names roads that are near each other, and roads near
//! each other share cells, so the same count of them dirties fewer tables.

use std::{env::args, time::Instant};

use rand::{RngExt, SeedableRng, rngs::StdRng};
use toolbox_rs::{
    customization::Customization,
    graph::EdgeID,
    io,
    level_directory::{CellId, LevelDirectory},
    static_graph::StaticGraph,
};

/// Works out every table there is, which is what a customization comes to.
fn customize(held: &Customization, levels: usize) -> usize {
    let mut tables = 0;
    for level in 0..levels {
        for cell in 0..held.cells_on_level(level) as CellId {
            if held.distances_of(level, cell).is_some() {
                tables += 1;
            }
        }
    }
    tables
}

fn main() {
    let mut argv = args().skip(1);
    let mut next = |what: &str| {
        argv.next().unwrap_or_else(|| {
            panic!("usage: on_device <graph> <directory> [share ...]: missing {what}")
        })
    };
    let graph_path = next("graph");
    let directory_path = next("directory");
    let shares: Vec<f64> = argv
        .map(|share| share.parse::<f64>().expect("a share in per cent"))
        .collect();
    let shares = if shares.is_empty() {
        vec![0.1, 1.0, 10.0, 100.0]
    } else {
        shares
    };

    let edges = io::read_edges_from_file(&graph_path);
    let directory: LevelDirectory = io::read_from_file(&directory_path);
    let levels = directory.levels();
    let arcs = edges.len();
    println!(
        "{} nodes, {arcs} arcs, {levels} levels",
        directory.number_of_nodes()
    );

    // The full run, which is what everything else is measured against. It is
    // also what leaves a customization in the state an update starts from, so
    // it is done once and then updated over and over.
    let started = Instant::now();
    let mut held = Customization::new(StaticGraph::new(edges.clone()), directory.clone());
    let tables = customize(&held, levels);
    let whole = started.elapsed();
    println!("customized {tables} tables in {whole:.1?}, which is the run an update is spared\n");

    println!(
        "{:>10} {:>8} {:>12} {:>10} {:>12} {:>10} {:>10}",
        "drawn", "share", "arcs", "dirty", "of tables", "took", "of a run"
    );
    let mut rng = StdRng::seed_from_u64(0x0DE7);
    for share in shares {
        let how_many = ((arcs as f64) * share / 100.0).round() as usize;
        let how_many = how_many.clamp(1, arcs);

        // Twice over: once from the whole graph, which is the worst this can
        // be asked to do, and once from one run of it. The nodes were
        // renumbered so that a cell's nodes are a run, so a run of arcs is
        // roads near each other -- which is what a jam is, and what a feed
        // naming one part of a city is.
        for scattered in [true, false] {
            let changes: Vec<(EdgeID, u32)> = if scattered {
                (0..how_many)
                    .map(|_| (rng.random_range(0..arcs), 1 + rng.random_range(0..1000)))
                    .collect()
            } else {
                let first = if how_many >= arcs {
                    0
                } else {
                    rng.random_range(0..arcs - how_many)
                };
                (first..(first + how_many).min(arcs))
                    .map(|edge| (edge, 1 + rng.random_range(0..1000)))
                    .collect()
            };

            let started = Instant::now();
            let dirty = held.update(&changes);
            let marked = started.elapsed();
            let started = Instant::now();
            customize(&held, levels);
            let took = marked + started.elapsed();

            println!(
                "{:>10} {:>7.1}% {:>12} {dirty:>10} {:>11.1}% {:>10} {:>9.1}%",
                if scattered { "scattered" } else { "together" },
                share,
                changes.len(),
                100.0 * dirty as f64 / tables as f64,
                format!("{took:.1?}"),
                100.0 * took.as_secs_f64() / whole.as_secs_f64(),
            );
        }
    }
}
