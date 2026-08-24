//! What an offline instance actually costs in memory.
//!
//! A budget governs the cell tables and nothing else, so it is not what the
//! process comes to. This walks the offline path alone -- no customization in
//! memory, nothing kept that only the packer wanted -- and reads the resident
//! set at each step, so the budget can be laid against the things beside it.
//!
//!   paged_rss <graph> <directory> <pairs> <blocks> [MiB]
//!
//! The steps are cumulative: each line is what the process holds once that
//! step is done, and the difference between two lines is what the step added.

use std::{env::args, path::Path, process, time::Instant};

use toolbox_rs::{
    block_map::BlockMap,
    block_store::BlockStore,
    border_levels::BorderLevels,
    cell_tree::CellTree,
    graph::{Graph, NodeID},
    io,
    level_directory::LevelDirectory,
    mld_query::MldQuery,
    node_ordering::NodeOrdering,
    overlay::Overlay,
    packed_partition::PackedPartition,
    paged_overlay::{Budget, PagedOverlay},
    path_unpacking::Unpacker,
    static_graph::StaticGraph,
};

const MIB: f64 = (1024 * 1024) as f64;

/// The resident set of this process, in bytes.
///
/// Read from the operating system rather than added up from what was
/// allocated: what is wanted is what the machine is holding, which includes
/// the allocator's own slack and excludes whatever it has handed back.
fn resident() -> u64 {
    #[cfg(target_os = "linux")]
    {
        // statm is in pages, and the second field is the resident set
        let statm = std::fs::read_to_string("/proc/self/statm").unwrap_or_default();
        let pages: u64 = statm
            .split_whitespace()
            .nth(1)
            .and_then(|field| field.parse().ok())
            .unwrap_or(0);
        return pages * 4096;
    }
    #[cfg(not(target_os = "linux"))]
    {
        // ps reports kibibytes
        let out = process::Command::new("ps")
            .args(["-o", "rss=", "-p", &process::id().to_string()])
            .output();
        let kib: u64 = out
            .ok()
            .and_then(|out| String::from_utf8(out.stdout).ok())
            .and_then(|text| text.trim().parse().ok())
            .unwrap_or(0);
        kib * 1024
    }
}

fn report(step: &str, before: u64) -> u64 {
    let now = resident();
    println!(
        "{step:<38} {:>9.1} MiB   {:+9.1} MiB",
        now as f64 / MIB,
        (now as f64 - before as f64) / MIB
    );
    now
}

fn pairs_of(path: &str) -> Vec<(NodeID, NodeID)> {
    std::fs::read_to_string(path)
        .expect("a file of pairs")
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split(',');
            let source = fields.next()?.trim().parse().ok()?;
            let target = fields.next()?.trim().parse().ok()?;
            Some((source, target))
        })
        .collect()
}

