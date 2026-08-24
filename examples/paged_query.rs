//! What a query costs when the cells are on a disk rather than in memory.
//!
//! ```text
//! paged_query <graph> <directory> <coordinates> <pairs.csv> <blocks> [MiB ...]
//! ```
//!
//! Wants an instance whose cells are numbered in key order and whose nodes are
//! numbered by cell path, which is what `renumber --numbering cell-path
//! --cells-in-key-order` writes.
//!
//! Builds the store if the blocks file is not there, then for each budget
//! opens it afresh, runs every pair, and says what it cost. Every answer is
//! compared with the same search over the cells in memory, so the run says
//! whether the store is right as well as how fast it is.

use std::{
    env::args,
    fs::File,
    io::{BufRead, BufReader},
    path::Path,
    time::Instant,
};

use toolbox_rs::{
    block_codec::Codec,
    block_map::BlockMap,
    block_store::{BlockStore, BlockWriter},
    border_levels::BorderLevels,
    cell_block::{CellBlock, CellEntry},
    cell_tree::CellTree,
    customization::Customization,
    geometry::FPCoordinate,
    graph::NodeID,
    io,
    level_directory::{CellId, LevelDirectory},
    mld_query::MldQuery,
    packed_partition::PackedPartition,
    paged_overlay::{Budget, PagedOverlay},
    static_graph::StaticGraph,
};

const MIB: usize = 1024 * 1024;

fn megabytes(bytes: u64) -> String {
    format!("{:.1} MiB", bytes as f64 / MIB as f64)
}

/// Writes every cell of a customization into a store, a level at a time.
fn pack(
    customization: &Customization,
    tree: &CellTree,
    path: &Path,
    target: usize,
) -> std::io::Result<BlockMap> {
    let mut writer = BlockWriter::create(path)?;
    for level in 0..tree.levels() {
        let border_leads = level == 0;
        let count = tree.cells_on_level(level);
        let mut at = 0;
        while at < count {
            // as many cells as fit the target, and never fewer than one
            let (mut upto, mut carrying) = (at, 0_usize);
            while upto < count && (carrying < target || upto == at) {
                let wide = tree.facts(level, upto as CellId).on_border as usize;
                carrying += wide * wide * 4;
                upto += 1;
            }

            let mut matrices = Vec::new();
            let mut widths = Vec::new();
            let mut places = Vec::new();
            let mut holds = Vec::new();
            for cell in at..upto {
                let cell = cell as CellId;
                let held = customization.distances_of(level, cell);
                let wide = held.map_or(0, |table| table.border_nodes_of().len());
                let mut matrix = Vec::with_capacity(wide * wide);
                if let Some(table) = held {
                    for source in 0..wide {
                        matrix.extend_from_slice(table.row(source));
                    }
                }
                let begins = tree.nodes_begin(level, cell);
                places.push(if border_leads {
                    Vec::new()
                } else {
                    held.map_or_else(Vec::new, |table| {
                        table
                            .border_nodes_of()
                            .iter()
                            .map(|&node| node - begins)
                            .collect()
                    })
                });
                matrices.push(matrix);
                widths.push(wide);
                holds.push(tree.facts(level, cell).nodes as usize);
            }

            let entries = matrices
                .iter()
                .zip(&widths)
                .zip(&places)
                .zip(&holds)
                .map(|(((matrix, &wide), places), &holds)| CellEntry {
                    matrix,
                    wide,
                    places,
                    holds,
                })
                .collect::<Vec<_>>();
            let block = CellBlock::of(level, at as CellId, &entries, border_leads);
            let last = (upto - 1) as CellId;
            let keys = (
                tree.range_of(level, at as CellId).0,
                tree.range_of(level, last).1,
            );
            let first_node = tree.nodes_begin(level, at as CellId);
            let nodes = (
                first_node,
                tree.nodes_begin(level, last) + tree.facts(level, last).nodes - first_node,
            );
            writer.push(
                &block,
                keys,
                (at as CellId, (upto - at) as u32),
                nodes,
                Codec::Lz4,
                3,
            )?;
            at = upto;
        }
    }
    writer.finish()
}

fn pairs_of(path: &str) -> Vec<(NodeID, NodeID)> {
    let mut pairs = Vec::new();
    for line in BufReader::new(File::open(path).expect("the pairs"))
        .lines()
        .skip(1)
    {
        let line = line.expect("a line");
        let mut fields = line.split(',');
        if let (Some(Ok(source)), Some(Ok(target))) =
            (fields.next().map(str::parse), fields.next().map(str::parse))
        {
            pairs.push((source, target));
        }
    }
    pairs
}

