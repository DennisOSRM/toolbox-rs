//! Temporary instrumentation for the phase 0 measurements of issue #545.
//! Counters are aggregated in memory and reported once, so that a full
//! partitioning run does not have to emit a log line per solver invocation.
//! This module is not part of the library's purpose and gets removed once the
//! measurements are recorded.
use crate::edge::InputEdge;
use crate::max_flow::ResidualEdgeData;
use std::io::Write;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// phase counts are bucketed exactly up to this many, everything above lands
/// in the last bucket
const BUCKETS: usize = 33;

static COMPLETED_RUNS: AtomicU64 = AtomicU64::new(0);
static ABORTED_RUNS: AtomicU64 = AtomicU64::new(0);
static COMPLETED_PHASES: [AtomicU64; BUCKETS] = [const { AtomicU64::new(0) }; BUCKETS];
static ABORTED_PHASES: [AtomicU64; BUCKETS] = [const { AtomicU64::new(0) }; BUCKETS];
static AUGMENTATIONS: AtomicU64 = AtomicU64::new(0);
static NODES: AtomicU64 = AtomicU64::new(0);
static ARCS: AtomicU64 = AtomicU64::new(0);
/// what full-BFS-per-phase costs: sum over runs of phases * (V + E)
static BFS_WORK: AtomicU64 = AtomicU64::new(0);
/// what a single sweep per run would cost: sum over runs of (V + E)
static ONE_SWEEP: AtomicU64 = AtomicU64::new(0);
static COMPLETED_PHASE_TOTAL: AtomicU64 = AtomicU64::new(0);
static ABORTED_PHASE_TOTAL: AtomicU64 = AtomicU64::new(0);
static MAX_PHASES: AtomicU64 = AtomicU64::new(0);

/// Records one invocation of a max-flow solver.
pub fn record(phases: u64, augmentations: u64, aborted: bool, nodes: usize, arcs: usize) {
    let bucket = (phases as usize).min(BUCKETS - 1);
    if aborted {
        ABORTED_RUNS.fetch_add(1, Ordering::Relaxed);
        ABORTED_PHASES[bucket].fetch_add(1, Ordering::Relaxed);
    } else {
        COMPLETED_RUNS.fetch_add(1, Ordering::Relaxed);
        COMPLETED_PHASES[bucket].fetch_add(1, Ordering::Relaxed);
    }
    if aborted {
        ABORTED_PHASE_TOTAL.fetch_add(phases, Ordering::Relaxed);
    } else {
        COMPLETED_PHASE_TOTAL.fetch_add(phases, Ordering::Relaxed);
    }
    AUGMENTATIONS.fetch_add(augmentations, Ordering::Relaxed);
    NODES.fetch_add(nodes as u64, Ordering::Relaxed);
    ARCS.fetch_add(arcs as u64, Ordering::Relaxed);
    let sweep = (nodes + arcs) as u64;
    ONE_SWEEP.fetch_add(sweep, Ordering::Relaxed);
    BFS_WORK.fetch_add(phases * sweep, Ordering::Relaxed);
    MAX_PHASES.fetch_max(phases, Ordering::Relaxed);
}

fn load(histogram: &[AtomicU64; BUCKETS]) -> Vec<u64> {
    histogram
        .iter()
        .map(|b| b.load(Ordering::Relaxed))
        .collect()
}

fn quantile(histogram: &[u64], fraction: f64) -> usize {
    let total: u64 = histogram.iter().sum();
    if total == 0 {
        return 0;
    }
    let target = (total as f64 * fraction) as u64;
    let mut seen = 0;
    for (phases, count) in histogram.iter().enumerate() {
        seen += count;
        if seen >= target {
            return phases;
        }
    }
    BUCKETS - 1
}

