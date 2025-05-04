use clap::Parser;
use std::path::PathBuf;
use std::time::Instant;
use toolbox_rs::{complete_graph::CompleteGraph, tsplib};

#[derive(Parser, Debug)]
#[clap(name = "tsp_solver", about = "A simple TSP solver")]
struct Args {
    /// Path to the TSP file
    #[clap(short, long, value_parser)]
    input: PathBuf,

    /// Whether to use nearest neighbor algorithm
    #[clap(short, long)]
    nearest_neighbor: bool,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    let args = Args::parse();
    println!(r#"     ___              _                            "#);
    println!(r#"    / __|    ___     | |    __ __    ___      _ _  "#);
    println!(r#"    \__ \   / _ \    | |    \ V /   / -_)    | '_| "#);
    println!(r#"    |___/   \___/   _|_|_   _\_/_   \___|   _|_|_  "#);
    println!(r#"  _|"""""|_|"""""|_|"""""|_|"""""|_|"""""|_|"""""| "#);
    println!(r#"  "`-0-0-'"`-0-0-'"`-0-0-'"`-0-0-'"`-0-0-'"`-0-0-' "#);
    println!("Reading TSP file: {}", args.input.display());
    let start = Instant::now();
    let sites =
        tsplib::read_tsp_file(args.input.to_str().unwrap()).expect("graph could not be read");
    println!("Read {} sites in {:?}", sites.len(), start.elapsed());

    // Build a complete graph with distances between all sites
    let start = Instant::now();
    println!("Building distance matrix...");
    let num_nodes = sites.len();
    let mut graph = CompleteGraph::new(num_nodes);

    // Compute distances between all pairs of sites
    for i in 0..num_nodes {
        for j in 0..num_nodes {
            if i == j {
                continue; // Skip diagonal
            }
            let distance = tsplib::euclidean_distance(&sites[i], &sites[j]);
            *graph.get_mut(i, j) = distance;
        }
    }
    println!("Built distance matrix in {:?}", start.elapsed());

    // Solve TSP using a simple algorithm (nearest neighbor heuristic)
    if args.nearest_neighbor {
        let start = Instant::now();
        let (_tour, length) = solve_nearest_neighbor(&graph);
        println!(
            "Nearest neighbor tour length: {} (computed in {:?})",
            length,
            start.elapsed()
        );
        println!("Tour length: {:?}", _tour.len());
    } else {
        println!(
            "No solving method specified. Use --nearest-neighbor to solve using nearest neighbor heuristic."
        );
    }

    Ok(())
}

/// Solves the TSP using a simple nearest neighbor heuristic
fn solve_nearest_neighbor(graph: &CompleteGraph<i32>) -> (Vec<usize>, i32) {
    let n = graph.num_nodes();
    if n <= 1 {
        return (vec![0], 0);
    }

    let mut tour = Vec::with_capacity(n);
    let mut visited = vec![false; n];
    let mut current = 0; // Start from node 0
    let mut tour_length = 0;

    tour.push(current);
    visited[current] = true;

    // Find n-1 more nodes to complete the tour
    for _ in 1..n {
        let mut next = usize::MAX;
        let mut min_distance = i32::MAX;

        // Find the nearest unvisited node
        for j in 0..n {
            if !visited[j] && current != j && graph[(current, j)] < min_distance {
                next = j;
                min_distance = graph[(current, j)];
            }
        }

        if next == usize::MAX {
            // Shouldn't happen in a complete graph
            break;
        }

        tour.push(next);
        visited[next] = true;
        tour_length += min_distance;
        current = next;
    }

    // Add the distance back to the starting node to complete the tour
    tour_length += graph[(current, 0)];

    (tour, tour_length)
}
