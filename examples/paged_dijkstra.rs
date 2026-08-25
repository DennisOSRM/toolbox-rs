//! A plain Dijkstra over a graph on a file, against the same over memory.
//!
//!   paged_dijkstra <graph> <directory> <coordinates> <pairs> <arcs> [MiB ...]
//!
//! No overlay and no cell tables: this is the graph alone, packed into blocks
//! and read back as a search walks it, which is the part of an instance that
//! was too big to page before and is the larger half of what one costs.
//!
//! TOOLBOX_ARC_KIB      how much a block holds unpacked, in kibibytes
//! TOOLBOX_ORDERING     puts the pairs through the numbering the pack was built on
//! TOOLBOX_TIMINGS      per-pair rows for rank_plot
//! TOOLBOX_PAIR_STRIDE  keeps every Nth pair

use std::{env::args, fs::File, io::Write, path::Path, time::Instant};

use toolbox_rs::{
    block_codec::Codec,
    block_map::BlockMap,
    border_levels::BorderLevels,
    cell_tree::CellTree,
    geometry::FPCoordinate,
    graph::{Graph, NodeID},
    io,
    level_directory::LevelDirectory,
    node_ordering::NodeOrdering,
    one_to_many_dijkstra::OneToManyDijkstra,
    packed_partition::PackedPartition,
    paged_graph::{PagedGraph, pack},
    static_graph::StaticGraph,
};

const MIB: usize = 1024 * 1024;

fn pairs_of(path: &str) -> Vec<(NodeID, NodeID, u64)> {
    std::fs::read_to_string(path)
        .expect("a file of pairs")
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split(',');
            let source = fields.next()?.trim().parse().ok()?;
            let target = fields.next()?.trim().parse().ok()?;
            let rank = fields
                .next()
                .and_then(|at| at.trim().parse().ok())
                .unwrap_or(0);
            Some((source, target, rank))
        })
        .collect()
}

