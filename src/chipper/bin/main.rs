#[cfg(not(target_os = "windows"))]
#[global_allocator]
static GLOBAL: jemallocator::Jemalloc = jemallocator::Jemalloc;

mod command_line;
mod serialize;

use env_logger::Env;
use itertools::Itertools;

use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, info, warn};
use rayon::prelude::*;
use rustc_hash::FxHashSet;
use std::sync::{
    Arc,
    atomic::{AtomicI32, AtomicU8, AtomicU32, Ordering},
};
use toolbox_rs::geometry::FPCoordinate;
use toolbox_rs::io;
use toolbox_rs::{
    assembly,
    boykov_kolmogorov::BoykovKolmogorov,
    dinic::Dinic,
    inertial_flow::{self, Flow, flow_cmp},
    level_directory::CellId,
    partition_id::PartitionID,
    push_relabel::PushRelabel,
};
use {
    command_line::{Arguments, Solver},
    serialize::{write_level_directory, write_results},
};

/// Numbers whatever the bisection left behind from zero without a gap.
fn compact(of_node: &[usize]) -> Vec<CellId> {
    let mut cell_of = rustc_hash::FxHashMap::default();
    of_node
        .iter()
        .map(|leaf| {
            let next = cell_of.len() as CellId;
            *cell_of.entry(*leaf).or_insert(next)
        })
        .collect()
}

