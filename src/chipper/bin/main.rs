mod command_line;
mod serialize;

use env_logger::Env;
use itertools::Itertools;

use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, info};
use rayon::prelude::*;
use std::sync::{atomic::AtomicI32, Arc};
use toolbox_rs::geometry::primitives::FPCoordinate;
use toolbox_rs::io;
use toolbox_rs::unsafe_slice::UnsafeSlice;
use toolbox_rs::{
    inertial_flow::{self, flow_cmp, FlowResult},
    partition::PartitionID,
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

    // enqueue initial job for partitioning of the root node into job queue
    let id_vector = (0..coordinates.len()).collect_vec();
    let job = (edges.clone(), id_vector);
    let mut current_job_queue = vec![job];

    let sty = ProgressStyle::default_spinner()
        .template("{spinner:.green} [{elapsed_precise}] {wide_bar:.green/yellow} {msg}")
        .unwrap()
        .progress_chars("#>-");

    let mut current_level = 0;
    let mut partition_ids_vec = vec![PartitionID::root(); coordinates.len()];
    let partition_ids = UnsafeSlice::new(&mut partition_ids_vec);

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
                    .map(|axis| -> FlowResult {
                        inertial_flow::sub_step(
                            &job.0,
                            &job.1,
                            &coordinates,
                            axis,
                            args.b_factor,
                            upper_bound.clone(),
                        )
                    })
                    .min_by(flow_cmp);

                let result = best_max_flow.unwrap();
                debug!(
                    "best max-flow: {}, balance: {:.3}",
                    result.flow, result.balance
                );

                debug!("partitioning and assigning ids for all nodes");

                (result.left_ids).iter().for_each(|id| unsafe {
                    partition_ids.get(*id).inplace_left_child();
                });
                (result.right_ids).iter().for_each(|id| unsafe {
                    partition_ids.get(*id).inplace_right_child();
                });

                // partition edge and node id sets for the next iteration
                debug!("generating next level edges");
                // TODO: don't copy, but partition in place
                let (left_edges, right_edges): (Vec<_>, Vec<_>) = job
                    .0
                    .iter()
                    .filter(|edge| unsafe {
                        partition_ids.get(edge.source) == partition_ids.get(edge.target)
                    })
                    .partition(|edge| unsafe { partition_ids.get(edge.source).is_left_child() });
                debug!("generating next level ids");

                // iterate on both halves
                let mut next_jobs = Vec::new();
                if result.left_ids.len() > args.minimum_cell_size {
                    next_jobs.push((left_edges, result.left_ids));
                } else {
                    let level_difference = (args.recursion_depth - current_level - 1) as usize;
                    for i in &result.left_ids {
                        unsafe {
                            partition_ids
                                .get(*i)
                                .inplace_leftmost_descendant(level_difference);
                        }
                    }
                }
                if result.right_ids.len() > args.minimum_cell_size {
                    next_jobs.push((right_edges, result.right_ids));
                } else {
                    let level_difference = (args.recursion_depth - current_level - 1) as usize;
                    for i in &result.right_ids {
                        unsafe {
                            partition_ids
                                .get(*i)
                                .inplace_leftmost_descendant(level_difference);
                        }
                    }
                }
                next_jobs
            })
            .collect();
        current_level += 1;
        pb.finish_with_message(format!("level {current_level} done"));
        current_job_queue = next_job_queue;
    }

    write_results(&args, &partition_ids_vec, &coordinates, &edges);

    for id in &partition_ids_vec {
        debug_assert_eq!(id.level(), args.recursion_depth);
    }
    info!("done.");
}
