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

use std::{env::args, path::Path, sync::Arc, time::Instant};

use toolbox_rs::{
    block_map::BlockMap,
    block_store::BlockStore,
    cell_tree::CellTree,
    customization::Customization,
    graph::{Arcs, NodeID},
    io,
    level_directory::LevelDirectory,
    mld_query::{MldQuery, SparseMldQuery},
    node_ordering::NodeOrdering,
    overlay::Overlay,
    packed_partition::PackedPartition,
    paged_graph::{GraphIndex, PagedGraph},
    paged_overlay::{Budget, Footing, PagedOverlay},
    path_unpacking::Unpacker,
    pool::Pool,
    static_graph::StaticGraph,
};

const MIB: f64 = (1024 * 1024) as f64;

/// What this process is holding, in bytes.
///
/// # Not the resident set
///
/// `ps` reports a resident set, and on a Mac that counts pages the process
/// does not own: the text and constant data of every library it links, which
/// are file backed and shared with every other process using them, and pages
/// the allocator has freed but not yet handed back, which the kernel may take
/// at any time. Measured that way this instance reads at four hundred and
/// sixty megabytes, of which two hundred and seventeen is shared library and
/// a hundred and forty is memory nobody is using.
///
/// What a budget means is what the process would have to be given if it were
/// alone, which is the physical footprint: dirty private pages plus its share
/// of what it compresses to. It is the number a memory limit is enforced
/// against, and it reads a hundred and thirty six against the same run.
#[cfg(target_os = "macos")]
fn resident() -> u64 {
    // rusage_info_v2 is the first with a footprint in it
    #[repr(C)]
    #[derive(Default)]
    struct RUsageV2 {
        uuid: [u8; 16],
        user_time: u64,
        system_time: u64,
        pkg_idle_wkups: u64,
        interrupt_wkups: u64,
        pageins: u64,
        wired_size: u64,
        resident_size: u64,
        phys_footprint: u64,
        proc_start_abstime: u64,
        proc_exit_abstime: u64,
        child_user_time: u64,
        child_system_time: u64,
        child_pkg_idle_wkups: u64,
        child_interrupt_wkups: u64,
        child_pageins: u64,
        child_elapsed_abstime: u64,
        diskio_bytesread: u64,
        diskio_byteswritten: u64,
    }

    let mut held = RUsageV2::default();
    let got = unsafe {
        libc::proc_pid_rusage(
            std::process::id() as i32,
            2,
            std::ptr::from_mut(&mut held).cast(),
        )
    };
    if got == 0 { held.phys_footprint } else { 0 }
}