fn main() {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    println!(r#"             chipping road networks into pieces.             "#);
    println!(r#"       ___    _         _      _ __    _ __                  "#);
    println!(r#"      / __|  | |_      (_)    | '_ \  | '_ \   ___      _ _  "#);
    println!(r#"     | (__   | ' \     | |    | .__/  | .__/  / -_)    | '_| "#);
    println!(r#"      \___|  |_||_|   _|_|_   |_|__   |_|__   \___|   _|_|_  "#);
    println!(r#"    _|"""""|_|"""""|_|"""""|_|"""""|_|"""""|_|"""""|_|"""""| "#);
    println!(r#"    "`-0-0-'"`-0-0-'"`-0-0-'"`-0-0-'"`-0-0-'"`-0-0-'"`-0-0-' "#);
    println!("build: {}", env!("GIT_HASH"));

    // parse and print command line parameters
    let args = <Arguments as clap::Parser>::parse();
    // Which min-cut solver the flow runs on. The two find cuts of the same
    // cost, but not necessarily the same cut, so this is here to compare the
    // partitions they lead to rather than to be switched in passing.

    info!("{args}");

    // set the number of threads if supplied on the command line
    if let Some(number_of_threads) = args.number_of_threads {
        info!("setting number of threads to {number_of_threads}");
        rayon::ThreadPoolBuilder::new()
            .num_threads(number_of_threads)
            .build_global()
            .unwrap();
    }

    let edges = io::read_graph_into_trivial_edges(&args.graph);
    let coordinates = io::read_vec_from_file::<FPCoordinate>(&args.coordinates);
    info!(
        "loaded {} edges and {} coordinates",
        edges.len(),
        coordinates.len()
    );

    // enqueue initial job for partitioning of the root node into job queue. The
    // root job takes ownership of the edge set, which is only needed again if
    // the cut is to be written out.
    let id_vector = (0..coordinates.len()).collect_vec();
    // The assembly walks the arcs to find which cells hold together and which
    // of them are neighbours, so they are kept for it as well as for the cut
    // csv. Nothing else needs them and a copy of the arcs of a continent is
    // not worth keeping for nobody.
    let input_edges = if args.cut_csv.is_empty() && args.level_sizes.is_empty() {
        Vec::new()
    } else {
        edges.clone()
    };
    let node_count = coordinates.len();
    // The size a cell has to reach before the cutting stops. Assembling a level
    // out of cells a quarter of its size leaves the assembly room to come close
    // to the size that was asked for.
    let stop_at = args
        .level_sizes
        .iter()
        .min()
        .map_or(args.minimum_cell_size, |smallest| (smallest / 4).max(1));
    if !args.level_sizes.is_empty() {
        info!(
            "cutting down to cells of {stop_at} nodes, then assembling {:?}",
            args.level_sizes
        );
    }

    // Which cell of the bisection each node has ended up in so far. A cell is
    // numbered when it is created, so the number of a node is the number of the
    // deepest cell it has landed in, and after the cutting that is its leaf.
    let mut leaf_of_node = vec![0_usize; coordinates.len()];
    let mut cells_created = 1;

    let job = (edges, id_vector, 0_usize);
    let mut current_job_queue = vec![job];

    let sty = ProgressStyle::default_spinner()
        .template("{spinner:.green} [{elapsed_precise}] {wide_bar:.green/yellow} {msg}")
        .unwrap()
        .progress_chars("#>-");

    let mut current_level = 0;
    // Cells are disjoint, hence each entry is written by at most one thread per
    // level. Relaxed atomics express that without an aliasing hazard and
    // compile to plain loads and stores.
    let partition_ids = (0..coordinates.len())
        .map(|_| AtomicU32::new(PartitionID::root().0))
        .collect_vec();
    let load = |index: usize| PartitionID(partition_ids[index].load(Ordering::Relaxed));
    let store = |index: usize, id: PartitionID| partition_ids[index].store(id.0, Ordering::Relaxed);

    // The side of the latest cut that a node ended up on, which is all that
    // splitting the edge set needs. Reading it off the partition id instead
    // only works while the id has room: it shifts left by one per level and
    // packs the whole path into a u32, so it cannot record a bisection deeper
    // than 31 and the lowest bit stops meaning the latest cut.
    const LEFT: u8 = 0;
    const RIGHT: u8 = 1;
    let sides = (0..coordinates.len())
        .map(|_| AtomicU8::new(LEFT))
        .collect_vec();
    let side_of = |index: usize| sides[index].load(Ordering::Relaxed);
    let set_side = |index: usize, side: u8| sides[index].store(side, Ordering::Relaxed);

    while !current_job_queue.is_empty() && current_level < args.recursion_depth {
        let pb = ProgressBar::new(current_job_queue.len() as u64);
        pb.set_style(sty.clone());

        let outcomes: Vec<_> = current_job_queue
            .par_iter_mut()
            .enumerate()
            .map(|(id, job)| {
                pb.set_message(format!("cell #{id}"));
                pb.inc(1);

                // we use the count of coordinates as an upper bound to the cut size
                let upper_bound = Arc::new(AtomicI32::new(job.1.len().try_into().unwrap()));
                // run inertial flow on all four axes
                let best_max_flow = (0..4)
                    .into_par_iter()
                    .map(|axis| -> Result<Flow, inertial_flow::FlowError> {
                        let cut = |solver| match solver {
                            Solver::PushRelabel => inertial_flow::sub_step::<PushRelabel>,
                            Solver::Dinic => inertial_flow::sub_step::<Dinic>,
                            Solver::BoykovKolmogorov => inertial_flow::sub_step::<BoykovKolmogorov>,
                        };
                        cut(args.solver)(
                            &job.0,
                            &job.1,
                            &coordinates,
                            axis,
                            args.b_factor,
                            upper_bound.clone(),
                        )
                    })
                    .filter_map(Result::ok)
                    .min_by(flow_cmp);

                let Some(result) = best_max_flow else {
                    // No axis yielded a cut, e.g. because the cell has no edges
                    // at all. The cell stays as it is, but its nodes still have
                    // to descend to the bottom of the hierarchy.
                    debug!("cell of {} nodes could not be cut", job.1.len());
                    let level_difference = (args.recursion_depth - current_level) as usize;
                    for i in &job.1 {
                        let mut id = load(*i);
                        id.make_leftmost_descendant(level_difference);
                        store(*i, id);
                    }
                    return (job.2, None, std::mem::take(&mut job.1));
                };
                debug!(
                    "best max-flow: {}, balance: {:.3}",
                    result.flow, result.balance
                );

                debug!("partitioning and assigning ids for all nodes");

                (result.left_ids).iter().for_each(|i| {
                    let mut id = load(*i);
                    id.make_left_child();
                    store(*i, id);
                    set_side(*i, LEFT);
                });
                (result.right_ids).iter().for_each(|i| {
                    let mut id = load(*i);
                    id.make_right_child();
                    store(*i, id);
                    set_side(*i, RIGHT);
                });

                // Partition edge and node id sets for the next iteration. The
                // edge set of the parent is consumed here, so that it is freed
                // right away instead of at the end of the level. Edges of the
                // cut are dropped: their head is outside of the cell they would
                // end up in, where they only inflate the flow graph by a node
                // that no flow can pass through.
                debug!("generating next level edges");
                let mut left_edges = Vec::new();
                let mut right_edges = Vec::new();
                for edge in std::mem::take(&mut job.0) {
                    let tail_side = side_of(edge.source);
                    if tail_side != side_of(edge.target) {
                        continue;
                    }
                    if tail_side == LEFT {
                        left_edges.push(edge);
                    } else {
                        right_edges.push(edge);
                    }
                }
                debug!("generating next level ids");

                let level_difference = (args.recursion_depth - current_level - 1) as usize;
                if result.left_ids.len() <= stop_at {
                    for i in &result.left_ids {
                        let mut id = load(*i);
                        id.make_leftmost_descendant(level_difference);
                        store(*i, id);
                    }
                }
                if result.right_ids.len() <= stop_at {
                    for i in &result.right_ids {
                        let mut id = load(*i);
                        id.make_rightmost_descendant(level_difference);
                        store(*i, id);
                    }
                }
                (
                    job.2,
                    Some((left_edges, result.left_ids, right_edges, result.right_ids)),
                    Vec::new(),
                )
            })
            .collect();

        // The halves are numbered here rather than in the threads above, so
        // that a cell gets the same number whichever order the jobs finish in.
        let mut next_job_queue = Vec::new();
        for (parent, cut, uncut_ids) in outcomes {
            let Some((left_edges, left_ids, right_edges, right_ids)) = cut else {
                // a cell that could not be cut stays as it is and its nodes
                // stay in it
                for &node in &uncut_ids {
                    leaf_of_node[node] = parent;
                }
                continue;
            };

            for (ids, edges) in [(left_ids, left_edges), (right_ids, right_edges)] {
                let cell = cells_created;
                cells_created += 1;
                for &node in &ids {
                    leaf_of_node[node] = cell;
                }
                if ids.len() > stop_at {
                    next_job_queue.push((edges, ids, cell));
                }
            }
        }
        current_level += 1;
        pb.finish_with_message(format!("level {current_level} done"));
        current_job_queue = next_job_queue;
    }

    // Cells are numbered as they are created, and the ones that were cut again
    // are not cells of the result, so the leaves have to be counted rather than
    // read off the tally.
    let leaves = leaf_of_node.iter().copied().collect::<FxHashSet<_>>().len();
    info!("the cutting left {leaves} cells over {cells_created} it made on the way");

    if !args.level_sizes.is_empty() {
        // The cells of the bisection need not hold together, as a minimum cut
        // puts everything the source cannot reach on the far side whether it
        // hangs together with the rest or not. Merging such a cell into a
        // larger one carries the split upwards, so they are taken apart first.
        let base_cells = compact(&leaf_of_node);
        let pieces = assembly::fragments(node_count, &input_edges, &base_cells);
        let piece_count = pieces.iter().copied().max().map_or(0, |p| p as usize + 1);
        info!("those cells hold together in {piece_count} pieces");

        let cells = assembly::cell_graph(&input_edges, &pieces);
        let directory = assembly::assemble_connected(&cells, &pieces, &args.level_sizes);
        for level in 0..directory.levels() {
            info!(
                "level {level} of {} nodes: {} cells",
                args.level_sizes[level],
                directory.cells_on_level(level)
            );
        }
        if args.level_directory.is_empty() {
            warn!("no level directory was asked for, so the levels are dropped");
        } else {
            write_level_directory(&args.level_directory, &directory);
        }
    }

    let partition_ids_vec = partition_ids
        .iter()
        .map(|id| PartitionID(id.load(Ordering::Relaxed)))
        .collect_vec();
    for id in &partition_ids_vec {
        debug_assert_eq!(id.level(), args.recursion_depth);
    }

    write_results(&args, &partition_ids_vec, &coordinates, &input_edges);
    info!("done.");
}
