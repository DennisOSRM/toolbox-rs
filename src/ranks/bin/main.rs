//! Draws pairs of nodes, says where each of them sits on the Dijkstra rank
//! axis, and times what a search costs for them.
//!
//! # Why two modes
//!
//! A rank plot is a picture of what a query costs against how far apart its
//! ends are, and "how far apart" is measured in what a plain search has to do
//! to get from one to the other: the rank of a target is the number of nodes
//! settled before it. Working that out means counting, and counting is not
//! free, so the pairs are drawn in one pass and timed in another. Nothing is
//! collected while the clock is running.
//!
//! ```text
//! ranks sample -g graph.toolbox -s 1000 -o pairs.csv
//! ranks time   -g graph.toolbox -i pairs.csv -e dijkstra -o timings.csv
//! ranks time   -g graph.toolbox -d levels.bin -i pairs.csv -e mld -o timings.csv
//! ```
//!
//! The two runs of `time` can be laid end to end, as each row says which
//! engine it came from.

mod command_line;

use std::{
    error::Error,
    fs::File,
    io::{BufRead, BufReader, BufWriter, Write},
    time::Instant,
};

use command_line::{Arguments, Check, Engine, Mode, Pack, Sample, Scans, Time};
use env_logger::{Builder, Env};
use indicatif::{ProgressBar, ProgressStyle};
use log::{info, warn};
use rand::{RngExt, SeedableRng, prelude::StdRng, seq::SliceRandom};
use rayon::prelude::*;
use rustc_hash::FxHashMap;

/// The bar the crate draws elsewhere, so this looks like the rest of it.
fn bar_of(count: usize, what: &str) -> ProgressBar {
    let bar = ProgressBar::new(count as u64);
    bar.set_style(
        ProgressStyle::default_spinner()
            .template(
                "{spinner:.green} [{elapsed_precise}] {wide_bar:.green/yellow} {pos}/{len} {msg}",
            )
            .expect("the template is not a template")
            .progress_chars("#>-"),
    );
    bar.set_message(what.to_string());
    bar
}

use toolbox_rs::{
    bidirectional_dijkstra::BidirectionalDijkstra,
    bidirectional_mld_query::{BidirectionalMldQuery, TrackedBidirectionalMldQuery},
    block_map::BlockMap,
    block_store::BlockStore,
    border_levels::BorderLevels,
    cell_tree::CellTree,
    customization::Customization,
    edge::InputEdge,
    edge_based::AnglePenalty,
    edge_based_overlay::{EdgeBasedOverlay, PagedEdgeBasedOverlay},
    geometry::FPCoordinate,
    graph::{Graph, NodeID},
    heap_stats::{Counters, RankTargets},
    io,
    level_directory::{CellId, LevelDirectory},
    mld_query::{MldQuery, TrackedMldQuery},
    node_ordering::NodeOrdering,
    overlay::Overlay,
    packed_partition::PackedPartition,
    paged_overlay::{PagedOverlay, pack_cells, pack_overlay},
    pool::Pool,
    static_graph::StaticGraph,
    unidirectional_dijkstra::{UnidirectionalDijkstra, UnidirectionalSearch},
};

/// A pair to time, and the rank its target sits at.
type ToTime = (NodeID, NodeID, usize);

/// The arcs a route may leave the source on, each at what it has cost so far,
/// and the arcs it may arrive at the target by.
type ArcEnds = (Vec<(NodeID, usize)>, Vec<(NodeID, usize)>);

/// What one pair cost: the pair itself, its rank, the nanoseconds, and what
/// the search said the distance was. The pair is carried through so the two
/// engines can be held against each other rather than trusted to come out in
/// the same order.
type Timing = (NodeID, NodeID, usize, u128, usize);

/// A pair of nodes and where the target sits on the rank axis.
///
/// The rank is the count of nodes a plain search settled before it reached
/// the target, which is what a plain search cost for this pair and the axis a
/// rank plot is drawn against. There is no separate settled count to write
/// down: the two are the same number.
struct Pair {
    source: NodeID,
    target: NodeID,
    rank: usize,
    distance: usize,
}

fn main() -> Result<(), Box<dyn Error>> {
    Builder::from_env(Env::default().default_filter_or("info")).init();
    let args = <Arguments as clap::Parser>::parse();
    info!("{args}");

    match &args.mode {
        Mode::Sample(sample) => run_sample(sample),
        Mode::Check(check) => run_check(check),
        Mode::Time(time) => run_time(time),
        Mode::Scans(scans) => run_scans(scans),
        Mode::Pack(pack) => run_pack(pack),
    }
}

/// Whether a path is there to be read, said plainly rather than left to the
/// unwrap inside the reader.
fn readable(path: &str, what: &str) -> Result<(), Box<dyn Error>> {
    if path.is_empty() {
        return Err(format!("no {what} was given").into());
    }
    if !std::path::Path::new(path).is_file() {
        return Err(format!("the {what} {path:?} is not a file that can be read").into());
    }
    Ok(())
}

fn load_graph(path: &str) -> StaticGraph<u32> {
    let edges = io::read_edges_from_file(path);
    info!("loaded {} graph edges", edges.len());
    let graph = StaticGraph::new(edges);
    info!(
        "graph of {} nodes and {} edges",
        graph.number_of_nodes(),
        graph.number_of_edges()
    );
    graph
}

