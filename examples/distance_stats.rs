//! What the cell tables actually hold, and what encodings would cost.
//!
//! ```text
//! distance_stats <graph> <directory>
//! ```
//!
//! The tables are the bulk of what a store ships, so how they are written down
//! is worth choosing on the numbers rather than on a guess. This customizes
//! every cell and asks, per level, what the distances in a row look like and
//! what each of four ways of writing them would come to.
//!
//! The four:
//!
//! - **raw**, four bytes an entry, which is what is held in memory today.
//! - **min and two bytes**, the smallest of the row and then each entry as its
//!   distance above it. An entry that does not fit escapes to a list.
//! - **min and a row width**, the same but with the deltas cut to as many bits
//!   as the widest of them needs, one width for the row.
//! - **min and a cell width**, one width for the whole table rather than one a
//!   row, which costs bits on the narrow rows and saves the per-row byte.
//!
//! What cannot be reached takes the largest value the width holds, so a width
//! has to have room for one more than the widest real distance.

use std::{env::args, time::Instant};

use rayon::prelude::*;
use toolbox_rs::{
    customization::Customization, io, level_directory::LevelDirectory, static_graph::StaticGraph,
};

/// What one way of writing the tables down comes to.
#[derive(Clone, Copy, Default)]
struct Cost {
    bytes: u64,
    /// entries that had to be written out of line
    escaped: u64,
}

#[derive(Clone, Copy, Default)]
struct Counts {
    rows: u64,
    entries: u64,
    unreachable: u64,
    /// rows whose spread fits in two bytes
    narrow_rows: u64,
    raw: Cost,
    two_bytes: Cost,
    row_width: Cost,
    cell_width: Cost,
    /// the widest spread seen in any row
    widest: u32,
    /// rows whose smallest reachable distance is not nought
    least_not_nought: u64,
    /// a cell width with no per-row least written at all
    bare: Cost,
}

impl Counts {
    fn fold(mut self, other: Self) -> Self {
        self.rows += other.rows;
        self.entries += other.entries;
        self.unreachable += other.unreachable;
        self.narrow_rows += other.narrow_rows;
        self.raw.bytes += other.raw.bytes;
        self.two_bytes.bytes += other.two_bytes.bytes;
        self.two_bytes.escaped += other.two_bytes.escaped;
        self.row_width.bytes += other.row_width.bytes;
        self.cell_width.bytes += other.cell_width.bytes;
        self.widest = self.widest.max(other.widest);
        self.least_not_nought += other.least_not_nought;
        self.bare.bytes += other.bare.bytes;
        self
    }
}

/// How many bits it takes to hold every number up to and including `largest`.
fn bits_for(largest: u32) -> u32 {
    if largest == 0 {
        1
    } else {
        u32::BITS - largest.leading_zeros()
    }
}

