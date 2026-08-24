//! Builds the cell tree of an instance and says what it costs to ship.
//!
//! ```text
//! skeleton <graph> <directory> <coordinates> [out]
//! ```
//!
//! The tree is the part of a store that everybody has: it says what cells
//! there are, what each holds, where each is, and which key range each covers.
//! Nothing can be looked up without it, so it is downloaded whole and its size
//! is a number worth watching rather than discovering.
//!
//! Reports what it costs per level, writes it if asked where, and reads it
//! back to check the writing.

use std::{env::args, time::Instant};

use toolbox_rs::{
    cell_tree::CellTree, geometry::FPCoordinate, io, level_directory::LevelDirectory,
    packed_partition::PackedPartition, static_graph::StaticGraph,
};

fn main() {
    env_logger::init();
    let mut argv = args().skip(1);
    let mut next = |what: &str| {
        argv.next().unwrap_or_else(|| {
            panic!("usage: skeleton <graph> <directory> <coordinates> [out]: missing {what}")
        })
    };
    let graph = StaticGraph::new(io::read_edges_from_file(&next("graph")));
    let directory: LevelDirectory = io::read_from_file(&next("directory"));
    let coordinates = io::read_vec_from_file::<FPCoordinate>(&next("coordinates"));
    let out = argv.next();

    let started = Instant::now();
    let partition = PackedPartition::of(&directory);
    let packed = started.elapsed();
    let started = Instant::now();
    let tree = CellTree::of(&directory, &partition, &graph, &coordinates);
    println!(
        "packed the partition in {packed:.2?}, built the tree in {:.2?}, key is {} bits wide",
        started.elapsed(),
        tree.key_bits()
    );

    println!(
        "{:>5} {:>9} {:>7} {:>12} {:>12} {:>9} {:>18}",
        "level", "cells", "bits", "nodes", "on border", "share", "widest key range"
    );
    for level in 0..tree.levels() {
        let cells = tree.cells_on_level(level);
        let mut nodes = 0_u64;
        let mut border = 0_u64;
        let mut widest = 0_u128;
        for cell in 0..cells {
            let facts = tree.facts(level, cell as u32);
            nodes += u64::from(facts.nodes);
            border += u64::from(facts.on_border);
            let (first, last) = tree.range_of(level, cell as u32);
            widest = widest.max(last - first + 1);
        }
        let bits = if level + 1 < tree.levels() {
            tree.begins_at(level + 1) - tree.begins_at(level)
        } else {
            tree.key_bits() - tree.begins_at(level)
        };
        println!(
            "{level:>5} {cells:>9} {bits:>7} {nodes:>12} {border:>12} {:>9} {widest:>18}",
            format!("{:.1}%", 100.0 * border as f64 / nodes as f64),
        );
    }

    let Some(out) = out else {
        return;
    };
    let started = Instant::now();
    io::write_to_file(&out, &tree);
    let written = started.elapsed();
    let size = std::fs::metadata(&out).expect("the tree was written").len();
    let started = Instant::now();
    let read: CellTree = io::read_from_file(&out);
    println!(
        "wrote {:.1} MiB to {out} in {written:.2?}, read back in {:.2?}, {}",
        size as f64 / (1024.0 * 1024.0),
        started.elapsed(),
        if read == tree {
            "the same tree"
        } else {
            "A DIFFERENT TREE"
        }
    );
    assert!(read.check_version().is_ok(), "a version this cannot read");
}
