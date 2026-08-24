//! Packs an instance into blocks and reads every table back out of them.
//!
//! ```text
//! block_pack <graph> <directory> <coordinates> [target MiB]
//! ```
//!
//! Wants an instance numbered by cell path, since a block says where a border
//! node sits inside its cell's run of numbers and there is no run otherwise.
//!
//! Cuts a level's cells into blocks of about the target size, packs each, and
//! then unpacks every table and compares it with what went in. Reports what
//! the blocks come to and how much of that is not the entries themselves.
//!
//! Also asks whether the cells of a level are numbered in the order their keys
//! run, which decides whether a block is a run of cell numbers or has to carry
//! a list of which cells it holds.

use std::{env::args, time::Instant};

use toolbox_rs::{
    block_codec::Codec,
    cell_block::{CellBlock, CellEntry},
    cell_tree::CellTree,
    customization::Customization,
    geometry::FPCoordinate,
    io,
    level_directory::LevelDirectory,
    packed_partition::PackedPartition,
    static_graph::StaticGraph,
};

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / (1024.0 * 1024.0))
}

fn main() {
    env_logger::init();
    let mut argv = args().skip(1);
    let mut next = |what: &str| {
        argv.next().unwrap_or_else(|| {
            panic!(
                "usage: block_pack <graph> <directory> <coordinates> [target MiB]: missing {what}"
            )
        })
    };
    let graph = StaticGraph::new(io::read_edges_from_file(&next("graph")));
    let directory: LevelDirectory = io::read_from_file(&next("directory"));
    let coordinates = io::read_vec_from_file::<FPCoordinate>(&next("coordinates"));
    let target = argv
        .next()
        .map_or(4.0, |mib| mib.parse::<f64>().expect("a size in MiB"))
        * 1024.0
        * 1024.0;

    let levels = directory.levels();
    let partition = PackedPartition::of(&directory);
    let tree = CellTree::of(&directory, &partition, &graph, &coordinates);
    let customization = Customization::new(graph, directory);

    println!(
        "{:>5} {:>9} {:>7} {:>10} {:>10} {:>9} {:>7} {:>7}",
        "level", "cells", "blocks", "raw", "blocks", "framing", "share", "wrong"
    );
    // what each codec makes of the blocks, and what it costs to read one back
    let tried: [(Codec, i32); 5] = [
        (Codec::Stored, 0),
        (Codec::Lz4, 0),
        (Codec::Deflate, 6),
        (Codec::Zstd, 3),
        (Codec::Zstd, 19),
    ];
    let mut squeezed = [0_u64; 5];
    let mut decoding = [std::time::Duration::ZERO; 5];

    let started = Instant::now();
    let (mut all_raw, mut all_block, mut all_framing, mut all_wrong, mut all_blocks) =
        (0_u64, 0_u64, 0_u64, 0_u64, 0_u64);
    let mut keys_in_order = true;

    for level in 0..levels {
        let holding = customization.level(level);
        let count = tree.cells_on_level(level);

        // are the cells numbered the way their keys run?
        let mut last = 0_u128;
        for cell in 0..count {
            let (first, _) = tree.range_of(level, cell as u32);
            if first < last {
                keys_in_order = false;
            }
            last = first;
        }

        // the finest level is the one whose border nodes lead each run
        let border_leads = level == 0;
        let (mut raw, mut packed, mut framing, mut wrong, mut blocks) =
            (0_u64, 0_u64, 0_u64, 0_u64, 0_u64);

        let mut held: Vec<Vec<u32>> = Vec::new();
        let mut widths: Vec<usize> = Vec::new();
        let mut places: Vec<Vec<u32>> = Vec::new();
        let mut holds: Vec<usize> = Vec::new();
        let mut first_of_run = 0_u32;
        let mut carrying = 0_f64;

        let mut flush = |held: &mut Vec<Vec<u32>>,
                         widths: &mut Vec<usize>,
                         places: &mut Vec<Vec<u32>>,
                         holds: &mut Vec<usize>,
                         first: u32,
                         raw: &mut u64,
                         packed: &mut u64,
                         framing: &mut u64,
                         wrong: &mut u64,
                         blocks: &mut u64| {
            if held.is_empty() {
                return;
            }
            let entries = held
                .iter()
                .zip(widths.iter())
                .zip(places.iter())
                .zip(holds.iter())
                .map(|(((matrix, &wide), places), &holds)| CellEntry {
                    matrix,
                    wide,
                    places,
                    holds,
                })
                .collect::<Vec<_>>();
            let block = CellBlock::of(level, first, &entries, border_leads);
            // what it comes to on disk, each way of writing it down
            let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&block).expect("a block serializes");
            for (which, &(codec, effort)) in tried.iter().enumerate() {
                let stored = codec.encode(&bytes, effort);
                let began = Instant::now();
                let read = codec
                    .decode(&stored, bytes.len())
                    .expect("a block reads back");
                decoding[which] += began.elapsed();
                assert_eq!(read.len(), bytes.len(), "{} lost bytes", codec.name());
                squeezed[which] += stored.len() as u64;
            }
            *packed += block.bytes() as u64;
            *framing += block.framing_bytes() as u64;
            *blocks += 1;

            let mut out = Vec::new();
            let mut read_places = Vec::new();
            for (which, matrix) in held.iter().enumerate() {
                block.unpack_into(which, widths, &mut out);
                if &out != matrix {
                    *wrong += 1;
                }
                block.places_into(which, widths, &mut read_places);
                if !border_leads && read_places != places[which] {
                    *wrong += 1;
                }
                *raw += matrix.len() as u64 * 4;
            }
            held.clear();
            widths.clear();
            places.clear();
            holds.clear();
        };

        for cell in 0..count {
            let Some(table) = customization.distances_of(level, cell as u32) else {
                continue;
            };
            let nodes = holding.nodes_of(cell as u32);
            let wide = table.border_nodes_of().len();
            let mut matrix = Vec::with_capacity(wide * wide);
            for source in 0..wide {
                matrix.extend_from_slice(table.row(source));
            }
            // a border node as an offset into the cell's run rather than a
            // node of the graph
            let base = nodes.first().copied().unwrap_or(0);
            let at = if border_leads {
                Vec::new()
            } else {
                table
                    .border_nodes_of()
                    .iter()
                    .map(|&node| node - base as u32)
                    .collect()
            };

            if held.is_empty() {
                first_of_run = cell as u32;
            }
            carrying += (matrix.len() * 4) as f64;
            held.push(matrix);
            widths.push(wide);
            places.push(at);
            holds.push(nodes.len());

            if carrying >= target {
                flush(
                    &mut held,
                    &mut widths,
                    &mut places,
                    &mut holds,
                    first_of_run,
                    &mut raw,
                    &mut packed,
                    &mut framing,
                    &mut wrong,
                    &mut blocks,
                );
                carrying = 0.0;
            }
        }
        flush(
            &mut held,
            &mut widths,
            &mut places,
            &mut holds,
            first_of_run,
            &mut raw,
            &mut packed,
            &mut framing,
            &mut wrong,
            &mut blocks,
        );

        println!(
            "{level:>5} {count:>9} {blocks:>7} {:>10} {:>10} {:>9} {:>7} {:>7}",
            megabytes(raw),
            megabytes(packed),
            megabytes(framing),
            format!("{:.0}%", 100.0 * packed as f64 / raw.max(1) as f64),
            wrong,
        );
        all_raw += raw;
        all_block += packed;
        all_framing += framing;
        all_wrong += wrong;
        all_blocks += blocks;
    }

    println!("\npacked {all_blocks} blocks in {:.1?}", started.elapsed());
    for (name, bytes) in [
        ("raw, four bytes an entry", all_raw),
        ("the blocks", all_block),
        ("of which framing", all_framing),
    ] {
        println!(
            "  {name:<28} {:>10}  {:>5}",
            megabytes(bytes),
            format!("{:.1}%", 100.0 * bytes as f64 / all_raw.max(1) as f64),
        );
    }
    println!(
        "\n  {:<16} {:>10} {:>8} {:>12}",
        "codec", "on disk", "share", "to read all"
    );
    for (which, &(codec, effort)) in tried.iter().enumerate() {
        let name = if effort > 0 {
            format!("{} {effort}", codec.name())
        } else {
            codec.name().to_owned()
        };
        println!(
            "  {name:<16} {:>10} {:>8} {:>12}",
            megabytes(squeezed[which]),
            format!(
                "{:.1}%",
                100.0 * squeezed[which] as f64 / all_raw.max(1) as f64
            ),
            format!("{:.2?}", decoding[which]),
        );
    }
    println!(
        "\n  cells numbered as their keys run: {}",
        if keys_in_order { "yes" } else { "NO" }
    );
    if all_wrong == 0 {
        println!("  every table and every place read back as it was written");
    } else {
        println!("  {all_wrong} TABLES OR PLACES DID NOT READ BACK");
        std::process::exit(1);
    }
}