/// Prints the aggregated counters. Called once at the end of a run.
pub fn report() {
    let completed = COMPLETED_RUNS.load(Ordering::Relaxed);
    let aborted = ABORTED_RUNS.load(Ordering::Relaxed);
    let runs = completed + aborted;
    if runs == 0 {
        return;
    }
    let completed_histogram = load(&COMPLETED_PHASES);
    let aborted_histogram = load(&ABORTED_PHASES);
    let phases =
        COMPLETED_PHASE_TOTAL.load(Ordering::Relaxed) + ABORTED_PHASE_TOTAL.load(Ordering::Relaxed);
    // a run that gives up within its first two phases had no chance to amortize
    let gave_up_early: u64 = aborted_histogram[0..3].iter().sum();

    println!("--- solver statistics, issue #545 phase 0 ---");
    println!("solver runs                {runs}");
    println!(
        "  completed                {completed}  ({:.1}%)",
        100. * completed as f64 / runs as f64
    );
    println!(
        "  aborted on upper bound   {aborted}  ({:.1}%)",
        100. * aborted as f64 / runs as f64
    );
    println!(
        "  aborted within 2 phases  {gave_up_early}  ({:.1}% of all runs)",
        100. * gave_up_early as f64 / runs as f64
    );
    println!(
        "phases (successful BFS runs) {phases}, {:.2} per run",
        phases as f64 / runs as f64
    );
    println!(
        "  median phases, completed runs {}",
        quantile(&completed_histogram, 0.5)
    );
    println!(
        "  90th percentile, completed    {}",
        quantile(&completed_histogram, 0.9)
    );
    println!(
        "  median phases, aborted runs   {}",
        quantile(&aborted_histogram, 0.5)
    );
    println!(
        "augmentations              {}, {:.2} per phase",
        AUGMENTATIONS.load(Ordering::Relaxed),
        AUGMENTATIONS.load(Ordering::Relaxed) as f64 / phases.max(1) as f64
    );
    println!(
        "mean solver size           V {:.0}, E {:.0}",
        NODES.load(Ordering::Relaxed) as f64 / runs as f64,
        ARCS.load(Ordering::Relaxed) as f64 / runs as f64
    );
    let bfs_work = BFS_WORK.load(Ordering::Relaxed);
    let one_sweep = ONE_SWEEP.load(Ordering::Relaxed);
    println!(
        "phases by outcome         completed {}, aborted {}",
        COMPLETED_PHASE_TOTAL.load(Ordering::Relaxed),
        ABORTED_PHASE_TOTAL.load(Ordering::Relaxed)
    );
    println!(
        "longest run                {} phases",
        MAX_PHASES.load(Ordering::Relaxed)
    );
    println!("BFS work, sum of phases*(V+E)  {bfs_work}");
    println!("one sweep per run, sum of (V+E) {one_sweep}");
    println!(
        "work weighted sweeps per run   {:.1}   (this is the amortisation factor)",
        bfs_work as f64 / one_sweep.max(1) as f64
    );
    println!("phase histogram, completed runs (phases: count)");
    for (phases, count) in completed_histogram.iter().enumerate() {
        if *count > 0 {
            let label = if phases == BUCKETS - 1 {
                format!("{}+", BUCKETS - 1)
            } else {
                phases.to_string()
            };
            println!("  {label:>3}: {count}");
        }
    }
}

/// One slot per power of two of (V + E), so that a dump captures cells across
/// the whole size range rather than a million copies of the smallest one.
const SIZE_BUCKETS: usize = 28;
static DUMPED: [AtomicBool; SIZE_BUCKETS] = [const { AtomicBool::new(false) }; SIZE_BUCKETS];

/// Writes the first solver input seen in each size bucket to `TOOLBOX_DUMP_CELLS`,
/// building a corpus of real inputs that can be replayed without running the
/// whole partitioner. Temporary, for the phase 2 benchmark of issue #545.
///
/// Format is little endian: edge count as u64, source and target as u32, then
/// one u32 pair per arc. Capacities are not stored because the partitioner only
/// ever uses unit capacities, which is checked here.
pub fn maybe_dump(edges: &[InputEdge<ResidualEdgeData>], source: usize, target: usize) {
    let Ok(directory) = std::env::var("TOOLBOX_DUMP_CELLS") else {
        return;
    };
    let bucket = (usize::BITS - edges.len().leading_zeros()) as usize;
    if bucket >= SIZE_BUCKETS || DUMPED[bucket].swap(true, Ordering::Relaxed) {
        return;
    }

    let mut buffer = Vec::with_capacity(16 + 8 * edges.len());
    buffer.extend_from_slice(&(edges.len() as u64).to_le_bytes());
    buffer.extend_from_slice(&(source as u32).to_le_bytes());
    buffer.extend_from_slice(&(target as u32).to_le_bytes());
    for edge in edges {
        debug_assert_eq!(edge.data.capacity, 1, "the corpus assumes unit capacities");
        buffer.extend_from_slice(&(edge.source as u32).to_le_bytes());
        buffer.extend_from_slice(&(edge.target as u32).to_le_bytes());
    }
    let path = format!("{directory}/cell_{:02}_{}.bin", bucket, edges.len());
    if let Ok(mut file) = std::fs::File::create(&path) {
        let _ = file.write_all(&buffer);
    }
}

/// Reads a corpus file written by [`maybe_dump`].
pub fn read_cell(path: &std::path::Path) -> (Vec<InputEdge<ResidualEdgeData>>, usize, usize) {
    let bytes = std::fs::read(path).expect("could not read cell file");
    let count = u64::from_le_bytes(bytes[0..8].try_into().unwrap()) as usize;
    let source = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
    let target = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let mut edges = Vec::with_capacity(count);
    for i in 0..count {
        let at = 16 + i * 8;
        edges.push(InputEdge::new(
            u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as usize,
            u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize,
            ResidualEdgeData::new(1),
        ));
    }
    (edges, source, target)
}
