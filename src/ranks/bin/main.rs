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

use command_line::{Arguments, Engine, Mode, Sample, Time};
use env_logger::{Builder, Env};
use log::{info, warn};
use rand::{RngExt, SeedableRng, prelude::StdRng};
use rayon::prelude::*;

use toolbox_rs::{
    customization::Customization,
    edge::InputEdge,
    graph::{Graph, NodeID},
    heap_stats::{Counters, RankTargets},
    io,
    level_directory::LevelDirectory,
    mld_query::MldQuery,
    static_graph::StaticGraph,
    unidirectional_dijkstra::{UnidirectionalDijkstra, UnidirectionalSearch},
};

/// A pair to time, and the rank its target sits at.
type ToTime = (NodeID, NodeID, usize);

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
        Mode::Time(time) => run_time(time),
    }
}

fn load_graph(path: &str) -> StaticGraph<usize> {
    let edges = io::read_vec_from_file::<InputEdge<usize>>(path);
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
    let pairs: Vec<Pair> = if args.pairs {
        sources
            .par_iter()
            .zip(targets.par_iter())
            .filter_map(|(&source, &target)| one_pair(&graph, source, target))
            .collect()
    } else {
        sources
            .par_iter()
            .flat_map_iter(|&source| ranks_of(&graph, source))
            .collect()
    };
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
fn ranks_of(graph: &StaticGraph<usize>, source: NodeID) -> Vec<Pair> {
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
fn one_pair(graph: &StaticGraph<usize>, source: NodeID, target: NodeID) -> Option<Pair> {
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

fn run_time(args: &Time) -> Result<(), Box<dyn Error>> {
    let graph = load_graph(&args.graph);
    let pairs = read_pairs(&args.input)?;
    info!("read {} pairs from {}", pairs.len(), args.input);
    if pairs.is_empty() {
        return Err("there are no pairs to time".into());
    }

    let timings = match args.engine {
        Engine::Dijkstra => time_dijkstra(&graph, &pairs, args.warmup),
        Engine::Mld => {
            if args.directory.is_empty() {
                return Err("the mld engine wants a level directory, see --directory".into());
            }
            let directory: LevelDirectory = io::read_from_file(&args.directory);
            info!(
                "loaded a directory of {} levels over {} nodes",
                directory.levels(),
                directory.number_of_nodes()
            );
            time_mld(graph, directory, &pairs, args.warmup)
        }
    };

    let mut out = BufWriter::new(File::create(&args.out)?);
    writeln!(out, "engine,rank,nanos,distance")?;
    for (rank, nanos, distance) in &timings {
        writeln!(out, "{},{rank},{nanos},{distance}", args.engine)?;
    }
    out.flush()?;
    info!("wrote {} timings to {}", timings.len(), args.out);
    Ok(())
}

/// The pairs, timed one at a time on one thread. Timing under rayon would say
/// as much about the scheduler as about the search.
fn time_dijkstra(
    graph: &StaticGraph<usize>,
    pairs: &[ToTime],
    warmup: usize,
) -> Vec<(usize, u128, usize)> {
    let mut search = UnidirectionalDijkstra::new();
    for &(source, target, _) in pairs.iter().take(warmup) {
        search.run(graph, source, target);
    }

    pairs
        .iter()
        .map(|&(source, target, rank)| {
            let started = Instant::now();
            let distance = search.run(graph, source, target);
            (rank, started.elapsed().as_nanos(), distance)
        })
        .collect()
}

/// The same pairs over the cells of the partition.
///
/// The overlay is worked out as it is asked for, so the warm-up matters more
/// here than it does for the plain search: without it the first pairs would be
/// paying for the customization of every cell they touch.
fn time_mld(
    graph: StaticGraph<usize>,
    directory: LevelDirectory,
    pairs: &[ToTime],
    warmup: usize,
) -> Vec<(usize, u128, usize)> {
    let customization = Customization::new(graph, directory);
    let mut query = MldQuery::new();

    let started = Instant::now();
    for &(source, target, _) in pairs.iter().take(warmup) {
        query.run(&customization, source, &[target]);
    }
    info!(
        "warmed {} cells in {:.1} s",
        customization.customized_cells(),
        started.elapsed().as_secs_f64()
    );

    pairs
        .iter()
        .map(|&(source, target, rank)| {
            let started = Instant::now();
            let reached = query.run(&customization, source, &[target]);
            let elapsed = started.elapsed().as_nanos();
            let distance = if reached {
                query.distance(target)
            } else {
                usize::MAX
            };
            (rank, elapsed, distance)
        })
        .collect()
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
