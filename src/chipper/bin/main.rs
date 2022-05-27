mod command_line;
mod serialize;

use bool_ext::BoolExt;
use env_logger::Env;
use itertools::Itertools;

use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, info};
use rayon::prelude::*;
use std::sync::{atomic::AtomicI32, Arc};
use toolbox_rs::{
    dimacs,
    edge::Edge,
    inertial_flow::{self, flow_cmp, FlowResult},
    max_flow::ResidualCapacity,
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

    // parse and print command line parameters
    let args = <Arguments as clap::Parser>::parse();
    info!("{args}");

    let edges = dimacs::read_graph::<ResidualCapacity>(&args.graph, dimacs::WeightType::Unit);

    let coordinates = dimacs::read_coordinates(&args.coordinates);
    info!(
        "loaded {} nodes and {} edges",
        coordinates.len(),
        edges.len()
    );

    let mut partition_ids = vec![PartitionID::root(); coordinates.len()];

    // enqueue initial job for root node
    let proxy_vector = (0..coordinates.len()).collect_vec();
    let job = (edges.clone(), &coordinates, proxy_vector);
    let mut current_job_queue = vec![job];

    let sty = ProgressStyle::default_spinner()
        .template("{spinner:.green} [{elapsed_precise}] {wide_bar:.green/yellow} {msg}")
        .progress_chars("#>-");

    let mut current_level = 0;
    while !current_job_queue.is_empty() && current_level < args.target_level {
        let mut next_job_queue = Vec::new();
        let pb = ProgressBar::new(current_job_queue.len() as u64);
        pb.set_style(sty.clone());

        current_job_queue.iter().enumerate().for_each(|(id, job)| {
            pb.set_message(format!("cell #{}", id));
            pb.inc(1);

            // we use the count of coordinates as an upper bound to the cut size
            let upper_bound = Arc::new(AtomicI32::new((&job.2).len().try_into().unwrap()));
            // run inertial flow on all four axes
            let best_max_flow = (0..4)
                .into_par_iter()
                .map(|axis| -> FlowResult {
                    inertial_flow::sub_step(
                        axis,
                        &job.0,
                        job.1,
                        &job.2,
                        args.b_factor,
                        upper_bound.clone(),
                    )
                })
                .min_by(|a, b| flow_cmp(a, b));

            let result = best_max_flow.unwrap();
            debug!(
                "best max-flow: {}, balance: {:.3}",
                result.flow, result.balance
            );

            debug!("assigning partition ids to all nodes");
            // assign child ids for nodes by iterating over the proxy vector elements
            job.2.iter().for_each(|id| {
                result.assignment[*id]
                    .and_do(|| partition_ids[*id].make_left_child())
                    .or_do(|| partition_ids[*id].make_right_child());
            });
            // partition edge and node id sets for the next iterationß
            debug!("generating next level edges");
            let (left_edges, right_edges): (Vec<_>, Vec<_>) = (&job.0)
                .iter()
                .filter(|edge| partition_ids[edge.source()] == partition_ids[edge.target()])
                .partition(|edge| partition_ids[edge.source()].is_left_child());
            debug!("generating next level ids");
            let (left_ids, right_ids): (Vec<_>, Vec<_>) = (&job.2)
                .iter()
                .partition(|id| partition_ids[**id].is_left_child());
            // iterate on both halves
            next_job_queue.push((left_edges, &coordinates, left_ids));
            next_job_queue.push((right_edges, &coordinates, right_ids));
        });
        current_level += 1;
        pb.finish_with_message(format!("level {current_level} done"));
        current_job_queue = next_job_queue;
    }

    write_results(&args, &partition_ids, &coordinates, &edges);

    for id in partition_ids {
        debug_assert_eq!(id.level(), args.target_level);
    }
    info!("done.");
}
