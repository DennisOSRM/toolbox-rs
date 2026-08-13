#[cfg(not(target_os = "windows"))]
#[global_allocator]
static GLOBAL: jemallocator::Jemalloc = jemallocator::Jemalloc;

mod command_line;
mod serialize;

use env_logger::Env;
use itertools::Itertools;

use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, info};
use rayon::prelude::*;
use std::sync::{
    Arc,
    atomic::{AtomicI32, AtomicU32, Ordering},
};
use toolbox_rs::geometry::FPCoordinate;
use toolbox_rs::io;
use toolbox_rs::{
    assembly,
    inertial_flow::{self, Flow, flow_cmp},
    partition_id::PartitionID,
};
use {
    command_line::Arguments,
    serialize::{write_level_directory, write_results},
};

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
    let input_edges = if args.cut_csv.is_empty() {
        Vec::new()
    } else {
        edges.clone()
    };
    let node_count = coordinates.len();
    // The size a cell has to reach before the cutting stops. Assembling a level
    // out of cells a quarter of its size leaves the assembly room to come close
    // to the size that was asked for.
    let base_cell_size = args
        .level_sizes
        .iter()
        .min()
        .map_or(args.minimum_cell_size, |smallest| (smallest / 4).max(1));
    if !args.level_sizes.is_empty() {
        info!(
            "cutting down to cells of {base_cell_size} nodes, then assembling {:?}",
            args.level_sizes
        );
    }

    // The tree the cutting leaves behind, in the order the cells are created,
    // so a parent always comes before its children. The root holds everything.
    let mut tree_sizes = vec![node_count];
    let mut tree_children: Vec<Option<(usize, usize)>> = vec![None];
    let mut leaf_of_node = vec![0_usize; node_count];

    // Without level sizes the cutting stops where it always did, so a run that
    // asks for nothing new behaves as it did before.
    let stop_at = if args.level_sizes.is_empty() {
        args.minimum_cell_size
    } else {
        base_cell_size
    };

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
                        inertial_flow::sub_step(
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
                });
                (result.right_ids).iter().for_each(|i| {
                    let mut id = load(*i);
                    id.make_right_child();
                    store(*i, id);
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
                    let tail_is_left = load(edge.source).is_left_child();
                    if tail_is_left != load(edge.target).is_left_child() {
                        continue;
                    }
                    if tail_is_left {
                        left_edges.push(edge);
                    } else {
                        right_edges.push(edge);
                    }
                }
                debug!("generating next level ids");

                // A half is cut further while it is larger than the size a cell
                // has to reach, and settles where it is otherwise. The ids of a
                // half that settles are pushed to the bottom of the hierarchy,
                // so that every node ends up on the same level and the ids stay
                // what they were before the levels were assembled.
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

        // Grow the tree and lay out the next level. The cells are numbered as
        // they are created, so a parent always comes before its children.
        let mut next_job_queue = Vec::new();
        for (parent, cut, uncut_ids) in outcomes {
            let Some((left_edges, left_ids, right_edges, right_ids)) = cut else {
                // a cell that could not be cut stays a cell of the tree as it is
                for &node in &uncut_ids {
                    leaf_of_node[node] = parent;
                }
                continue;
            };

            let left = tree_sizes.len();
            tree_sizes.push(left_ids.len());
            tree_children.push(None);
            let right = tree_sizes.len();
            tree_sizes.push(right_ids.len());
            tree_children.push(None);
            tree_children[parent] = Some((left, right));

            for (index, ids, edges) in [
                (left, left_ids, left_edges),
                (right, right_ids, right_edges),
            ] {
                for &node in &ids {
                    leaf_of_node[node] = index;
                }
                if ids.len() > stop_at {
                    next_job_queue.push((edges, ids, index));
                }
            }
        }
        current_level += 1;
        pb.finish_with_message(format!("level {current_level} done"));
        current_job_queue = next_job_queue;
    }

    if !args.level_sizes.is_empty() {
        // The tree was numbered as it grew, so a parent sits before its
        // children. Reversing it puts every child before its parent, which is
        // the order the assembly walks.
        let last = tree_sizes.len() - 1;
        let flip = |index: usize| last - index;
        let nodes = tree_sizes
            .iter()
            .zip(&tree_children)
            .map(|(&size, children)| assembly::Node {
                size,
                children: children.map(|(left, right)| (flip(left), flip(right))),
            })
            .rev()
            .collect_vec();
        let tree =
            assembly::Tree::new(nodes, leaf_of_node.iter().map(|&leaf| flip(leaf)).collect());
        info!(
            "bisection left {} cells over {} nodes",
            tree.number_of_cells(),
            tree.number_of_nodes()
        );

        let directory = assembly::assemble(&tree, &args.level_sizes);
        for level in 0..directory.levels() {
            info!(
                "level {level} of {} nodes: {} cells",
                args.level_sizes[level],
                directory.cells_on_level(level)
            );
        }
        if !args.level_directory.is_empty() {
            write_level_directory(&args.level_directory, &directory);
        }
    }

    let partition_ids_vec = partition_ids
        .iter()
        .map(|id| PartitionID(id.load(Ordering::Relaxed)))
        .collect_vec();
    if args.level_sizes.is_empty() {
        for id in &partition_ids_vec {
            debug_assert_eq!(id.level(), args.recursion_depth);
        }
    }

    write_results(&args, &partition_ids_vec, &coordinates, &input_edges);
    info!("done.");
}