fn main() {
    env_logger::init();
    let mut argv = args().skip(1);
    let mut next = |what: &str| {
        argv.next().unwrap_or_else(|| {
            panic!("usage: paged_query <graph> <directory> <coordinates> <pairs> <blocks> [MiB ...]: missing {what}")
        })
    };
    let graph_path = next("graph");
    let directory_path = next("directory");
    let coordinates_path = next("coordinates");
    let pairs_path = next("pairs");
    let blocks_path = next("blocks");
    let budgets = argv
        .map(|mib| mib.parse::<usize>().expect("a size in MiB") * MIB)
        .collect::<Vec<_>>();
    let budgets = if budgets.is_empty() {
        vec![75 * MIB, 150 * MIB, 300 * MIB, 700 * MIB]
    } else {
        budgets
    };

    let edges = io::read_edges_from_file(&graph_path);
    let directory: LevelDirectory = io::read_from_file(&directory_path);
    let coordinates = io::read_vec_from_file::<FPCoordinate>(&coordinates_path);
    let graph = StaticGraph::new(edges.clone());
    let partition = PackedPartition::of(&directory);
    let tree = CellTree::of(&directory, &partition, &graph, &coordinates);
    let pairs = pairs_of(&pairs_path);
    println!("{} pairs, {} levels", pairs.len(), tree.levels());

    let in_memory = Customization::new(StaticGraph::new(edges.clone()), directory.clone());
    let path = Path::new(&blocks_path);
    if !path.exists() {
        let started = Instant::now();
        // everything is customized before anything is written
        for level in 0..tree.levels() {
            for cell in 0..tree.cells_on_level(level) {
                let _ = in_memory.distances_of(level, cell as CellId);
            }
        }
        // how large a block is cut, in kibibytes of raw entries
        let target = std::env::var("TOOLBOX_BLOCK_KIB")
            .ok()
            .and_then(|kib| kib.parse::<usize>().ok())
            .unwrap_or(4096)
            * 1024;
        let map = pack(&in_memory, &tree, path, target).expect("a store to write");
        let (stored, unpacked) = map.bytes();
        println!(
            "wrote {} blocks in {:.1?}: {} on disk, {} unpacked",
            map.len(),
            started.elapsed(),
            megabytes(stored),
            megabytes(unpacked)
        );
        io::write_to_file(&format!("{blocks_path}.map"), &map);
        io::write_to_file(&format!("{blocks_path}.tree"), &tree);
    }
    let map: BlockMap = io::read_from_file(&format!("{blocks_path}.map"));
    let held_tree: CellTree = io::read_from_file(&format!("{blocks_path}.tree"));

    println!(
        "\n{:>7} {:>6} {:>10} {:>10} {:>9} {:>8} {:>9} {:>9} {:>9}",
        "budget", "held", "of which", "for cache", "open", "median", "p95", "reads/q", "hit rate"
    );
    for &bytes in &budgets {
        let budget = Budget::of(bytes);
        let store = BlockStore::open(path, map.clone(), held_tree.clone()).expect("a store");
        let opening = Instant::now();
        let paged = PagedOverlay::within(
            store,
            StaticGraph::new(edges.clone()),
            PackedPartition::of(&directory),
            BorderLevels::of(&graph, &partition),
            budget,
        );
        let open = opening.elapsed();
        let (pinned, cache) = budget.split(&held_tree);

        let mut over_file = MldQuery::new();
        let mut over_memory = MldQuery::new();
        let mut took = Vec::with_capacity(pairs.len());
        let mut wrong = 0_u64;
        let before = paged.faults();
        for &(source, target) in &pairs {
            over_file.clear();
            let started = Instant::now();
            over_file.run(&paged, source, &[target]);
            took.push(started.elapsed().as_nanos() as u64);
            over_memory.clear();
            over_memory.run(&in_memory, source, &[target]);
            if over_file.distance(target) != over_memory.distance(target) {
                wrong += 1;
            }
        }
        took.sort_unstable();
        let faults = paged.faults();
        let reads = faults.reads - before.reads;
        let asked = faults.hits + faults.misses - before.hits - before.misses;

        println!(
            "{:>7} {:>6} {:>10} {:>10} {:>9} {:>8} {:>9} {:>9} {:>9}{}",
            format!("{} MiB", bytes / MIB),
            format!("L{}+", paged.pinned_from()),
            megabytes(pinned),
            megabytes(cache as u64),
            format!("{open:.1?}"),
            format!("{:.0}us", took[took.len() / 2] as f64 / 1000.0),
            format!("{:.0}us", took[took.len() * 95 / 100] as f64 / 1000.0),
            format!("{:.1}", reads as f64 / pairs.len() as f64),
            format!(
                "{:.1}%",
                100.0 * (faults.hits - before.hits) as f64 / asked.max(1) as f64
            ),
            if wrong == 0 {
                String::new()
            } else {
                format!("  {wrong} WRONG")
            },
        );
    }
}