/// Everywhere else, the resident set out of the operating system.
#[cfg(not(target_os = "macos"))]
fn resident() -> u64 {
    #[cfg(target_os = "linux")]
    {
        let statm = std::fs::read_to_string("/proc/self/statm").unwrap_or_default();
        let pages: u64 = statm
            .split_whitespace()
            .nth(1)
            .and_then(|field| field.parse().ok())
            .unwrap_or(0);
        return pages * 4096;
    }
    #[cfg(not(target_os = "linux"))]
    0
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
            panic!("usage: paged_rss <graph> <directory> <pairs> <blocks> [MiB]: missing {what}")
        })
    };
    let _graph_path = next("graph");
    let _directory_path = next("directory");
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

    // No directory is read either. The partition was settled when the store
    // was packed, and building it from a directory means reading seventy one
    // mebibytes of one to throw it away again -- which puts the high-water
    // mark of the process above anything it goes on to hold.
    // One pool, made before anything is opened, because everything draws on
    // it: the arcs, the tables, the ways, and the partition and tree arrays
    // that are pinned into it. What stands outside it is the footing, which is
    // a third of a mebibyte, so the budget less a mebibyte is the pool's.
    let early = Pool::of(bytes.saturating_sub(MIB as usize));
    let partition = PackedPartition::open(
        Path::new(&format!(
            "{}.partition",
            std::env::var("TOOLBOX_ARCS").unwrap_or_default()
        )),
        &early,
    )
    .expect("a partition beside the arcs");
    at = report("the partition, read not built", at);
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
    // The tree written beside the arcs is already the query half: reading the
    // whole one to throw the build half away would put twenty two mebibytes
    // through the process on the way to holding seven.
    let arcs = std::env::var("TOOLBOX_ARCS").unwrap_or_else(|_| {
        panic!("set TOOLBOX_ARCS to a pack of arcs; this measures an instance that pages both")
    });
    let mut tree: CellTree = io::read_from_file(&format!("{arcs}.tree"));
    assert!(!tree.whole(), "the tree beside the arcs was not trimmed");
    // the two arrays a query asks of a tree are read as well, so what stands
    // in their place is an offset a level rather than an entry a cell
    tree.open_cells(Path::new(&format!("{arcs}.cells")), &early)
        .expect("the cells beside the arcs");
    at = report("the block map and the cell tree", at);
    for (part, bytes) in tree.bytes_by_part() {
        println!("    the tree's {part:<12} {:>7.1} MiB", bytes as f64 / MIB);
    }

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
        // levels held outright are read at open and never let go of, so a run
        // that must never exceed its budget while starting up may want none
        // Nothing is held outside the pool by default: a level held outright
        // is room the pool does not have, and the pool is where the pinning
        // that matters now happens.
        pinned_share: std::env::var("TOOLBOX_PIN_SHARE")
            .ok()
            .and_then(|share| share.parse().ok())
            .unwrap_or(0.0),
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
            cell_tree: tree.bytes() as u64,
            // nothing: the queue keeps only what a run touched
            searches: 0,
        };
        let pool = Arc::clone(&early);
        println!(
            "one pool of {:.1} MiB for the arcs, the tables, the ways and the pins",
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
    // every Nth pair, so a run can be made shorter without losing the spread
    // of ranks: what RSS does against the number of queries is what says
    // whether something is growing with the run or standing still
    if let Ok(stride) = std::env::var("TOOLBOX_PAIR_STRIDE") {
        let stride: usize = stride.parse().expect("a stride");
        pairs = pairs.into_iter().step_by(stride.max(1)).collect();
        println!("every {stride} pairs kept, leaving {}", pairs.len());
    }

    let mut query = SparseMldQuery::new();
    // the ways go into the same pool the arcs and the tables draw on
    let mut unpacker = Unpacker::sharing(Arc::clone(paged.pool()));
    let mut reads_of = String::new();
    if let Some(&(source, target, _)) = pairs.first() {
        query.run(&paged, source, &[target]);
    }
    at = report("what a search and an unpacker want", at);

    // A reference to answer against, where one is asked for: the same query
    // over an instance that holds everything. It costs what an offline
    // instance was built not to, which is why it is a switch and not the
    // default -- what it is here for is to say that the answers are the same.
    let reference = std::env::var("TOOLBOX_VERIFY").ok().map(|at| {
        let edges = io::read_edges_from_file(&at);
        let directory: LevelDirectory = io::read_from_file(&_directory_path);
        println!("a reference instance is being built to answer against");
        Customization::new(StaticGraph::new(edges), directory)
    });
    if let Some(held) = &reference {
        for level in 0..held.directory().levels() {
            held.level(level);
        }
        println!("the reference is customized");
    }

    let mut ways = 0_usize;
    let mut wrong = 0_usize;
    let mut wrong_way = 0_usize;
    let mut timings = String::new();
    let writing = std::env::var("TOOLBOX_TIMINGS").ok();
    let mut over_memory = MldQuery::new();
    let mut memory_unpacker = reference
        .as_ref()
        .map_or_else(Unpacker::default, Unpacker::for_instance);

    for &(source, target, rank) in &pairs {
        let before = paged.pool().faults().reads;
        query.clear();
        let started = Instant::now();
        query.run(&paged, source, &[target]);
        let offline_nanos = started.elapsed().as_nanos() as u64;
        let offline = query.distance(target);

        let mut way = None;
        let started = Instant::now();
        if let Some(packed) = query.retrieve_packed_path(target)
            && let Ok(put_back) = unpacker.unpack(&paged, &packed)
        {
            way = Some(put_back);
            ways += 1;
        }
        let unpack_nanos = started.elapsed().as_nanos() as u64;
        // every read the whole instance made for this pair, of every kind
        let reads = paged.pool().faults().reads - before;

        if let Some(held) = &reference {
            over_memory.clear();
            let started = Instant::now();
            over_memory.run(held, source, &[target]);
            let memory_nanos = started.elapsed().as_nanos() as u64;
            if over_memory.distance(target) != offline {
                wrong += 1;
            }
            // and the way, which has to be the same way and not merely as long
            if let Some(packed) = over_memory.retrieve_packed_path(target)
                && let Ok(theirs) = memory_unpacker.unpack(held, &packed)
                && way.as_ref() != Some(&theirs)
            {
                wrong_way += 1;
            }
            if writing.is_some() {
                use std::fmt::Write as _;
                let _ = writeln!(
                    timings,
                    "mld,{source},{target},{rank},{memory_nanos},{}",
                    over_memory.distance(target)
                );
            }
        }
        if writing.is_some() {
            use std::fmt::Write as _;
            let _ = writeln!(
                timings,
                "offline,{source},{target},{rank},{offline_nanos},{offline}"
            );
            let _ = writeln!(
                timings,
                "offline-unpack,{source},{target},{rank},{unpack_nanos},{offline}"
            );
            let _ = writeln!(
                reads_of,
                "offline,{source},{target},{rank},{reads},{offline}"
            );
        }
    }
    if let Some(at) = &writing {
        use std::io::Write as _;
        let mut out = std::fs::File::create(format!("{at}.times.csv")).expect("a file");
        writeln!(out, "engine,source,target,rank,nanos,distance").expect("a header");
        out.write_all(timings.as_bytes()).expect("the timings");
        let mut out = std::fs::File::create(format!("{at}.reads.csv")).expect("a file");
        writeln!(out, "engine,source,target,rank,nanos,distance").expect("a header");
        out.write_all(reads_of.as_bytes()).expect("the reads");
        println!("wrote {at}.times.csv and {at}.reads.csv");
    }
    if reference.is_some() {
        println!(
            "{} pairs answered against a reference: {wrong} wrong distances, {wrong_way} wrong ways",
            pairs.len()
        );
        assert_eq!(
            wrong, 0,
            "the offline instance answered a different distance"
        );
        assert_eq!(wrong_way, 0, "the offline instance found a different way");
    }
    let full = report("after the queries and the ways", at);
    println!(
        "  the queue came to {:.1} MiB, which no budget bounds",
        query.bytes() as f64 / MIB
    );

    // A run held open, so that vmmap and heap can be pointed at it: the
    // resident set says how much there is and neither of those has to be
    // guessed at afterwards.
    if let Ok(seconds) = std::env::var("TOOLBOX_HOLD") {
        let seconds: u64 = seconds.parse().unwrap_or(120);
        println!("HOLDING pid {} for {seconds}s", std::process::id());
        use std::io::Write as _;
        let _ = std::io::stdout().flush();
        std::thread::sleep(std::time::Duration::from_secs(seconds));
    }

    println!(
        "  of the pool, {:.1} MiB is pinned: the partition and the tree arrays",
        paged.pool().pinned() as f64 / MIB
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
    line(
        "the cell tree",
        tree_bytes,
        "children, parents, boxes and facts",
    );
    line("the cell tables", tables, "<- the budget governs this one");
    println!("  {:-<34} {:->13}", "", "");
    let accounted = footing.total() - footing.searches + tables;
    line("accounted for", accounted, "");
    line("the footprint", full, "");
    println!(
        "  {:<34} {:>8.1} MiB   the search's arrays and what the",
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
