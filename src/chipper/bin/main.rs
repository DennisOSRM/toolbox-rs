mod command_line;
mod serialize;

use clap::Parser;
use core::panic;
use env_logger::Env;

use crate::{
    command_line::recursion_in_range,
    serialize::{assignment_csv, binary_partition_file, geometry_list},
};
use log::info;
use rayon::prelude::*;
use std::sync::{atomic::AtomicI32, Arc};
use toolbox_rs::{
    dimacs,
    inertial_flow::{self},
    max_flow::ResidualCapacity,
    partition::PartitionID,
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

    /// Network recursion to use
    #[clap(short, long, parse(try_from_str=recursion_in_range), default_value_t = 0)]
    recursion_depth: usize,

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
    info!("recursion depth: {}", args.recursion_depth);
    info!("balance factor: {}", args.b_factor);
    info!("loading graph from {}", args.graph);
    info!("loading coordinates from {}", args.coordinates);

    let edges = dimacs::read_graph::<ResidualCapacity>(&args.graph, dimacs::WeightType::Unit);
    info!("edge count: {}", edges.len());

    let coordinates = dimacs::read_coordinates(&args.coordinates);
    info!("coordinate count: {}", coordinates.len());

    let mut partition_ids = vec![PartitionID::root(); coordinates.len()];

    // we use the count of coordinates as an upper bound to the cut size
    let upper_bound = Arc::new(AtomicI32::new(coordinates.len().try_into().unwrap()));

    // run inertial flow on all four axes
    let best_max_flow = (0..4)
        .into_par_iter()
        .map(|idx| -> (i32, f64, bitvec::vec::BitVec, Vec<usize>) {
            inertial_flow::sub_step(
                idx,
                &edges,
                &coordinates,
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
    // assign ids for nodes
    for i in 0..partition_ids.len() {
        match assignment[renumbering_table[i]] {
            true => partition_ids[i] = partition_ids[i].left_child(),
            false => partition_ids[i] = partition_ids[i].right_child(),
        }
    }

    // TODO: iterate on both halves

    if !args.assignment_csv.is_empty() {
        info!(
            "writing partition ids into csv file: {}",
            args.assignment_csv
        );
        assignment_csv(&args.assignment_csv, &partition_ids, &coordinates);
    }

    if !args.cut_csv.is_empty() {
        info!("writing cut geometry to {}", &args.cut_csv);
        geometry_list(
            &args.cut_csv,
            edges,
            assignment,
            renumbering_table,
            coordinates,
        );
    }

    if !args.partition_file.is_empty() {
        info!("writing partition ids to {}", &args.partition_file);
        binary_partition_file(&args.partition_file, partition_ids);
    }
    info!("done.");
}