fn main() {
    let mut argv = args().skip(1);
    let mut next = |what: &str| {
        argv.next().unwrap_or_else(|| {
            panic!("usage: paged_rss <graph> <directory> <pairs> <blocks> [MiB]: missing {what}")
        })
    };
    let graph_path = next("graph");
    let directory_path = next("directory");
    let pairs_path = next("pairs");
    let blocks_path = next("blocks");
    let bytes = argv
        .next()
        .map_or(128, |mib| mib.parse::<usize>().expect("a size in MiB"))
        * 1024
        * 1024;

    println!("{:<38} {:>9} {:>14}", "step", "resident", "of which new");
    let start = resident();
    println!("{:<38} {:>9.1} MiB", "an empty process", start as f64 / MIB);

    let graph = StaticGraph::new(io::read_edges_from_file(&graph_path));
    let mut at = report("the graph", start);

    // The directory is what the partition and the border levels are built out
    // of, and neither keeps it, so an offline instance drops it here. It is
    // counted anyway, since a process that opens a store has to hold it for as
    // long as it takes to build the two.
    let partition = {
        let directory: LevelDirectory = io::read_from_file(&directory_path);
        at = report("  ... and the level directory", at);
        PackedPartition::of(&directory)
    };
    at = report("the partition, directory dropped", at);
    let border_levels = BorderLevels::of(&graph, &partition);
    at = report("the border levels", at);

    // The pairs are translated here rather than later, and what translates them
    // is dropped again: a NodeOrdering is one entry a node each way and comes
    // to more than the whole budget, and it belongs to the measurement rather
    // than to an instance that answers queries.
    let mut pairs = pairs_of(&pairs_path);
    if let Ok(path) = std::env::var("TOOLBOX_ORDERING") {
        let ordering: NodeOrdering = io::read_from_file(&path);
        for pair in &mut pairs {
            pair.0 = ordering.new_of(pair.0);
            pair.1 = ordering.new_of(pair.1);
        }
        at = report("  ... and the numbering, since dropped", at);
    }

    let map: BlockMap = io::read_from_file(&format!("{blocks_path}.map"));
    let tree: CellTree = io::read_from_file(&format!("{blocks_path}.tree"));
    at = report("the block map and the cell tree", at);

    let map_bytes = (map.len() * size_of::<toolbox_rs::block_map::BlockEntry>()) as u64;
    // what every cell of every level would come to unpacked, which is what the
    // budget is a fraction of
    let all_tables: u64 = (0..tree.levels())
        .map(|level| tree.unpacked_bytes(level))
        .sum();
    let tree_bytes = (0..tree.levels())
        .map(|level| {
            (tree.cells_on_level(level) * size_of::<toolbox_rs::cell_tree::CellFacts>()) as u64
        })
        .sum::<u64>();

    let budget = Budget {
        bytes,
        pinned_share: 0.5,
    };
    let (pinned, cache) = budget.split(&tree);
    let store = BlockStore::open(Path::new(&blocks_path), map, tree).expect("a store");
    let opening = Instant::now();
    let paged = PagedOverlay::within(store, graph, partition, border_levels, budget);
    let open = opening.elapsed();
    at = report("the held levels, read and unpacked", at);

    // the arrays a search wants, which go with the nodes of the graph and not
    // with the budget: one query is run to make it allocate them
    let mut query = MldQuery::new();
    let mut unpacker = Unpacker::for_instance(&paged);
    if let Some(&(source, target)) = pairs.first() {
        query.run(&paged, source, &[target]);
    }
    at = report("what a search and an unpacker want", at);

    let mut ways = 0_usize;
    for &(source, target) in &pairs {
        query.clear();
        query.run(&paged, source, &[target]);
        if let Some(packed) = query.retrieve_packed_path(target)
            && unpacker.unpack(&paged, &packed).is_ok()
        {
            ways += 1;
        }
    }
    let full = report("after the queries and the ways", at);

    let faults = paged.faults();
    let tables = paged.pinned_bytes() as u64 + faults.held as u64;
    println!();
    println!(
        "budget {:.0} MiB: {:.1} MiB held outright, {:.1} MiB for the cache, opened in {open:.1?}",
        bytes as f64 / MIB,
        pinned as f64 / MIB,
        cache as f64 / MIB,
    );
    println!(
        "{} pairs asked, {ways} ways put back, {} blocks read",
        pairs.len(),
        faults.reads
    );

    println!();
    println!("what the process is holding");
    let line = |what: &str, bytes: u64, note: &str| {
        println!("  {what:<34} {:>8.1} MiB   {note}", bytes as f64 / MIB);
    };
    // Sizes the structures know, rather than differences between one reading of
    // the resident set and the next: memory that is given back does not come
    // off the resident set, so a difference charges whatever a step dropped to
    // the step after it. The level directory and the numbering are both gone by
    // now and both are still resident.
    let nodes = paged.graph().number_of_nodes() as u64;
    let arcs = paged.graph().number_of_edges() as u64;
    line(
        "the graph",
        arcs * 8 + nodes * 4,
        "8 bytes an arc, 4 a node",
    );
    line("the partition", nodes * 16, "one u128 a node");
    line("the border levels", nodes, "one byte a node");
    line("the block map", map_bytes, "one entry a block");
    line("the cell tree", tree_bytes, "one entry a cell");
    line("the cell tables", tables, "<- the budget governs this one");
    println!("  {:-<34} {:->13}", "", "");
    let accounted = arcs * 8 + nodes * 21 + map_bytes + tree_bytes + tables;
    line("accounted for", accounted, "");
    line("resident", full, "");
    println!(
        "  {:<34} {:>8.1} MiB   the search's arrays, the unpacker's",
        "the difference",
        (full - accounted.min(full)) as f64 / MIB
    );
    println!(
        "  {:<34} {:>8}       ways, and what the allocator kept",
        "", ""
    );
    println!();
    println!(
        "A budget of {:.0} MiB governs {:.1} MiB of {:.1} MiB resident: {:.0}% of it.",
        bytes as f64 / MIB,
        tables as f64 / MIB,
        full as f64 / MIB,
        100.0 * tables as f64 / full as f64,
    );
    println!(
        "The graph and the partition alone are {:.1} MiB, and no budget reaches them.",
        (arcs * 8 + nodes * 20) as f64 / MIB
    );
    println!(
        "Every cell table of every level would be {:.1} MiB unpacked, so the budget holds {:.0}%.",
        all_tables as f64 / MIB,
        100.0 * tables as f64 / all_tables as f64
    );
}
