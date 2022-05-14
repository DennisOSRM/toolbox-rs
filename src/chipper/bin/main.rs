use clap::Parser;
use core::panic;
use env_logger::Env;

use log::info;
use rayon::prelude::*;
use std::{
    fs::File,
    io::Write,
    sync::{atomic::AtomicI32, Arc},
};
use toolbox_rs::{
    dimacs,
    inertial_flow::{self},
    max_flow::ResidualCapacity,
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

    /// path to the output file
    #[clap(short, long, default_value_t = String::new())]
    output: String,

    /// balance factor to use
    #[clap(short, long, default_value_t = 0.25)]
    b_factor: f64,
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
    info!("balance factor: {}", args.b_factor);
    info!("loading graph from {}", args.graph);
    info!("loading coordinates from {}", args.coordinates);

    let edges = dimacs::read_graph::<ResidualCapacity>(&args.graph, dimacs::WeightType::Unit);
    info!("edge count: {}", edges.len());

    let coordinates = dimacs::read_coordinates(&args.coordinates);
    info!("coordinate count: {}", coordinates.len());

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

    if !args.output.is_empty() {
        let mut file = File::create(&args.output).expect("output file cannot be opened");
        file.write_all("latitude, longitude\n".as_bytes())
            .expect("error writing file");

        // fetch the cut and output its geometry
        info!("writing cut geometry to {}", &args.output);
        for edge in &edges {
            if assignment[renumbering_table[edge.source]]
                != assignment[renumbering_table[edge.target]]
            {
                file.write_all(
                    (coordinates[edge.source].lat as f64 / 1000000.)
                        .to_string()
                        .as_bytes(),
                )
                .expect("error writing file");
                file.write_all(b", ").expect("error writing file");
                file.write_all(
                    (coordinates[edge.source].lon as f64 / 1000000.)
                        .to_string()
                        .as_bytes(),
                )
                .expect("error writing file");
                file.write_all(b"\n").expect("error writing file");

                file.write_all(
                    (coordinates[edge.target].lat as f64 / 1000000.)
                        .to_string()
                        .as_bytes(),
                )
                .expect("error writing file");
                file.write_all(b", ").expect("error writing file");
                file.write_all(
                    (coordinates[edge.target].lon as f64 / 1000000.)
                        .to_string()
                        .as_bytes(),
                )
                .expect("error writing file");
                file.write_all(b"\n").expect("error writing file");
            }
        }
        file.flush().expect("error writing file");
        info!("done.");
    }
    //TODO: assign ids for nodes and iterate on both halves
}
