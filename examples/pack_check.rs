//! Packs every cell table of an instance and reads every one of them back.
//!
//! ```text
//! pack_check <graph> <directory>
//! ```
//!
//! The tables are what a store is mostly made of, so the thing to know about
//! the way they are written down is whether it gives back what it was given.
//! This customizes every cell, packs its table, unpacks it and compares it
//! entry by entry with what went in. Anything that differs is reported and
//! counted; nothing is sampled.
//!
//! What it says at the end is what the tables come to packed against what they
//! come to as four-byte numbers, per level and in all.

use std::{
    env::args,
    sync::atomic::{AtomicU64, Ordering},
    time::Instant,
};

use rayon::prelude::*;
use toolbox_rs::{
    customization::Customization, io, level_directory::LevelDirectory,
    packed_distances::PackedDistances, static_graph::StaticGraph,
};

#[derive(Clone, Copy, Default)]
struct Counts {
    cells: u64,
    entries: u64,
    raw: u64,
    packed: u64,
    /// the entries alone, without the frame a table is held in
    payload: u64,
    wrong: u64,
    /// the widest and narrowest a table came out
    widest_bits: u32,
    narrowest_bits: u32,
}

impl Counts {
    fn fold(mut self, other: Self) -> Self {
        self.cells += other.cells;
        self.entries += other.entries;
        self.raw += other.raw;
        self.packed += other.packed;
        self.payload += other.payload;
        self.wrong += other.wrong;
        self.widest_bits = self.widest_bits.max(other.widest_bits);
        self.narrowest_bits = if self.narrowest_bits == 0 {
            other.narrowest_bits
        } else if other.narrowest_bits == 0 {
            self.narrowest_bits
        } else {
            self.narrowest_bits.min(other.narrowest_bits)
        };
        self
    }
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
}

fn main() {
    env_logger::init();
    let mut argv = args().skip(1);
    let mut next = |what: &str| {
        argv.next()
            .unwrap_or_else(|| panic!("usage: pack_check <graph> <directory>: missing {what}"))
    };
    let graph = StaticGraph::new(io::read_edges_from_file(&next("graph")));
    let directory: LevelDirectory = io::read_from_file(&next("directory"));
    let levels = directory.levels();
    let customization = Customization::new(graph, directory);

    // reported rather than asserted, so that a run says how much is wrong
    // rather than stopping at the first of it
    static SHOWN: AtomicU64 = AtomicU64::new(0);

    println!(
        "{:>5} {:>9} {:>11} {:>10} {:>10} {:>10} {:>7} {:>7}",
        "level", "cells", "entries", "raw", "entries", "framing", "share", "wrong"
    );
    let started = Instant::now();
    let mut whole = Counts::default();
    for level in 0..levels {
        let cells = customization.cells_on_level(level);
        let counts = (0..cells)
            .into_par_iter()
            .map(|cell| {
                let mut counts = Counts::default();
                let Some(table) = customization.distances_of(level, cell as u32) else {
                    return counts;
                };
                let wide = table.border_nodes_of().len();
                // the table as one run, which is what the packer takes
                let mut matrix = Vec::with_capacity(wide * wide);
                for source in 0..wide {
                    matrix.extend_from_slice(table.row(source));
                }

                let packed = PackedDistances::of(&matrix, wide);
                let mut read = Vec::new();
                packed.unpack_into(&mut read);

                counts.cells = 1;
                counts.entries = matrix.len() as u64;
                counts.raw = matrix.len() as u64 * 4;
                counts.packed = packed.bytes() as u64;
                // what the entries themselves come to. The rest is the frame:
                // the width, the bit width and the vector header, which in a
                // block move into the directory and stop being paid per cell.
                counts.payload = (matrix.len() as u64 * u64::from(packed.bits())).div_ceil(8);
                counts.widest_bits = packed.bits();
                counts.narrowest_bits = packed.bits();
                if read != matrix {
                    counts.wrong = read
                        .iter()
                        .zip(&matrix)
                        .filter(|(read, was)| read != was)
                        .count() as u64;
                    if SHOWN.fetch_add(1, Ordering::Relaxed) < 4 {
                        let (place, (read, was)) = read
                            .iter()
                            .zip(&matrix)
                            .enumerate()
                            .find(|(_, (read, was))| read != was)
                            .expect("something differed");
                        println!(
                            "  level {level}, cell {cell}, entry {place}: read {read}, was {was}"
                        );
                    }
                }
                counts
            })
            .reduce(Counts::default, Counts::fold);

        println!(
            "{level:>5} {:>9} {:>11} {:>10} {:>10} {:>10} {:>7} {:>7}",
            counts.cells,
            counts.entries,
            megabytes(counts.raw),
            megabytes(counts.payload),
            megabytes(counts.packed - counts.payload),
            format!(
                "{:.0}%",
                100.0 * counts.payload as f64 / counts.raw.max(1) as f64
            ),
            counts.wrong,
        );
        whole = whole.fold(counts);
    }

    println!(
        "\npacked {} cells and {} entries in {:.1?}",
        whole.cells,
        whole.entries,
        started.elapsed()
    );
    for (name, bytes) in [
        ("raw, four bytes an entry", whole.raw),
        ("the entries packed", whole.payload),
        (
            "and the frame a block does not pay",
            whole.packed - whole.payload,
        ),
    ] {
        println!(
            "  {name:<36} {:>10}  {:>5}",
            megabytes(bytes),
            format!("{:.0}%", 100.0 * bytes as f64 / whole.raw.max(1) as f64),
        );
    }
    println!(
        "  widths from {} to {} bits",
        whole.narrowest_bits, whole.widest_bits
    );
    if whole.wrong == 0 {
        println!("  every entry of every table read back as it was written");
    } else {
        println!("  {} ENTRIES DID NOT READ BACK", whole.wrong);
        std::process::exit(1);
    }
}