fn main() {
    env_logger::init();
    let mut argv = args().skip(1);
    let mut next = |what: &str| {
        argv.next()
            .unwrap_or_else(|| panic!("usage: distance_stats <graph> <directory>: missing {what}"))
    };
    let graph = StaticGraph::new(io::read_edges_from_file(&next("graph")));
    let directory: LevelDirectory = io::read_from_file(&next("directory"));
    let levels = directory.levels();
    let customization = Customization::new(graph, directory);

    println!(
        "{:>5} {:>11} {:>7} {:>8} {:>10} {:>10} {:>10} {:>10}",
        "level", "entries", "unreach", "fit 2B", "raw", "min+2B", "row width", "cell width"
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
                // one width for the whole table wants the widest spread in it,
                // so the rows are looked at once to find it and once to count
                let mut widest_in_cell = 0_u32;
                for source in 0..wide {
                    let row = table.row(source);
                    if let Some(spread) = spread_of(row) {
                        widest_in_cell = widest_in_cell.max(spread);
                    }
                }
                let cell_bits = u64::from(bits_for(widest_in_cell.saturating_add(1)));

                for source in 0..wide {
                    let row = table.row(source);
                    let entries = row.len() as u64;
                    counts.rows += 1;
                    counts.entries += entries;
                    counts.unreachable += row.iter().filter(|&&at| at == u32::MAX).count() as u64;
                    counts.raw.bytes += entries * 4;

                    let Some(spread) = spread_of(row) else {
                        // nothing reachable: a width of nothing and a flag
                        counts.two_bytes.bytes += 5;
                        counts.row_width.bytes += 5;
                        counts.cell_width.bytes += 4;
                        counts.bare.bytes += 0;
                        continue;
                    };
                    counts.widest = counts.widest.max(spread);
                    if spread < u32::from(u16::MAX) {
                        counts.narrow_rows += 1;
                    }
                    // the smallest of the row, written once
                    let escaped = row
                        .iter()
                        .filter(|&&at| {
                            at != u32::MAX && u64::from(at) - u64::from(least(row)) >= 0xFFFF
                        })
                        .count() as u64;
                    counts.two_bytes.escaped += escaped;
                    // four for the least, two an entry, and six for each that
                    // had to be written out of line
                    counts.two_bytes.bytes += 4 + entries * 2 + escaped * 6;

                    let bits = u64::from(bits_for(spread.saturating_add(1)));
                    counts.row_width.bytes += 4 + 1 + (entries * bits).div_ceil(8);
                    counts.cell_width.bytes += 4 + (entries * cell_bits).div_ceil(8);
                    // the same without the least, which is worth asking about
                    // because a row holds the distance from a node to itself
                    if least(row) != 0 {
                        counts.least_not_nought += 1;
                    }
                    counts.bare.bytes += (entries * cell_bits).div_ceil(8);
                }
                counts
            })
            .reduce(Counts::default, Counts::fold);

        report(level, &counts);
        whole = whole.fold(counts);
    }
    println!("customized and walked in {:.1?}", started.elapsed());
    report_whole(&whole);
}

/// The smallest distance in a row that can be reached at all.
fn least(row: &[u32]) -> u32 {
    row.iter()
        .copied()
        .filter(|&at| at != u32::MAX)
        .min()
        .unwrap_or(0)
}

/// How far the reachable entries of a row spread, and nothing for a row that
/// reaches nowhere.
fn spread_of(row: &[u32]) -> Option<u32> {
    let mut low = u32::MAX;
    let mut high = 0_u32;
    let mut any = false;
    for &at in row {
        if at == u32::MAX {
            continue;
        }
        any = true;
        low = low.min(at);
        high = high.max(at);
    }
    any.then(|| high - low)
}

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
}

fn report(level: usize, counts: &Counts) {
    println!(
        "{level:>5} {:>11} {:>7} {:>8} {:>10} {:>10} {:>10} {:>10}",
        counts.entries,
        format!(
            "{:.1}%",
            100.0 * counts.unreachable as f64 / counts.entries.max(1) as f64
        ),
        format!(
            "{:.1}%",
            100.0 * counts.narrow_rows as f64 / counts.rows.max(1) as f64
        ),
        megabytes(counts.raw.bytes),
        megabytes(counts.two_bytes.bytes),
        megabytes(counts.row_width.bytes),
        megabytes(counts.cell_width.bytes),
    );
}

fn report_whole(counts: &Counts) {
    let raw = counts.raw.bytes as f64;
    println!(
        "\n{} entries, {} rows, widest spread {}, {} rows whose least is not nought",
        counts.entries, counts.rows, counts.widest, counts.least_not_nought
    );
    for (name, cost) in [
        ("raw, four bytes", counts.raw),
        ("least and two bytes", counts.two_bytes),
        ("least and a row width", counts.row_width),
        ("least and a cell width", counts.cell_width),
        ("a cell width, no least", counts.bare),
    ] {
        println!(
            "  {name:<24} {:>10}  {:>6}{}",
            megabytes(cost.bytes),
            format!("{:.0}%", 100.0 * cost.bytes as f64 / raw),
            if cost.escaped > 0 {
                format!(", {} written out of line", cost.escaped)
            } else {
                String::new()
            }
        );
    }
}
