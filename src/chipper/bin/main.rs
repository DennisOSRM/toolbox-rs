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
    inertial_flow::{self, Flow, flow_cmp},
    partition_id::PartitionID,
};
use {command_line::Arguments, serialize::write_results};

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
    let job = (edges, id_vector);
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

        let next_job_queue = current_job_queue
            .par_iter_mut()
            .enumerate()
            .flat_map(|(id, job)| {
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
                    return Vec::new();
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

                // iterate on left half if larger than the minimum cell size
                let mut next_jobs = Vec::new();
                let level_difference = (args.recursion_depth - current_level - 1) as usize;
                if result.left_ids.len() > args.minimum_cell_size {
                    next_jobs.push((left_edges, result.left_ids));
                } else {
                    for i in &result.left_ids {
                        let mut id = load(*i);
                        id.make_leftmost_descendant(level_difference);
                        store(*i, id);
                    }
                }
                // iterate on right half if larger than the minimum cell size
                if result.right_ids.len() > args.minimum_cell_size {
                    next_jobs.push((right_edges, result.right_ids));
                } else {
                    for i in &result.right_ids {
                        let mut id = load(*i);
                        id.make_rightmost_descendant(level_difference);
                        store(*i, id);
                    }
                }
                next_jobs
            })
            .collect();
        current_level += 1;
        pb.finish_with_message(format!("level {current_level} done"));
        current_job_queue = next_job_queue;
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
