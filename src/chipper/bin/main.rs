mod command_line;
mod serialize;

use clap::Parser;
use core::panic;
use env_logger::Env;
use itertools::Itertools;

use crate::{
    command_line::recursion_in_range,
    serialize::{assignment_csv, binary_partition_file, cut_csv},
};
use log::info;
use rayon::prelude::*;
use std::sync::{atomic::AtomicI32, Arc};
use toolbox_rs::{
    dimacs, edge::Edge, inertial_flow, max_flow::ResidualCapacity, partition::PartitionID,
};

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// path to the input graph
    #[clap(short, long)]
    graph: String,

    /// path to the input coordinates
    #[clap(short, long)]
    coordinates: String,

    /// path to the cut-csv file
    #[clap(short = 'o', long, default_value_t = String::new())]
    cut_csv: String,

    /// path to the assignment-csv file
    #[clap(short, long, default_value_t = String::new())]
    assignment_csv: String,

    /// balance factor to use
    #[clap(short, long, default_value_t = 0.25)]
    b_factor: f64,

    /// target level of the resulting partition
    #[clap(short, long, parse(try_from_str=recursion_in_range), default_value_t = 1)]
    target_level: u8,

    /// path to the output file with partition ids
    #[clap(short, long, default_value_t = String::new())]
    partition_file: String,
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

    // parse command line parameters
    let args = Args::parse();

    if args.b_factor > 0.5 || args.b_factor < 0. {
        panic!("balance factor must be between 0 and 0.5");
    }
    info!("target level: {}", args.target_level);
    info!("balance factor: {}", args.b_factor);
    info!("loading graph from {}", args.graph);
    info!("loading coordinates from {}", args.coordinates);

    let edges = dimacs::read_graph::<ResidualCapacity>(&args.graph, dimacs::WeightType::Unit);
    info!("edge count: {}", edges.len());

    let coordinates = dimacs::read_coordinates(&args.coordinates);
    info!("coordinate count: {}", coordinates.len());

    let mut partition_ids = vec![PartitionID::root(); coordinates.len()];

    // enqueue job for root node
    let proxy_vector = (0..coordinates.len()).collect_vec();
    let job = (edges.clone(), &coordinates, proxy_vector);
    let mut level_queue = vec![job];

    let mut iteration_count = 0;
    while !level_queue.is_empty() && iteration_count < args.target_level {
        let mut next_level_queue = Vec::new();

        level_queue.iter().for_each(|job| {
            // we use the count of coordinates as an upper bound to the cut size
            let upper_bound = Arc::new(AtomicI32::new((&job.2).len().try_into().unwrap()));
            // run inertial flow on all four axes
            let best_max_flow = (0..4)
                .into_par_iter()
                .map(|idx| -> (i32, f64, bitvec::vec::BitVec, Vec<usize>) {
                    inertial_flow::sub_step(
                        idx,
                        &job.0,
                        job.1,
                        &job.2,
                        args.b_factor,
                        upper_bound.clone(),
                    )
                })
                .min_by(|a, b| {
                    if a.0 == b.0 {
                        // note that a and b are inverted here on purpose:
                        // balance is at most 0.5 and we want the closest, ie. largest value to it
                        return b.1.partial_cmp(&a.1).unwrap();
                    }
                    a.0.cmp(&b.0)
                });

            let (max_flow, balance, assignment, renumbering_table) = best_max_flow.unwrap();
            info!("best max-flow: {}, balance: {:.3}", max_flow, balance);

            info!("assigning partition ids to all nodes");
            // assign ids for nodes by iterating over the proxy vector elements
            for id in &job.2 {
                // TODO: if this doesn't work in parallel then return all the assignments, collect and flatten, s.t. the partition ids are assigned at the end of the level
                let idx = renumbering_table[*id];
                if idx > partition_ids.len() {
                    continue;
                }
                match assignment[idx] {
                    true => partition_ids[*id].make_left_child(),
                    false => partition_ids[*id].make_right_child(),
                }
            }
            // partition proxy vector and edge sets
            info!("generating next level edges");
            let (left_edges, right_edges): (Vec<_>, Vec<_>) = (&job.0)
                .iter()
                .filter(|edge| partition_ids[edge.source()] == partition_ids[edge.target()])
                .partition(|edge| partition_ids[edge.source()].is_left_child());
            info!("generating next level ids");
            let (left_ids, right_ids): (Vec<_>, Vec<_>) = (&job.2).iter().partition(|id| {
                let idx = renumbering_table[**id];
                // nodes can get cut off from edges. They need to be assigned to a partition
                if idx == usize::MAX {
                    let left = *id % 2 == 0;
                    match left {
                        true => partition_ids[**id].make_left_child(),
                        false => partition_ids[**id].make_right_child(),
                    }
                    return left;
                }
                assignment[idx]
            });
            next_level_queue.push((left_edges, &coordinates, left_ids));
            next_level_queue.push((right_edges, &coordinates, right_ids));
        });
        iteration_count += 1;
        // iterate on both halves swapping jobs in next_level_queue to the job queue
        level_queue = next_level_queue;
    }

    if !args.assignment_csv.is_empty() {
        info!("writing partition csv into: {}", args.assignment_csv);
        assignment_csv(&args.assignment_csv, &partition_ids, &coordinates);
    }

    if !args.cut_csv.is_empty() {
        info!("writing cut csv to {}", &args.cut_csv);
        cut_csv(&args.cut_csv, &edges, &partition_ids, &coordinates);
    }

    if !args.partition_file.is_empty() {
        info!("writing partition ids to {}", &args.partition_file);
        binary_partition_file(&args.partition_file, &partition_ids);
    }

    for id in partition_ids {
        debug_assert_eq!(id.level(), args.target_level);
    }
    info!("done.");
}
