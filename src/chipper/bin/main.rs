use bitvec::prelude::BitVec;
use clap::Parser;
use env_logger::Env;
use itertools::Itertools;
use log::{debug, info};
use rayon::prelude::*;
use std::{
    fs::File,
    io::Write,
    sync::{atomic::AtomicI32, Arc},
};
use toolbox_rs::{dimacs, dinic::Dinic, inertial_flow::Coefficients, max_flow::MaxFlow};

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
    #[clap(short, long)]
    output: String,

    #[clap(short, long)]
    remove_eigenloops: bool,
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
    info!("loading graph from {}", args.graph);
    info!("loading coordinates from {}", args.coordinates);

    let edges = dimacs::read_graph(&args.graph);
    info!("edge count: {}", edges.len());

    let coordinates = dimacs::read_coordinates(&args.coordinates);
    info!("coordinate count: {}", coordinates.len());

    let best_cost = Arc::new(AtomicI32::new(coordinates.len().try_into().unwrap()));

    let coefficients = Coefficients::new();
    // run inertial flow on all four axes
    let best_max_flow = (0..4)
        .into_par_iter()
        .map(|idx| -> (i32, f64, bitvec::vec::BitVec, Vec<usize>) {
            let current_coefficients = &coefficients[idx];
            info!("[{idx}] sorting nodes by {:?}", current_coefficients);
            // generate proxy list, coordinates vector itself is not touched
            let mut proxy_vector = (0..coordinates.len()).collect_vec();
            proxy_vector.sort_unstable_by_key(|a| -> i32 {
                coordinates[*a].lon * current_coefficients.0
                    + coordinates[*a].lat * current_coefficients.1
            });

            // TODO: make thresholds configurable
            let sources = &proxy_vector[0..proxy_vector.len() / 4];
            let targets = &proxy_vector[(proxy_vector.len() * 3 / 4) + 1..];

            info!("[{idx}] renumbering of inertial flow graph");
            let mut renumbering_table = vec![usize::MAX; coordinates.len()];
            // the mapping is input id -> dinic id

            for s in sources {
                renumbering_table[*s] = 0;
            }
            for t in targets {
                renumbering_table[*t] = 1;
            }

            // each thread holds their own copy of the edge set
            let mut edges = edges.clone();
            let mut i = 1;
            for mut e in &mut edges {
                // nodes in the edge set are numbered consecutively
                if renumbering_table[e.source] == usize::MAX {
                    i += 1;
                    renumbering_table[e.source] = i;
                }
                if renumbering_table[e.target] == usize::MAX {
                    i += 1;
                    renumbering_table[e.target] = i;
                }
                e.source = renumbering_table[e.source];
                e.target = renumbering_table[e.target];
            }
            info!("[{idx}] instantiating min-cut solver, epsilon 0.25");

            // remove eigenloops
            if args.remove_eigenloops {
                let edge_count_before = edges.len();
                edges.retain(|edge| edge.source != edge.target);
                info!(
                    "[{idx}] eigenloop removal - edge count before {}, after {}",
                    edge_count_before,
                    edges.len()
                );
            }

            let mut max_flow_solver = Dinic::from_generic_edge_list(edges, 0, 1);
            info!("[{idx}] instantiated min-cut solver");
            max_flow_solver.run_with_upper_bound(best_cost.clone());

            let max_flow = max_flow_solver.max_flow();

            if max_flow.is_err() {
                // Error is returned in case the search is aborted early
                return (i32::MAX, 0., BitVec::new(), Vec::new());
            }
            let max_flow = max_flow.expect("max flow computation did not run");

            info!("[{idx}] computed max flow: {}", max_flow);
            let assignment = max_flow_solver
                .assignment(0)
                .expect("max flow computation did not run");

            let left_size = assignment.iter().filter(|b| !**b).count() + sources.len() - 1;
            let right_size = assignment.iter().filter(|b| **b).count() + targets.len() - 1;
            info!(
                "[{idx}] assignment has {} total entries",
                left_size + right_size
            );
            debug!("[{idx}] assignment has {} 1-entries", right_size);
            debug!("[{idx}] assignment has {} 0-entries", left_size);

            let balance =
                std::cmp::min(left_size, right_size) as f64 / (right_size + left_size) as f64;

            info!("[{idx}] balance: {balance}");

            (max_flow, balance, assignment, renumbering_table)
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

    let mut file = File::create(&args.output).expect("output file cannot be opened");
    file.write_all("latitude, longitude\n".as_bytes())
        .expect("error writing file");

    // fetch the cut and output its geometry
    info!("writing cut geometry to {}", &args.output);
    for edge in &edges {
        if assignment[renumbering_table[edge.source]] != assignment[renumbering_table[edge.target]]
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
    //TODO: assign ids for nodes and iterate on both halves
}