fn main() {
    let mut argv = args().skip(1);
    let mut next = |what: &str| {
        argv.next().unwrap_or_else(|| {
            panic!(
                "usage: paged_dijkstra <graph> <directory> <coordinates> <pairs> <arcs> [MiB ...]: missing {what}"
            )
        })
    };
    let graph_path = next("graph");
    let directory_path = next("directory");
    let coordinates_path = next("coordinates");
    let pairs_path = next("pairs");
    let arcs_path = next("arcs");
    let budgets: Vec<usize> = argv
        .map(|mib| mib.parse::<usize>().expect("a size in MiB") * MIB)
        .collect();
    let budgets = if budgets.is_empty() {
        vec![16 * MIB, 64 * MIB, 256 * MIB]
    } else {
        budgets
    };

    let graph = StaticGraph::new(io::read_edges_from_file(&graph_path));
    let directory: LevelDirectory = io::read_from_file(&directory_path);
    let coordinates = io::read_vec_from_file::<FPCoordinate>(&coordinates_path);
    let partition = PackedPartition::of(&directory);
    let tree = CellTree::of(&directory, &partition, &graph, &coordinates);

    let mut pairs = pairs_of(&pairs_path);
    if let Ok(path) = std::env::var("TOOLBOX_ORDERING") {
        let ordering: NodeOrdering = io::read_from_file(&path);
        for pair in &mut pairs {
            pair.0 = ordering.new_of(pair.0);
            pair.1 = ordering.new_of(pair.1);
        }
        println!("{} pairs put through the numbering", pairs.len());
    }
    if let Ok(stride) = std::env::var("TOOLBOX_PAIR_STRIDE") {
        let stride: usize = stride.parse().expect("a stride");
        pairs = pairs.into_iter().step_by(stride.max(1)).collect();
        println!("every {stride} pairs kept, leaving {}", pairs.len());
    }

    // how much a block holds once read back, which is what a budget bounds
    let kib: usize = std::env::var("TOOLBOX_ARC_KIB")
        .ok()
        .and_then(|kib| kib.parse().ok())
        .unwrap_or(64);
    let arcs_in_a_block = kib * 1024 / 8;

    let path = Path::new(&arcs_path);
    let index_path = format!("{arcs_path}.index");
    let (map, first_edges) = if path.exists() && Path::new(&index_path).exists() {
        let map: BlockMap = io::read_from_file(&format!("{arcs_path}.map"));
        let first_edges: Vec<u64> = io::read_vec_from_file(&index_path);
        println!("{} blocks already packed in {arcs_path}", map.len());
        (map, first_edges)
    } else {
        let started = Instant::now();
        // the border levels ride in the blocks with the arcs they belong to
        let walked = BorderLevels::of(&graph, &partition);
        let (map, first_edges) = pack(
            &graph,
            &walked,
            Some(&tree),
            path,
            arcs_in_a_block,
            Codec::Lz4,
            3,
        )
        .expect("a graph to pack");
        println!(
            "packed {} arcs into {} blocks of about {kib} KiB in {:.1?}",
            Graph::number_of_edges(&graph),
            map.len(),
            started.elapsed()
        );
        io::write_to_file(&format!("{arcs_path}.map"), &map);
        io::write_vec_to_file(&index_path, &first_edges);
        (map, first_edges)
    };

    let (stored, unpacked) = map.bytes();
    let in_memory =
        (Graph::number_of_edges(&graph) * 8 + Graph::number_of_nodes(&graph) * 4) as f64;
    println!(
        "the arcs: {:.1} MiB in memory, {:.1} MiB packed, {:.1} MiB on the file ({:.0}% of memory)",
        in_memory / MIB as f64,
        unpacked as f64 / MIB as f64,
        stored as f64 / MIB as f64,
        100.0 * stored as f64 / in_memory,
    );

    println!(
        "\n{:>8} {:>10} {:>10} {:>9} {:>9} {:>9}",
        "budget", "median", "p95", "reads/q", "hit rate", "vs memory"
    );
    for &bytes in &budgets {
        let read =
            PagedGraph::open(path, map.clone(), &first_edges, bytes).expect("a graph to open");

        let mut over_memory = OneToManyDijkstra::new();
        let mut over_file = OneToManyDijkstra::new();
        // both warmed, so neither is measured growing its arrays
        for &(source, target, _) in pairs.iter().take(8) {
            over_memory.clear();
            over_memory.run(&graph, source, &[target]);
            over_file.clear();
            over_file.run(&read, source, &[target]);
        }

        let mut took = Vec::with_capacity(pairs.len());
        let mut memory_took = Vec::with_capacity(pairs.len());
        let mut wrong = 0_u64;
        let before = read.faults();
        let mut timings = String::new();
        let writing = std::env::var("TOOLBOX_TIMINGS").ok();
        for &(source, target, rank) in &pairs {
            over_file.clear();
            let started = Instant::now();
            over_file.run(&read, source, &[target]);
            let paged_nanos = started.elapsed().as_nanos() as u64;
            took.push(paged_nanos);

            over_memory.clear();
            let started = Instant::now();
            over_memory.run(&graph, source, &[target]);
            let memory_nanos = started.elapsed().as_nanos() as u64;
            memory_took.push(memory_nanos);

            let (from_file, from_memory) =
                (over_file.distance(target), over_memory.distance(target));
            if from_file != from_memory {
                wrong += 1;
            }
            if writing.is_some() {
                use std::fmt::Write;
                let _ = writeln!(
                    timings,
                    "dijkstra,{source},{target},{rank},{memory_nanos},{from_memory}"
                );
                let _ = writeln!(
                    timings,
                    "dijkstra-offline,{source},{target},{rank},{paged_nanos},{from_file}"
                );
            }
        }
        if let Some(at) = &writing {
            let name = format!("{at}.{}mib.csv", bytes / MIB);
            let mut out = File::create(&name).expect("somewhere to write the timings");
            writeln!(out, "engine,source,target,rank,nanos,distance").expect("a header");
            out.write_all(timings.as_bytes()).expect("the timings");
            println!("  wrote {name}");
        }

        took.sort_unstable();
        memory_took.sort_unstable();
        let faults = read.faults();
        let asked = faults.hits + faults.misses - before.hits - before.misses;
        let median = took[took.len() / 2] as f64 / 1000.0;
        let in_memory = memory_took[memory_took.len() / 2] as f64 / 1000.0;
        println!(
            "{:>8} {:>10} {:>10} {:>9} {:>9} {:>9}{}",
            format!("{} MiB", bytes / MIB),
            format!("{median:.0}us"),
            format!("{:.0}us", took[took.len() * 95 / 100] as f64 / 1000.0),
            format!(
                "{:.1}",
                (faults.reads - before.reads) as f64 / pairs.len() as f64
            ),
            format!(
                "{:.1}%",
                100.0 * (faults.hits - before.hits) as f64 / asked.max(1) as f64
            ),
            format!("{:.2}x", median / in_memory),
            if wrong == 0 {
                String::new()
            } else {
                format!("  {wrong} WRONG")
            },
        );
    }
}
