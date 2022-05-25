mod command_line;
mod serialize;

use env_logger::Env;
use itertools::Itertools;

use log::info;
use rayon::prelude::*;
use std::sync::{atomic::AtomicI32, Arc};
use toolbox_rs::{
    dimacs, edge::Edge, inertial_flow, max_flow::ResidualCapacity, partition::PartitionID,
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

    // enqueue job for root node
    let proxy_vector = (0..coordinates.len()).collect_vec();
    let job = (edges.clone(), &coordinates, proxy_vector);
    let mut current_job_queue = vec![job];

    let mut current_level = 0;
    while !current_job_queue.is_empty() && current_level < args.target_level {
        let mut next_job_queue = Vec::new();

        current_job_queue.iter().for_each(|job| {
            // we use the count of coordinates as an upper bound to the cut size
            let upper_bound = Arc::new(AtomicI32::new((&job.2).len().try_into().unwrap()));
            // run inertial flow on all four axes
            let best_max_flow = (0..4)
                .into_par_iter()
                .map(|axis| -> (i32, f64, bitvec::vec::BitVec, Vec<usize>) {
                    inertial_flow::sub_step(
                        axis,
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
                        // balance is at most 0.5 and the closer the value the more balanced the partitions
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
                let index_of_node = renumbering_table[*id];
                // unconnected nodes don't have an assignment, i.e. an index into the assigment, they are assigned semi-randomly by their id being odd or even
                if (index_of_node < partition_ids.len() && assignment[index_of_node])
                    || (index_of_node >= partition_ids.len() && index_of_node % 2 == 0)
                {
                    partition_ids[*id].make_left_child();
                } else {
                    partition_ids[*id].make_right_child();
                }
            }
            // partition proxy vector and edge sets
            info!("generating next level edges");
            let (left_edges, right_edges): (Vec<_>, Vec<_>) = (&job.0)
                .iter()
                .filter(|edge| partition_ids[edge.source()] == partition_ids[edge.target()])
                .partition(|edge| partition_ids[edge.source()].is_left_child());
            info!("generating next level ids");
            let (left_ids, right_ids): (Vec<_>, Vec<_>) = (&job.2)
                .iter()
                .partition(|id| partition_ids[**id].is_left_child());
            // iterate on both halves
            next_job_queue.push((left_edges, &coordinates, left_ids));
            next_job_queue.push((right_edges, &coordinates, right_ids));
        });
        current_level += 1;
        current_job_queue = next_job_queue;
    }

    write_results(&args, &partition_ids, &coordinates, &edges);

    for id in partition_ids {
        debug_assert_eq!(id.level(), args.target_level);
    }
    info!("done.");
}
