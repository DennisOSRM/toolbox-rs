//! What a search over the cells costs against a search through them.
//!
//! Every pair of every rank goes into one measurement, so what comes out is a
//! mixture over the whole rank axis and a guard against the cost moving, not a
//! reading of the speedup. The speedup depends on how far apart the ends of a
//! query are, and where that is read is the rank plot the `ranks` tool feeds.
//!
//! A grid is also not a road network. Its cells are uniform and its finest
//! ones hold four nodes, of which all four sit on the border, so there is
//! little to step over down there. It says how the cost scales with the size
//! of the graph, and that the query does not get slower.

use criterion::{Criterion, criterion_group};
use rand::{SeedableRng, prelude::StdRng, seq::SliceRandom};
use std::hint::black_box;

use toolbox_rs::{
    customization::Customization,
    graph::{Graph, NodeID},
    grid_graph::grid,
    heap_stats::RankTargets,
    mld_query::MldQuery,
    unidirectional_dijkstra::{UnidirectionalDijkstra, UnidirectionalSearch},
};

/// The sides to measure on. A grid of 256 is 65,536 nodes, which is enough for
/// the rank axis to have somewhere to go and small enough for the overlay to
/// be worked out in the setup of a benchmark.
const SIDES: [usize; 3] = [64, 128, 256];

/// Pairs drawn the way the `ranks` tool draws them: one walk of the graph per
/// source, and the node settled at each power of two.
///
/// Working this out here rather than in the loop is what keeps the counting
/// out of the measurement. Nothing timed below collects anything.
fn pairs_of(
    graph: &toolbox_rs::static_graph::StaticGraph<u32>,
    sources: usize,
) -> Vec<(NodeID, NodeID)> {
    let mut rng = StdRng::seed_from_u64(0x_5EED);
    let mut search = UnidirectionalSearch::<RankTargets>::new();
    let mut pairs = Vec::new();

    for _ in 0..sources {
        let source = rand::RngExt::random_range(&mut rng, 0..graph.number_of_nodes() as NodeID);
        search.run(graph, source, NodeID::MAX);
        pairs.extend(
            search
                .stats()
                .targets()
                .iter()
                .filter(|&&(_, target)| target != source)
                .map(|&(_, target)| (source, target)),
        );
    }

    // The pairs come out as every rank of one source and then the next, so a
    // run in that order finds each source's cells warm from the query before
    // it. Shuffling spreads that out.
    pairs.shuffle(&mut rng);
    pairs
}

pub fn query_benchmark(c: &mut Criterion) {
    for side in SIDES {
        let (graph, directory) = grid(side, true);
        let pairs = pairs_of(&graph, 8);
        let customization = Customization::new(graph, directory);

        // every cell any of these pairs opens, worked out before the clock
        // starts, or the first iteration pays for the whole overlay
        let mut warm = MldQuery::new();
        for &(source, target) in &pairs {
            warm.run(&customization, source, &[target]);
        }

        let mut dijkstra = UnidirectionalDijkstra::new();
        c.bench_function(&format!("mld/dijkstra/{side}"), |b| {
            b.iter(|| {
                for &(source, target) in black_box(&pairs) {
                    black_box(dijkstra.run(customization.graph(), source, target));
                }
            });
        });

        let mut query = MldQuery::new();
        c.bench_function(&format!("mld/query/{side}"), |b| {
            b.iter(|| {
                for &(source, target) in black_box(&pairs) {
                    black_box(query.run(black_box(&customization), source, &[target]));
                }
            });
        });
    }
}

/// What the overlay costs to work out, which is what is being traded for the
/// query time above.
///
/// A fresh customization per iteration, as a warm one has nothing left to do.
pub fn customization_benchmark(c: &mut Criterion) {
    for side in [64_usize, 128] {
        let levels = grid(side, true).1.levels();

        c.bench_function(&format!("mld/customize/{side}"), |b| {
            b.iter_batched(
                || {
                    // built afresh each time, as a graph cannot be cloned and a
                    // customization that has already been asked has nothing
                    // left to work out
                    let (graph, directory) = grid(side, true);
                    Customization::new(graph, directory)
                },
                |customization| {
                    for level in 0..levels {
                        let cells = customization.level(level).cells();
                        for cell in 0..cells {
                            black_box(customization.distances_of(level, cell as u32));
                        }
                    }
                },
                criterion::BatchSize::LargeInput,
            );
        });
    }
}

criterion_group!(mld_query, query_benchmark, customization_benchmark);