fn run_sample(args: &Sample) -> Result<(), Box<dyn Error>> {
    readable(&args.graph, "graph")?;
    let graph = load_graph(&args.graph);
    let node_count = graph.number_of_nodes();
    if node_count == 0 {
        return Err("the graph has no nodes to draw from".into());
    }

    if let Some(threads) = args.threads {
        rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build_global()?;
    }

    // the drawing is done up front and on one thread, so that a sample is the
    // same sample however many threads happen to run it
    let mut rng = StdRng::seed_from_u64(args.seed);
    let sources = (0..args.sources)
        .map(|_| rng.random_range(0..node_count))
        .collect::<Vec<_>>();
    let targets = if args.pairs {
        (0..args.sources)
            .map(|_| rng.random_range(0..node_count))
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    let started = Instant::now();
    let bar = bar_of(sources.len(), "sources");
    let pairs: Vec<Pair> = if args.pairs {
        sources
            .par_iter()
            .zip(targets.par_iter())
            .filter_map(|(&source, &target)| {
                let pair = one_pair(&graph, source, target);
                bar.inc(1);
                pair
            })
            .collect()
    } else {
        sources
            .par_iter()
            .flat_map_iter(|&source| {
                let of_source = ranks_of(&graph, source);
                bar.inc(1);
                of_source
            })
            .collect()
    };
    bar.finish_and_clear();
    info!(
        "{} pairs from {} sources in {:.1} s",
        pairs.len(),
        args.sources,
        started.elapsed().as_secs_f64()
    );

    let mut out = BufWriter::new(File::create(&args.out)?);
    writeln!(out, "source,target,rank,distance")?;
    for pair in &pairs {
        writeln!(
            out,
            "{},{},{},{}",
            pair.source, pair.target, pair.rank, pair.distance
        )?;
    }
    out.flush()?;
    info!("wrote {} to {}", pairs.len(), args.out);
    Ok(())
}

/// One walk of the graph from a source, and the node settled at each power of
/// two along the way.
///
/// The search is given a target it will never reach, so it runs until the
/// queue is empty and the whole of what the source can reach is walked.
fn ranks_of(graph: &StaticGraph<u32>, source: NodeID) -> Vec<Pair> {
    let mut search = UnidirectionalSearch::<RankTargets>::new();
    search.run(graph, source, NodeID::MAX);

    search
        .stats()
        .targets()
        .iter()
        // rank one is the source itself, which is not a pair
        .filter(|&&(_, target)| target != source)
        .map(|&(rank, target)| Pair {
            source,
            target,
            rank,
            distance: search.distance(target),
        })
        .collect()
}

/// One search per pair, which is what asking about a pair drawn at random
/// costs.
fn one_pair(graph: &StaticGraph<u32>, source: NodeID, target: NodeID) -> Option<Pair> {
    let mut search = UnidirectionalSearch::<Counters>::new();
    let distance = search.run(graph, source, target);
    if distance == usize::MAX || source == target {
        return None;
    }
    Some(Pair {
        source,
        target,
        // the search stopped at the target, so what it settled is the rank
        rank: search.stats().deleted,
        distance,
    })
}

/// Asks both searches about every pair and says where they disagree.
///
/// The pairs of a source go to the query together. A query with a set of
/// targets has to walk the arcs of every cell holding one of them rather than
/// stepping over it, and asking for one target at a time never puts that to
/// the test.
fn run_check(args: &Check) -> Result<(), Box<dyn Error>> {
    readable(&args.graph, "graph")?;
    readable(&args.directory, "level directory")?;
    readable(&args.input, "input")?;

    let graph = load_graph(&args.graph);
    let pairs = read_pairs(&args.input)?;
    let directory: LevelDirectory = io::read_from_file(&args.directory);
    info!(
        "checking {} pairs over a directory of {} levels",
        pairs.len(),
        directory.levels()
    );

    let mut of_source: FxHashMap<NodeID, Vec<NodeID>> = FxHashMap::default();
    for &(source, target, _) in &pairs {
        of_source.entry(source).or_default().push(target);
    }

    let customization = Customization::new(graph, directory);
    let mut query = MldQuery::new();
    let mut plain = UnidirectionalDijkstra::new();
    let mut checked = 0_usize;
    let mut wrong = Vec::new();

    let bar = bar_of(of_source.len(), "sources");
    for (&source, targets) in &of_source {
        query.run(&customization, source, targets);
        for &target in targets {
            let by_query = query.distance(target);
            let by_plain = plain.run(customization.graph(), source, target);
            checked += 1;
            if by_query != by_plain {
                wrong.push((source, target, by_plain, by_query));
            }
        }
        bar.inc(1);
    }
    bar.finish_and_clear();

    for &(source, target, by_plain, by_query) in wrong.iter().take(args.report) {
        warn!("{source} to {target}: the graph says {by_plain}, the cells say {by_query}");
    }
    if wrong.is_empty() {
        info!("{checked} pairs, and the two agree on every one");
        return Ok(());
    }
    Err(format!("{} of {checked} pairs disagree", wrong.len()).into())
}

fn run_time(args: &Time) -> Result<(), Box<dyn Error>> {
    readable(&args.graph, "graph")?;
    readable(&args.input, "input")?;
    if matches!(
        args.engine,
        Engine::Mld | Engine::PagedMld | Engine::EdgeBasedMld | Engine::EdgeBasedPagedMld
    ) {
        readable(&args.directory, "level directory")?;
    }
    if matches!(
        args.engine,
        Engine::EdgeBasedMld | Engine::EdgeBasedPagedMld
    ) {
        if args.coordinates.is_empty() {
            return Err("the edge-based engine wants --coordinates, to price its turns by".into());
        }
        readable(&args.coordinates, "coordinates")?;
    }
    if matches!(args.engine, Engine::PagedMld | Engine::EdgeBasedPagedMld) {
        if args.tables.is_empty() {
            return Err("the paged-mld engine wants --tables, as the pack mode wrote them".into());
        }
        for suffix in ["blocks", "map", "tree"] {
            readable(&format!("{}.{suffix}", args.tables), "packed tables")?;
        }
    }
    let mut graph = load_graph(&args.graph);
    let mut pairs = read_pairs(&args.input)?;
    info!("read {} pairs from {}", pairs.len(), args.input);
    if pairs.is_empty() {
        return Err("there are no pairs to time".into());
    }

    // The pairs arrive in the order they were sampled, which is every rank of
    // one source and then every rank of the next. Timed in that order, each
    // query but the first of a source finds that source's cells already warm
    // and its part of the graph already in cache, and the ranks within a
    // source run from low to high, so the bias lines up with the very axis
    // being plotted. Shuffling spreads it out.
    pairs.shuffle(&mut StdRng::seed_from_u64(args.seed));

    // An instance written under a numbering of its own is read under it by
    // every engine, not only the ones that walk the cells. A plain search is
    // the yardstick a search over the cells is held against, and a yardstick
    // read off another copy of the graph is not one: the numbering lays the
    // nodes of a cell side by side, which a plain search feels too. Timing the
    // two over different copies would credit the cells with what the numbering
    // did.
    let mut ordering = None;
    if !args.ordering.is_empty() {
        let moved: NodeOrdering = io::read_from_file(&args.ordering);
        info!(
            "read a numbering of {} nodes, {} of them on the border of a cell",
            moved.len(),
            moved.on_a_border()
        );
        assert_eq!(
            moved.len(),
            graph.number_of_nodes(),
            "the numbering was worked out over another graph"
        );
        // the pairs arrived as the nodes the input had
        for pair in &mut pairs {
            pair.0 = moved.new_of(pair.0);
            pair.1 = moved.new_of(pair.1);
        }
        ordering = Some(moved);
    }

    let directory = match args.engine {
        Engine::Mld
        | Engine::BidirectionalMld
        | Engine::PagedMld
        | Engine::EdgeBasedMld
        | Engine::EdgeBasedPagedMld => {
            let directory: LevelDirectory = io::read_from_file(&args.directory);
            info!(
                "loaded a directory of {} levels over {} nodes",
                directory.levels(),
                directory.number_of_nodes()
            );
            if ordering.is_some() {
                // already written under a numbering, and the pairs are in it
                assert_eq!(
                    graph.number_of_nodes(),
                    directory.number_of_nodes(),
                    "the directory was built over another graph"
                );
                Some(directory)
            } else if !args.renumber {
                Some(directory)
            } else {
                let started = Instant::now();
                let moved = NodeOrdering::of(&graph, &PackedPartition::of(&directory));
                info!(
                    "numbered {} nodes in {:.1} s, {} of them ({:.1}%) on the border of a cell",
                    moved.len(),
                    started.elapsed().as_secs_f64(),
                    moved.on_a_border(),
                    100.0 * moved.on_a_border() as f64 / moved.len() as f64
                );
                graph = renumbered_of(&graph, &moved);
                let directory = moved.renumber_directory(&directory);
                // the pairs arrived as the numbers the input had
                for pair in &mut pairs {
                    pair.0 = moved.new_of(pair.0);
                    pair.1 = moved.new_of(pair.1);
                }
                ordering = Some(moved);
                Some(directory)
            }
        }
        _ => None,
    };

    let timings = match args.engine {
        Engine::Dijkstra => time_dijkstra(&graph, &pairs, args.warmup),
        Engine::Bidirectional => {
            // the backward side walks arcs into a node, which on a network
            // read in both directions is the same graph read the same way
            if is_symmetric(&graph) {
                info!("the graph is its own reverse, so the backward side shares it");
                time_bidirectional(&graph, &graph, &pairs, args.warmup)
            } else {
                warn!("the graph is directed, so a reversed copy is being built");
                let reverse = reverse_of(&graph);
                time_bidirectional(&graph, &reverse, &pairs, args.warmup)
            }
        }
        Engine::Mld => {
            let directory = directory.expect("a directory was read for the cells");
            time_mld(graph, directory, &pairs, args.warmup)
        }
        Engine::BidirectionalMld => {
            let directory = directory.expect("a directory was read for the cells");
            let reverse = reverse_of(&graph);
            info!("turned {} arcs around", reverse.number_of_edges());
            time_bidirectional_mld(graph, reverse, directory, &pairs, args.warmup)
        }
        Engine::PagedMld => {
            let directory = directory.expect("a directory was read for the cells");
            time_paged_mld(graph, &directory, args, &pairs)?
        }
        Engine::EdgeBasedMld => {
            let directory = directory.expect("a directory was read for the cells");
            time_edge_based_mld(graph, &directory, args, &pairs)
        }
        Engine::EdgeBasedPagedMld => {
            let directory = directory.expect("a directory was read for the cells");
            time_edge_based_paged_mld(graph, &directory, args, &pairs)?
        }
    };

    // written out as the numbers they arrived as, so that a run that was
    // renumbered can be laid against one that was not
    let timings = match &ordering {
        None => timings,
        Some(moved) => timings
            .into_iter()
            .map(|(source, target, rank, nanos, distance)| {
                (
                    moved.old_of(source),
                    moved.old_of(target),
                    rank,
                    nanos,
                    distance,
                )
            })
            .collect(),
    };

    let mut out = BufWriter::new(File::create(&args.out)?);
    writeln!(out, "engine,source,target,rank,nanos,distance")?;
    for (source, target, rank, nanos, distance) in &timings {
        writeln!(
            out,
            "{},{source},{target},{rank},{nanos},{distance}",
            args.engine
        )?;
    }
    out.flush()?;
    info!("wrote {} timings to {}", timings.len(), args.out);
    Ok(())
}

/// Clearing what the last query left behind belongs to that query, not to the
/// next one.
///
/// A search clears itself as the first thing it does, and what that costs goes
/// with the size of the run before it, not of the run being timed. Over pairs
/// drawn from every rank and then shuffled, that puts the cost of clearing a
/// search of sixteen million nodes onto whichever query happens to follow it:
/// a small query measured after a large one came out thirty times what the
/// same query costs after another small one. Clearing before the clock starts
/// puts it back where it belongs.
/// The pairs, timed one at a time on one thread. Timing under rayon would say
/// as much about the scheduler as about the search.
fn time_dijkstra(graph: &StaticGraph<u32>, pairs: &[ToTime], warmup: usize) -> Vec<Timing> {
    let mut search = UnidirectionalDijkstra::new();
    let bar = bar_of(warmup.min(pairs.len()), "warming");
    for &(source, target, _) in pairs.iter().take(warmup) {
        search.run(graph, source, target);
        bar.inc(1);
    }
    bar.finish_and_clear();

    let bar = bar_of(pairs.len(), "timing");
    let timings = pairs
        .iter()
        .map(|&(source, target, rank)| {
            search.clear();
            let started = Instant::now();
            let distance = search.run(graph, source, target);
            let elapsed = started.elapsed().as_nanos();
            // outside the reading above, so what is drawn is not what is
            // measured
            bar.inc(1);
            (source, target, rank, elapsed, distance)
        })
        .collect();
    bar.finish_and_clear();
    timings
}

/// Counts what each query settles, rather than what it costs.
///
/// A settled node is one the search took off its queue and was done with, and
/// how many of those a query needs is the figure that does not move when the
/// machine changes, the baseline changes, or the heap underneath changes. It
/// is what to hold against a published number.
fn run_scans(args: &Scans) -> Result<(), Box<dyn Error>> {
    let graph = load_graph(&args.graph);
    let directory: LevelDirectory = io::read_from_file(&args.directory);
    info!(
        "loaded a directory of {} levels over {} nodes",
        directory.levels(),
        directory.number_of_nodes()
    );
    let pairs = read_pairs(&args.input)?;
    info!("read {} pairs from {}", pairs.len(), args.input);

    let reverse = match args.engine {
        Engine::BidirectionalMld => Some(reverse_of(&graph)),
        _ => None,
    };
    let customization = Customization::new(graph, directory);
    let backward = reverse
        .as_ref()
        .map(|reverse| BorderLevels::of(reverse, customization.partition()));

    let bar = bar_of(pairs.len(), "counting");
    let mut out = BufWriter::new(File::create(&args.out)?);
    writeln!(out, "engine,source,target,rank,settled,inserted,decreased")?;

    let mut one = TrackedMldQuery::new();
    let mut both = TrackedBidirectionalMldQuery::new();
    for &(source, target, rank) in &pairs {
        let (settled, inserted, decreased) = match args.engine {
            Engine::BidirectionalMld => {
                let reverse = reverse.as_ref().expect("a reversed graph was built");
                let backward = backward.as_ref().expect("its arcs were levelled");
                both.run(&customization, reverse, backward, source, target);
                let (forward, backward) = both.stats();
                (
                    forward.deleted + backward.deleted,
                    forward.inserted + backward.inserted,
                    forward.decreased + backward.decreased,
                )
            }
            _ => {
                one.run(&customization, source, &[target]);
                let stats = one.stats();
                (stats.deleted, stats.inserted, stats.decreased)
            }
        };
        writeln!(
            out,
            "{},{source},{target},{rank},{settled},{inserted},{decreased}",
            args.engine
        )?;
        bar.inc(1);
    }
    bar.finish_and_clear();
    out.flush()?;
    info!("wrote {} counts to {}", pairs.len(), args.out);
    Ok(())
}

/// Whether every arc of the graph has its opposite, at the same cost.
///
/// A network read in both directions is its own reverse, and then the backward
/// side of a search from both ends can walk the forward graph and no second
/// copy of it is needed. That is worth one pass to establish rather than
/// assuming: assuming it of a graph that is directed somewhere would quietly
/// hand back distances that are wrong only for the pairs that go that way.
fn is_symmetric(graph: &StaticGraph<u32>) -> bool {
    graph.node_range().all(|u| {
        graph.edge_range(u).all(|edge| {
            let v = graph.target(edge);
            graph
                .find_edge(v, u)
                .is_some_and(|back| graph.data(back) == graph.data(edge))
        })
    })
}

/// The same arcs, between the numbers a renumbering gave their ends.
///
/// Walked off the graph rather than kept beside it: a continent is forty odd
/// million arcs, and holding a second copy of them to renumber costs more than
/// building the one that is wanted.
fn renumbered_of(graph: &StaticGraph<u32>, ordering: &NodeOrdering) -> StaticGraph<u32> {
    let mut edges = Vec::with_capacity(graph.number_of_edges());
    for u in graph.node_range() {
        let moved = ordering.new_of(u);
        for edge in graph.edge_range(u) {
            edges.push(InputEdge::new(
                moved,
                ordering.new_of(graph.target(edge)),
                *graph.data(edge),
            ));
        }
    }
    StaticGraph::new(edges)
}

/// The graph with every arc turned around.
fn reverse_of(graph: &StaticGraph<u32>) -> StaticGraph<u32> {
    let mut edges = Vec::with_capacity(graph.number_of_edges());
    for u in graph.node_range() {
        for edge in graph.edge_range(u) {
            edges.push(InputEdge::new(graph.target(edge), u, *graph.data(edge)));
        }
    }
    StaticGraph::new(edges)
}

/// The same pairs, run from both ends at once.
///
/// This is the yardstick that costs nothing to have: no preprocessing, no
/// overlay, just a second queue. What the overlay is worth is what it beats
/// this by, not what it beats the one-ended search by.
fn time_bidirectional(
    graph: &StaticGraph<u32>,
    reverse: &StaticGraph<u32>,
    pairs: &[ToTime],
    warmup: usize,
) -> Vec<Timing> {
    let mut search = BidirectionalDijkstra::new();
    let bar = bar_of(warmup.min(pairs.len()), "warming");
    for &(source, target, _) in pairs.iter().take(warmup) {
        search.run(graph, reverse, source, target);
        bar.inc(1);
    }
    bar.finish_and_clear();

    let bar = bar_of(pairs.len(), "timing");
    let timings = pairs
        .iter()
        .map(|&(source, target, rank)| {
            search.clear();
            let started = Instant::now();
            let distance = search.run(graph, reverse, source, target);
            let elapsed = started.elapsed().as_nanos();
            bar.inc(1);
            (source, target, rank, elapsed, distance)
        })
        .collect();
    bar.finish_and_clear();
    timings
}

/// The same pairs over the cells of the partition.
///
/// The overlay is worked out as it is asked for, so the warm-up matters more
/// here than it does for the plain search: without it the first pairs would be
/// paying for the customization of every cell they touch.
fn time_mld(
    graph: StaticGraph<u32>,
    directory: LevelDirectory,
    pairs: &[ToTime],
    warmup: usize,
) -> Vec<Timing> {
    let customization = Customization::new(graph, directory);
    let mut query = MldQuery::new();

    let started = Instant::now();
    let bar = bar_of(warmup.min(pairs.len()), "warming the overlay");
    for &(source, target, _) in pairs.iter().take(warmup) {
        query.run(&customization, source, &[target]);
        bar.inc(1);
    }
    bar.finish_and_clear();
    info!(
        "warmed {} cells in {:.1} s",
        customization.customized_cells(),
        started.elapsed().as_secs_f64()
    );

    let bar = bar_of(pairs.len(), "timing");
    let timings = pairs
        .iter()
        .map(|&(source, target, rank)| {
            query.clear();
            let started = Instant::now();
            let reached = query.run(&customization, source, &[target]);
            let elapsed = started.elapsed().as_nanos();
            let distance = if reached {
                query.distance(target)
            } else {
                usize::MAX
            };
            bar.inc(1);
            (source, target, rank, elapsed, distance)
        })
        .collect();
    bar.finish_and_clear();
    timings
}

/// Checks that each cell's nodes run consecutively, which is what a store of
/// blocks is written against.
///
/// A block says where a cell's nodes begin and how many it holds, and a table
/// names its border nodes by how far into the cell they sit. Both are a running
/// total of what the cells hold, so both are answers only where a cell's nodes
/// are consecutive. Packed against a numbering where they are not, every table
/// is written down against the wrong nodes and every query is quietly wrong
/// rather than refused: 3605 of 4800 pairs of a continent disagreed with the
/// same query over the same cells held in memory.
fn contiguous_cells(
    partition: &PackedPartition,
    directory: &LevelDirectory,
) -> Result<(), Box<dyn Error>> {
    for level in 0..directory.levels() {
        let mut seen = vec![false; directory.cells_on_level(level)];
        let mut last = usize::MAX;
        for node in 0..directory.number_of_nodes() {
            let cell = partition.cell_of(node, level) as usize;
            if cell == last {
                continue;
            }
            if seen[cell] {
                return Err(format!(
                    "the nodes of cell {cell} on level {level} do not run consecutively, so the \
                     instance is not laid out for a store of blocks. Run the renumber binary over \
                     it first, with --cells-in-key-order --numbering cell-path, and pack what \
                     that writes"
                )
                .into());
            }
            seen[cell] = true;
            last = cell;
        }
    }
    Ok(())
}

/// Customizes the cells once and writes them to a file, so that a search can
/// read them off it rather than hold them.
///
/// Three files come out and all three are wanted to read a table back: the
/// blocks, the map that says where each block landed, and the tree of cells the
/// blocks are ordered by.
fn run_pack(args: &Pack) -> Result<(), Box<dyn Error>> {
    readable(&args.graph, "graph")?;
    readable(&args.coordinates, "coordinates")?;
    readable(&args.directory, "level directory")?;

    let graph = load_graph(&args.graph);
    let coordinates = io::read_vec_from_file::<FPCoordinate>(&args.coordinates);
    let directory: LevelDirectory = io::read_from_file(&args.directory);
    assert_eq!(
        graph.number_of_nodes(),
        directory.number_of_nodes(),
        "the directory was built over another graph"
    );
    info!(
        "loaded a directory of {} levels over {} nodes",
        directory.levels(),
        directory.number_of_nodes()
    );

    let partition = PackedPartition::of(&directory);
    contiguous_cells(&partition, &directory)?;
    let tree = CellTree::of(&directory, &partition, &graph, &coordinates);

    if args.edge_based {
        return pack_edge_based(args, graph, coordinates, &directory, &tree);
    }
    let started = Instant::now();
    let customization = Customization::new(load_graph(&args.graph), directory);
    info!(
        "customized {} cells in {:.1} s",
        customization.customized_cells(),
        started.elapsed().as_secs_f64()
    );

    let blocks = format!("{}.blocks", args.out);
    let started = Instant::now();
    let map = pack_cells(
        &customization,
        &tree,
        std::path::Path::new(&blocks),
        args.cells_a_block,
    )?;
    info!(
        "wrote {} blocks to {blocks} in {:.1} s",
        map.len(),
        started.elapsed().as_secs_f64()
    );

    io::write_to_file(&format!("{}.map", args.out), &map);
    io::write_to_file(&format!("{}.tree", args.out), &tree);
    info!("wrote the map and the tree beside it");
    Ok(())
}

/// The arcs a route may leave each source on, and the ones it may arrive at
/// each target by.
///
/// Which arcs run into a node has to be gathered, since an adjacency array says
/// only which run out. Asking instead for the reverse of each arc leaving the
/// target would answer only on a graph holding every arc both ways, and would
/// quietly report no route where it does not.
fn arc_ends(graph: &StaticGraph<u32>, pairs: &[ToTime]) -> Vec<ArcEnds> {
    let mut into = vec![Vec::new(); Graph::number_of_nodes(graph)];
    for node in graph.node_range() {
        for arc in graph.edge_range(node) {
            into[graph.target(arc)].push(arc);
        }
    }
    pairs
        .iter()
        .map(|&(source, target, _)| {
            (
                graph.edge_range(source).map(|arc| (arc, 0)).collect(),
                // what it cost to arrive is the distance to the arc plus what
                // travelling the arc itself costs
                into[target]
                    .iter()
                    .map(|&arc| (arc, *graph.data(arc) as usize))
                    .collect(),
            )
        })
        .collect()
}

/// Tabulates the cells over the arcs of the graph and writes them to a file.
///
/// The tables name arcs rather than nodes, so a block counts its ids from the
/// cell's first arc rather than its first node, and the store is told so when
/// it is opened again. Nothing else about the store differs.
fn pack_edge_based(
    _args: &Pack,
    _graph: StaticGraph<u32>,
    _coordinates: Vec<FPCoordinate>,
    _directory: &LevelDirectory,
    _tree: &CellTree,
) -> Result<(), Box<dyn Error>> {
    Err(
        "an edge-based table is not square and a block of the store is: its ways in \
         outnumber its ways out by nearly three to one, and the format names one list \
         of border ids to serve as both. Packing one wants the rows and the columns \
         written apart"
            .into(),
    )
}

#[allow(dead_code)]
fn pack_edge_based_once_blocks_hold_rectangles(
    args: &Pack,
    graph: StaticGraph<u32>,
    coordinates: Vec<FPCoordinate>,
    directory: &LevelDirectory,
    tree: &CellTree,
) -> Result<(), Box<dyn Error>> {
    let started = Instant::now();
    let overlay = EdgeBasedOverlay::new(graph, coordinates, AnglePenalty::new(30., 100), directory);
    let bar = bar_of(overlay.levels(), "tabulating the cells");
    for level in 0..overlay.levels() {
        for cell in 0..overlay.cells_on_level(level) {
            let _ = overlay.distances_of(level, cell as CellId);
        }
        bar.inc(1);
    }
    bar.finish_and_clear();
    info!(
        "tabulated every cell in {:.1} s",
        started.elapsed().as_secs_f64()
    );

    let begins = overlay.arcs_begin();
    let holds = overlay.arcs_held();
    let widths = overlay.border_widths();
    let blocks = format!("{}.blocks", args.out);
    let started = Instant::now();
    let map = pack_overlay(
        &overlay,
        tree,
        std::path::Path::new(&blocks),
        args.cells_a_block,
        // a cell's arcs run consecutively, and the ones on its border are
        // scattered through them rather than leading
        |level, cell| {
            (
                begins[level][cell as usize],
                false,
                holds[level][cell as usize],
            )
        },
    )?;
    info!(
        "wrote {} blocks to {blocks} in {:.1} s",
        map.len(),
        started.elapsed().as_secs_f64()
    );

    io::write_to_file(&format!("{}.map", args.out), &map);
    io::write_to_file(&format!("{}.tree", args.out), tree);
    io::write_vec_to_file(&format!("{}.begins", args.out), &begins);
    io::write_vec_to_file(&format!("{}.widths", args.out), &widths);
    info!("wrote the map, the tree, the arc offsets and the widths beside it");
    Ok(())
}

/// The same pairs over cells whose nodes are the arcs of the graph, read off a
/// file rather than held.
fn time_edge_based_paged_mld(
    graph: StaticGraph<u32>,
    directory: &LevelDirectory,
    args: &Time,
    pairs: &[ToTime],
) -> Result<Vec<Timing>, Box<dyn Error>> {
    let coordinates = io::read_vec_from_file::<FPCoordinate>(&args.coordinates);
    let map: BlockMap = io::read_from_file(&format!("{}.map", args.tables));
    let tree: CellTree = io::read_from_file(&format!("{}.tree", args.tables));
    let begins: Vec<Vec<u32>> = io::read_vec_from_file(&format!("{}.begins", args.tables));
    let widths: Vec<Vec<u32>> = io::read_vec_from_file(&format!("{}.widths", args.tables));
    info!(
        "read a map of {} blocks over {} levels",
        map.len(),
        tree.levels()
    );

    let ends = arc_ends(&graph, pairs);
    let partition = PackedPartition::of(directory);
    let borders = BorderLevels::of(&graph, &partition);
    let store = BlockStore::open_counting_from(
        std::path::Path::new(&format!("{}.blocks", args.tables)),
        map,
        tree,
        begins,
        widths,
    )?;
    let tables = PagedOverlay::new(
        store,
        load_graph(&args.graph),
        partition,
        borders,
        Pool::of(args.budget),
    );
    let overlay =
        PagedEdgeBasedOverlay::new(tables, graph, coordinates, AnglePenalty::new(30., 100));
    info!("holding at most {} bytes of tables", args.budget);

    let mut query = MldQuery::new();
    let bar = bar_of(args.warmup.min(pairs.len()), "warming the pool");
    for (leaving, arriving) in ends.iter().take(args.warmup) {
        query.run_to_arcs(&overlay, leaving, arriving);
        bar.inc(1);
    }
    bar.finish_and_clear();

    let bar = bar_of(pairs.len(), "timing");
    let timings = pairs
        .iter()
        .zip(&ends)
        .map(|(&(source, target, rank), (leaving, arriving))| {
            query.clear();
            let started = Instant::now();
            let distance = query.run_to_arcs(&overlay, leaving, arriving);
            let elapsed = started.elapsed().as_nanos();
            bar.inc(1);
            (source, target, rank, elapsed, distance)
        })
        .collect();
    bar.finish_and_clear();

    let faults = overlay.faults();
    info!(
        "{} tables were found already held, {} were read off the file, {} were thrown away",
        faults.hits, faults.misses, faults.evicted
    );
    Ok(timings)
}

/// The same pairs over cells whose nodes are the arcs of the graph.
///
/// A query names its ends as nodes and this searches over arcs. A node of the
/// overlay is the tail of its arc, so it starts on every arc leaving the source
/// at no cost, and finishes on the arcs running *into* the target, whose
/// distance plus their own weight is what it cost to arrive.
///
/// Finishing on the arcs *leaving* the target instead would ask for a way to
/// carry on out of it, which a dead end refusing reversals does not have: 996
/// of 4800 pairs came back unreachable that way, all of them at low ranks.
///
/// What comes back is what it costs to reach the target with every turn on the
/// way priced, which is not the number the node-based engines report and is not
/// meant to be.
fn time_edge_based_mld(
    graph: StaticGraph<u32>,
    directory: &LevelDirectory,
    args: &Time,
    pairs: &[ToTime],
) -> Vec<Timing> {
    let coordinates = io::read_vec_from_file::<FPCoordinate>(&args.coordinates);
    let ends = arc_ends(&graph, pairs);

    let started = Instant::now();
    let overlay = EdgeBasedOverlay::new(graph, coordinates, AnglePenalty::new(30., 100), directory);
    info!(
        "built the overlay in {:.1} s",
        started.elapsed().as_secs_f64()
    );

    // Every cell, not only the ones a warmup happens to walk through. A cell is
    // tabulated the first time it is asked for, so a query reaching one nobody
    // has asked for yet pays to build it with the clock running: left to a
    // warmup of a hundred pairs, the low ranks came out with medians of a
    // thirtieth of a millisecond and whiskers at sixty, which is cell building
    // and not query time.
    let started = Instant::now();
    let bar = bar_of(overlay.levels(), "tabulating the cells");
    for level in 0..overlay.levels() {
        for cell in 0..overlay.cells_on_level(level) {
            let _ = overlay.distances_of(level, cell as CellId);
        }
        bar.inc(1);
    }
    bar.finish_and_clear();
    info!(
        "tabulated every cell in {:.1} s",
        started.elapsed().as_secs_f64()
    );

    let mut query = MldQuery::new();
    let bar = bar_of(args.warmup.min(pairs.len()), "warming");
    for (leaving, arriving) in ends.iter().take(args.warmup) {
        query.run_to_arcs(&overlay, leaving, arriving);
        bar.inc(1);
    }
    bar.finish_and_clear();

    let bar = bar_of(pairs.len(), "timing");
    let timings = pairs
        .iter()
        .zip(&ends)
        .map(|(&(source, target, rank), (leaving, arriving))| {
            query.clear();
            let started = Instant::now();
            let distance = query.run_to_arcs(&overlay, leaving, arriving);
            let elapsed = started.elapsed().as_nanos();
            bar.inc(1);
            (source, target, rank, elapsed, distance)
        })
        .collect();
    bar.finish_and_clear();
    timings
}

/// The same pairs over cells that are read off a file rather than held.
///
/// What is being timed here is not another algorithm. It is the same query
/// over the same tables, with a budget on how much of them may be in memory at
/// once, so what the rows say is what reading a table off a file costs a query
/// that would otherwise have found it already there.
fn time_paged_mld(
    graph: StaticGraph<u32>,
    directory: &LevelDirectory,
    args: &Time,
    pairs: &[ToTime],
) -> Result<Vec<Timing>, Box<dyn Error>> {
    let map: BlockMap = io::read_from_file(&format!("{}.map", args.tables));
    let tree: CellTree = io::read_from_file(&format!("{}.tree", args.tables));
    info!(
        "read a map of {} blocks over {} levels",
        map.len(),
        tree.levels()
    );

    let partition = PackedPartition::of(directory);
    let borders = BorderLevels::of(&graph, &partition);
    let store = BlockStore::open(
        std::path::Path::new(&format!("{}.blocks", args.tables)),
        map,
        tree,
    )?;
    let overlay = PagedOverlay::new(store, graph, partition, borders, Pool::of(args.budget));
    info!("holding at most {} bytes of tables", args.budget);

    let mut query = MldQuery::new();
    let bar = bar_of(args.warmup.min(pairs.len()), "warming the pool");
    for &(source, target, _) in pairs.iter().take(args.warmup) {
        query.run(&overlay, source, &[target]);
        bar.inc(1);
    }
    bar.finish_and_clear();

    let bar = bar_of(pairs.len(), "timing");
    let timings = pairs
        .iter()
        .map(|&(source, target, rank)| {
            query.clear();
            let started = Instant::now();
            let reached = query.run(&overlay, source, &[target]);
            let elapsed = started.elapsed().as_nanos();
            let distance = if reached {
                query.distance(target)
            } else {
                usize::MAX
            };
            bar.inc(1);
            (source, target, rank, elapsed, distance)
        })
        .collect();
    bar.finish_and_clear();

    let faults = overlay.faults();
    info!(
        "{} tables were found already held, {} were read off the file, {} were thrown away",
        faults.hits, faults.misses, faults.evicted
    );
    Ok(timings)
}

/// The same pairs over the cells, with a front growing from each end.
fn time_bidirectional_mld(
    graph: StaticGraph<u32>,
    reverse: StaticGraph<u32>,
    directory: LevelDirectory,
    pairs: &[ToTime],
    warmup: usize,
) -> Vec<Timing> {
    let customization = Customization::new(graph, directory);
    // the backward side walks the reversed graph, whose arcs are held in
    // another order than the ones the customization holds
    let backward = BorderLevels::of(&reverse, customization.partition());
    let mut query = BidirectionalMldQuery::new();

    let started = Instant::now();
    let bar = bar_of(warmup.min(pairs.len()), "warming the overlay");
    for &(source, target, _) in pairs.iter().take(warmup) {
        query.run(&customization, &reverse, &backward, source, target);
        bar.inc(1);
    }
    bar.finish_and_clear();
    info!(
        "warmed {} cells in {:.1} s",
        customization.customized_cells(),
        started.elapsed().as_secs_f64()
    );

    let bar = bar_of(pairs.len(), "timing");
    let timings = pairs
        .iter()
        .map(|&(source, target, rank)| {
            query.clear();
            let started = Instant::now();
            let distance = query.run(&customization, &reverse, &backward, source, target);
            let elapsed = started.elapsed().as_nanos();
            bar.inc(1);
            (source, target, rank, elapsed, distance)
        })
        .collect();
    bar.finish_and_clear();
    timings
}

fn read_pairs(path: &str) -> Result<Vec<ToTime>, Box<dyn Error>> {
    let mut pairs = Vec::new();
    let mut skipped = 0;
    for line in BufReader::new(File::open(path)?).lines().skip(1) {
        let line = line?;
        let mut fields = line.split(',');
        let source = fields.next().and_then(|f| f.parse().ok());
        let target = fields.next().and_then(|f| f.parse().ok());
        let rank = fields.next().and_then(|f| f.parse().ok());
        match (source, target, rank) {
            (Some(source), Some(target), Some(rank)) => pairs.push((source, target, rank)),
            _ => skipped += 1,
        }
    }
    if skipped > 0 {
        warn!("{skipped} lines of {path} were not a pair and were left out");
    }
    Ok(pairs)
}
