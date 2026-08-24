//! Customizes every cell of every level and says what it cost.
//!
//! ```text
//! customize <graph> <directory> [level]
//! ```
//!
//! The overlay is worked out as it is asked for, so nothing in the crate ever
//! customizes the whole of it in one go and there is no number saying what
//! that costs. This asks for every cell of every level in turn, which is the
//! same work a customization would do, and reports it per level.
//!
//! Bottom up, because a cell is built out of the cells below it. Asking for a
//! coarse cell first would pull the fine ones in behind it and charge their
//! time to the coarse level, which is the one measurement here that has to be
//! right.
//!
//! What is reported per level is what the cost is made of. A level is a number
//! of cells, each with a number of border nodes, and the table of a cell is
//! that number squared, filled by that number of searches. So the totals of
//! the border nodes and of their squares say which levels can be expected to
//! be dear before anything is measured, and the wall time says which ones
//! turned out to be.

use std::{
    env::args,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use rayon::prelude::*;
use toolbox_rs::{
    customization::Customization, graph::Graph, io, level_directory::LevelDirectory,
    static_graph::StaticGraph,
};

fn main() {
    env_logger::init();
    let mut argv = args().skip(1);
    let mut next = |what: &str| {
        argv.next().unwrap_or_else(|| {
            panic!("usage: customize <graph> <directory> [level]: missing {what}")
        })
    };
    let graph_path = next("graph");
    let directory_path = next("directory");
    let rest: Vec<String> = argv.collect();
    // cells of one level are independent of one another, so a level is
    // parallel; the levels are not, a cell being built out of the cells below
    let parallel = rest.iter().any(|word| word == "parallel");
    let only: Option<usize> = rest
        .iter()
        .find(|word| word.parse::<usize>().is_ok())
        .map(|word| word.parse().expect("a level"));

    let loading = Instant::now();
    let graph = StaticGraph::new(io::read_edges_from_file(&graph_path));
    let directory: LevelDirectory = io::read_from_file(&directory_path);
    let levels = directory.levels();
    println!(
        "{} nodes, {} arcs, {levels} levels, loaded in {:.1?}",
        directory.number_of_nodes(),
        graph.number_of_edges(),
        loading.elapsed()
    );

    // What the cells of a level cost, split into what was spent building the
    // graph to search and what was spent searching it. Six levels of counters
    // rather than a lock around a table: this is read once per cell, and a
    // measurement that contends is a measurement that changes what it measures.
    const LEVELS: usize = 16;
    static BUILDING: [AtomicU64; LEVELS] = [const { AtomicU64::new(0) }; LEVELS];
    static SEARCHING: [AtomicU64; LEVELS] = [const { AtomicU64::new(0) }; LEVELS];
    static NODES: [AtomicU64; LEVELS] = [const { AtomicU64::new(0) }; LEVELS];
    static ARCS: [AtomicU64; LEVELS] = [const { AtomicU64::new(0) }; LEVELS];

    let customization = Customization::new(graph, directory).watched_by(|report| {
        let level = report.level.min(LEVELS - 1);
        BUILDING[level].fetch_add(report.building.as_nanos() as u64, Ordering::Relaxed);
        SEARCHING[level].fetch_add(report.searching.as_nanos() as u64, Ordering::Relaxed);
        NODES[level].fetch_add(report.searched as u64, Ordering::Relaxed);
        ARCS[level].fetch_add(report.arcs as u64, Ordering::Relaxed);
    });
    println!(
        "{:>5} {:>8} {:>9} {:>6} {:>7} {:>7} {:>7} {:>10} {:>10} {:>9}",
        "level", "cells", "entries", "V", "wall", "search", "build", "tables", "levelled", "a cell"
    );

    let mut whole = std::time::Duration::ZERO;
    let mut all_entries = 0usize;
    let mut all_bytes = 0usize;
    for level in 0..levels {
        let cells = customization.cells_on_level(level);
        let started = Instant::now();
        let of_cell = |cell: usize| {
            customization
                .distances_of(level, cell as u32)
                .map_or((0, 0, 0, 0), |distances| {
                    let wide = distances.border_nodes.len();
                    (1, wide, wide * wide, distances.bytes())
                })
        };
        let add = |a: (usize, usize, usize, usize), b: (usize, usize, usize, usize)| {
            (a.0 + b.0, a.1 + b.1, a.2 + b.2, a.3 + b.3)
        };
        let (tabulated, border, entries, bytes) = if parallel {
            // Widest cell first.
            //
            // A level is handed out a cell to a thread, and the coarsest level
            // of a continent has twenty-six of them of very different sizes.
            // Taken in the order they are numbered a big one can come up last
            // and every thread waits on it; taken widest first it starts at
            // once and the small ones fill in around it. Longest job first,
            // which is the oldest trick there is for this and costs a sort of
            // as many numbers as the level has cells.
            let holding = customization.level(level);
            let mut order: Vec<usize> = (0..cells).collect();
            order.sort_unstable_by_key(|&cell| {
                std::cmp::Reverse(holding.nodes_of(cell as u32).len())
            });
            order
                .into_par_iter()
                .map(of_cell)
                .reduce(|| (0, 0, 0, 0), add)
        } else {
            (0..cells).map(of_cell).fold((0, 0, 0, 0), add)
        };
        let elapsed = started.elapsed();
        whole += elapsed;
        all_entries += entries;
        let building = BUILDING[level].load(Ordering::Relaxed) as f64 / 1e9;
        let searching = SEARCHING[level].load(Ordering::Relaxed) as f64 / 1e9;
        let searched = NODES[level].load(Ordering::Relaxed);
        let arcs = ARCS[level].load(Ordering::Relaxed);
        let holding = customization.level(level).bytes();
        all_bytes += bytes + holding;
        println!(
            "{level:>5} {cells:>8} {entries:>9} {:>6} {:>7} {:>7} {:>7} {:>10} {:>10} {:>9}",
            searched / tabulated.max(1) as u64,
            format!("{:.2}s", elapsed.as_secs_f64()),
            format!("{searching:.2}s"),
            format!("{building:.2}s"),
            in_bytes(bytes),
            in_bytes(holding),
            in_bytes(bytes / tabulated.max(1)),
        );
        // the arcs and the border nodes are counted for whoever wants them
        debug_assert!(arcs >= border as u64 || border == 0);
        // a level is built out of the one below, so the ones below a level
        // asked for are worked out and reported on the way up to it
        if only.is_some_and(|wanted| wanted == level) {
            break;
        }
    }

    // one table of borders for the whole partition rather than one a level
    let shared = customization.directory().number_of_nodes();
    println!(
        "customized {} cells in {:.2?}, {all_entries} entries, {:.0} entries a second",
        customization.customized_cells(),
        whole,
        all_entries as f64 / whole.as_secs_f64()
    );
    println!(
        "holding {} in all: {} of tables and levels, {} of borders shared by every level",
        in_bytes(all_bytes + shared),
        in_bytes(all_bytes),
        in_bytes(shared)
    );
}

/// A count of bytes, in whichever unit says it in fewest digits.
fn in_bytes(count: usize) -> String {
    const STEPS: [(f64, &str); 4] = [
        (1024.0 * 1024.0 * 1024.0, "GiB"),
        (1024.0 * 1024.0, "MiB"),
        (1024.0, "KiB"),
        (1.0, "B"),
    ];
    for (size, name) in STEPS {
        if count as f64 >= size {
            return format!("{:.1} {name}", count as f64 / size);
        }
    }
    format!("{count} B")
}
