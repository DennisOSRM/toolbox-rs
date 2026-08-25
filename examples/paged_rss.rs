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

use std::{env::args, path::Path, process, sync::Arc, time::Instant};

use toolbox_rs::{
    block_map::BlockMap,
    block_store::BlockStore,
    cell_tree::CellTree,
    graph::{Arcs, NodeID},
    io,
    level_directory::LevelDirectory,
    mld_query::SparseMldQuery,
    node_ordering::NodeOrdering,
    overlay::Overlay,
    packed_partition::PackedPartition,
    paged_graph::{GraphIndex, PagedGraph},
    paged_overlay::{Budget, Footing, PagedOverlay},
    path_unpacking::Unpacker,
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
    let _graph_path = next("graph");
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

    // No graph is read at all. It was here for the border levels, which are
    // now written down when the store is packed and read back, and nothing
    // else an instance does wants every arc at once.
    let mut at = report("no graph is read", start);

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
    println!(
        "  the partition is {} runs over {} nodes, {:.1} MiB",
        partition.runs(),
        partition.number_of_nodes(),
        partition.bytes() as f64 / MIB
    );
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
    // Where a pack of the arcs is at hand, the graph pages too and the footing
    // stands for its index rather than for its arcs.
    let arcs = std::env::var("TOOLBOX_ARCS").unwrap_or_else(|_| {
        panic!("set TOOLBOX_ARCS to a pack of arcs; this measures an instance that pages both")
    });
    let (paged_arcs, pool, footing) = {
        let at = &arcs;
        let arc_map: BlockMap = io::read_from_file(&format!("{at}.map"));
        let first_edges: Vec<u64> = io::read_vec_from_file(&format!("{at}.index"));
        // The pool is sized before anything is opened, because everything that
        // reads draws on the same one: the arcs, the tables and the ways an
        // unpacker keeps. Its size is the budget less what stands whatever
        // happens, and the index is what the graph stands for.
        let index = GraphIndex::of(&arc_map, &first_edges);
        let footing = Footing {
            graph: index.bytes() as u64,
            partition: partition.bytes() as u64,
            // nothing: the levels ride in the arc blocks
            border_levels: 0,
            block_map: (map.len() * size_of::<toolbox_rs::block_map::BlockEntry>()) as u64,
            cell_tree: (0..tree.levels())
                .map(|level| {
                    (tree.cells_on_level(level) * size_of::<toolbox_rs::cell_tree::CellFacts>())
                        as u64
                })
                .sum(),
            // nothing: the queue keeps only what a run touched
            searches: 0,
        };
        let pool = budget.pool_for(&tree, &footing);
        println!(
            "one pool of {:.1} MiB for the arcs, the tables and the ways alike",
            pool.budget() as f64 / MIB
        );
        let read = PagedGraph::open(Path::new(at), arc_map, &first_edges, Arc::clone(&pool))
            .expect("a graph to open");
        (read, pool, footing)
    };
    at = report("the arcs, which page too", at);

    // the sparse queue, since an array over the nodes of a continent is more
    // than half of a hundred and twenty eight megabyte budget standing still

    let (pinned, cache) = budget.split(&tree, &footing);
    match budget.for_tables(&footing) {
        Ok(left) => println!(
            "\nbudget {:.0} MiB: {:.1} MiB is the footing, {:.1} MiB left for tables",
            bytes as f64 / MIB,
            footing.total() as f64 / MIB,
            left as f64 / MIB,
        ),
        Err(short) => {
            println!("\n{short}");
            println!(
                "  the graph                          {:>8.1} MiB",
                footing.graph as f64 / MIB
            );
            println!(
                "  the partition                      {:>8.1} MiB",
                footing.partition as f64 / MIB
            );
            println!(
                "  the border levels                  {:>8.1} MiB",
                footing.border_levels as f64 / MIB
            );
            println!(
                "  the block map and the cell tree    {:>8.1} MiB",
                (footing.block_map + footing.cell_tree) as f64 / MIB
            );
            println!(
                "  what a search wants                {:>8.1} MiB",
                footing.searches as f64 / MIB
            );
            println!(
                "  {:-<34} {:->13}\n  the footing                        {:>8.1} MiB",
                "",
                "",
                footing.total() as f64 / MIB
            );
            println!(
                "\nNothing is asked, since a budget that leaves no room for a table\n                 spends the whole run reading one and letting it go again."
            );
            return;
        }
    }
    let store = BlockStore::open(Path::new(&blocks_path), map, tree).expect("a store");
    let opening = Instant::now();
    // Nothing is read for the border levels: they ride in the blocks with the
    // arcs they belong to, so the graph is what answers for them and the same
    // handle serves as both.
    let held = Arc::new(paged_arcs);
    let paged = PagedOverlay::sharing(
        store,
        Arc::clone(&held),
        partition,
        Arc::clone(&held),
        budget,
        pool,
    );
    let open = opening.elapsed();
    at = report("the held levels, read and unpacked", at);

    // the arrays a search wants, which go with the nodes of the graph and not
    // with the budget: one query is run to make it allocate them
    let mut query = SparseMldQuery::new();
    // the ways go into the same pool the arcs and the tables draw on
    let mut unpacker = Unpacker::sharing(Arc::clone(paged.pool()));
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
    println!(
        "  the queue came to {:.1} MiB, which no budget bounds",
        query.bytes() as f64 / MIB
    );

    let faults = paged.faults();
    let tables = paged.pinned_bytes() as u64 + faults.held as u64;
    let pool = paged.pool().faults();
    println!(
        "\nthe pool: {:.1} MiB of {:.1} MiB held, most ever {:.1} MiB, {} let go of",
        pool.held as f64 / MIB,
        paged.pool().budget() as f64 / MIB,
        pool.highest as f64 / MIB,
        pool.evicted,
    );
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
    line("the graph", footing.graph, "8 bytes an arc, 4 a node");
    line(
        "the partition",
        footing.partition,
        "a run a cell, not a word a node",
    );
    line(
        "the border levels",
        footing.border_levels,
        "nothing: they ride in the arc blocks",
    );
    line("the block map", map_bytes, "one entry a block");
    line("the cell tree", tree_bytes, "one entry a cell");
    line("the cell tables", tables, "<- the budget governs this one");
    println!("  {:-<34} {:->13}", "", "");
    let accounted = footing.total() - footing.searches + tables;
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
        "The whole instance -- graph, partition and all -- stands in {:.1} MiB before tables.",
        footing.total() as f64 / MIB
    );
    println!(
        "It would be {:.1} MiB with the arcs held and a word a node: the arcs alone are {:.1}.",
        (footing.total() + arcs * 8 + nodes * 4 + nodes * 16 - footing.partition) as f64 / MIB,
        (arcs * 8 + nodes * 4) as f64 / MIB,
    );
    println!(
        "Every cell table of every level would be {:.1} MiB unpacked, so the budget holds {:.0}%.",
        all_tables as f64 / MIB,
        100.0 * tables as f64 / all_tables as f64
    );
}
