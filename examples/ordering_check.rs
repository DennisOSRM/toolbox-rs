//! Whether a numbering leaves every cell in one run of numbers.
//!
//! ```text
//! ordering_check <graph> <directory>
//! ```
//!
//! A store that hands out a subtree at a time wants the nodes of that subtree
//! to be one range of numbers: then a block holds a range, an absent range is
//! a region nobody downloaded, and neither needs a list of members. Whether a
//! numbering gives that is not a matter of opinion, so this asks.
//!
//! For every cell of every level it takes the numbers its nodes were given and
//! asks whether they run from the lowest to the highest with nothing else in
//! between. A cell that does is one range; a cell that does not is as many
//! pieces as the numbering broke it into, and the store would have to hold a
//! list for it.
//!
//! `TOOLBOX_CELL_MAJOR=1` picks the other numbering.

use std::{env::args, time::Instant};

use toolbox_rs::{
    graph::NodeID, io, level_directory::LevelDirectory, node_ordering::NodeOrdering,
    packed_partition::PackedPartition, static_graph::StaticGraph,
};

fn main() {
    env_logger::init();
    let mut argv = args().skip(1);
    let mut next = |what: &str| {
        argv.next()
            .unwrap_or_else(|| panic!("usage: ordering_check <graph> <directory>: missing {what}"))
    };
    let graph = StaticGraph::new(io::read_edges_from_file(&next("graph")));
    let directory: LevelDirectory = io::read_from_file(&next("directory"));
    let levels = directory.levels();

    let partition = PackedPartition::of(&directory);
    let started = Instant::now();
    let ordering = NodeOrdering::of(&graph, &partition);
    println!(
        "numbered {} nodes in {:.2?}, {}",
        ordering.len(),
        started.elapsed(),
        if std::env::var("TOOLBOX_CELL_MAJOR").is_ok() {
            "by cell path first"
        } else {
            "by border level first"
        }
    );

    println!(
        "{:>5} {:>9} {:>11} {:>9} {:>9} {:>11}",
        "level", "cells", "in one run", "share", "worst", "pieces in all"
    );
    for level in 0..levels {
        let count = directory.cells_on_level(level);
        // the lowest and highest number given to a node of each cell, and how
        // many nodes it holds. One run means the three agree.
        let mut lowest = vec![u32::MAX; count];
        let mut highest = vec![0_u32; count];
        let mut holds = vec![0_u32; count];
        for node in 0..ordering.len() {
            let cell = partition.cell_in(partition.word(node), level) as usize;
            let place = ordering.new_of(node as NodeID) as u32;
            lowest[cell] = lowest[cell].min(place);
            highest[cell] = highest[cell].max(place);
            holds[cell] += 1;
        }

        let mut whole = 0_usize;
        let mut worst = 0_u32;
        let mut pieces = 0_usize;
        for cell in 0..count {
            if holds[cell] == 0 {
                continue;
            }
            let span = highest[cell] - lowest[cell] + 1;
            if span == holds[cell] {
                whole += 1;
                pieces += 1;
            } else {
                // not the number of pieces, which would want another walk, but
                // how much of the run the cell does not fill
                worst = worst.max(span - holds[cell]);
                pieces += 2;
            }
        }
        let held = holds.iter().filter(|&&count| count > 0).count();
        println!(
            "{level:>5} {held:>9} {whole:>11} {:>9} {:>9} {:>11}",
            format!("{:.1}%", 100.0 * whole as f64 / held.max(1) as f64),
            worst,
            if whole == held {
                "one each"
            } else {
                ">= 2 each"
            },
        );
        let _ = pieces;
    }
}
