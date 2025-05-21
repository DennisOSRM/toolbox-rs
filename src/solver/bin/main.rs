use clap::Parser;
use std::path::PathBuf;
use std::time::Instant;
use toolbox_rs::{complete_graph::CompleteGraph, tsplib};

#[derive(Parser, Debug)]
#[clap(name = "tsp_solver", about = "A simple TSP solver")]
struct Args {
    /// Path to the TSP file
    #[clap(short, long, value_parser, required_unless_present = "test")]
    input: Option<PathBuf>,

    /// Whether to use nearest neighbor algorithm
    #[clap(short, long)]
    nearest_neighbor: bool,

    /// Whether to use brute force algorithm (warning: only use for small instances!)
    #[clap(short, long)]
    brute_force: bool,

    /// Whether to use dynamic programming algorithm (works for medium-sized instances)
    #[clap(short, long)]
    dynamic_programming: bool,

    /// Whether to run the test cases
    #[clap(short, long)]
    test: bool,
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

    // Run test cases if requested
    if args.test {
        run_test_cases();
        return Ok(());
    }

    // Regular TSP solver mode requires an input file
    let input_path = args
        .input
        .as_ref()
        .expect("Input file required when not in test mode");
    println!("Reading TSP file: {}", input_path.display());
    let start = Instant::now();
    let instance =
        tsplib::read_tsp_file(input_path.to_str().unwrap()).expect("graph could not be read");
    let (sites, matrix): (Option<Vec<tsplib::TspSite>>, Option<Vec<Vec<i32>>>) = match instance {
        tsplib::TspInstance::Coordinates(sites) => (Some(sites), None),
        tsplib::TspInstance::ExplicitMatrix(matrix) => (None, Some(matrix)),
    };
    if let Some(ref sites) = sites {
        println!("Read {} sites in {:?}", sites.len(), start.elapsed());
    } else if let Some(ref matrix) = matrix {
        println!(
            "Read explicit distance matrix ({}x{}) in {:?}",
            matrix.len(),
            matrix[0].len(),
            start.elapsed()
        );
    }

    // Build a complete graph with distances between all sites
    let start = Instant::now();
    println!("Building distance matrix...");
    let num_nodes = if let Some(ref s) = sites {
        s.len()
    } else {
        matrix.as_ref().unwrap().len()
    };
    let mut graph = CompleteGraph::new(num_nodes);

    if let Some(ref sites) = sites {
        // Compute distances between all pairs of sites (coordinate mode)
        for i in 0..num_nodes {
            for j in 0..num_nodes {
                if i == j {
                    continue; // Skip diagonal
                }
                let distance = tsplib::euclidean_distance(&sites[i], &sites[j]);
                *graph.get_mut(i, j) = distance;
            }
        }
    } else if let Some(ref matrix) = matrix {
        // Use explicit matrix
        for i in 0..num_nodes {
            for j in 0..num_nodes {
                *graph.get_mut(i, j) = matrix[i][j];
            }
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
    } else if args.brute_force {
        let start = Instant::now();
        let (_tour, length) = solve_brute_force(&graph);
        println!(
            "Brute force tour length: {} (computed in {:?})",
            length,
            start.elapsed()
        );
        println!("Tour length: {:?}", _tour.len());
    } else if args.dynamic_programming {
        let start = Instant::now();
        let (_tour, length) = solve_dynamic_programming(&graph);
        println!(
            "Dynamic programming tour length: {} (computed in {:?})",
            length,
            start.elapsed()
        );
        println!("Tour length: {:?}", _tour.len());
    } else {
        println!(
            "No solving method specified. Use --nearest-neighbor for nearest neighbor heuristic, --brute-force for optimal solution (small instances only), or --dynamic-programming for optimal solution (medium-sized instances)."
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

/// Solves the TSP using a brute force approach by checking all possible permutations
/// Warning: This has factorial time complexity O(n!) and is only suitable for small instances
fn solve_brute_force(graph: &CompleteGraph<i32>) -> (Vec<usize>, i32) {
    let n = graph.num_nodes();
    if n <= 1 {
        return (vec![0], 0);
    }

    // Initialize the best solution as a simple path 0,1,2,...,n-1,0
    let mut best_tour = (0..n).collect::<Vec<usize>>();
    let mut best_length = calculate_tour_length(&best_tour, graph);

    // Generate all permutations of nodes 1 to n-1 (keep 0 as the fixed starting point)
    let mut nodes: Vec<usize> = (1..n).collect();
    permute_and_evaluate(&mut nodes, &mut best_tour, &mut best_length, graph);

    (best_tour, best_length)
}

/// Helper function to calculate the total length of a tour
fn calculate_tour_length(tour: &[usize], graph: &CompleteGraph<i32>) -> i32 {
    let n = tour.len();
    if n <= 1 {
        return 0;
    }

    (0..n).map(|i| graph[(tour[i], tour[(i + 1) % n])]).sum()
}

/// Iteratively generates all permutations and evaluates them using a stack
fn permute_and_evaluate(
    nodes: &mut [usize],
    best_tour: &mut Vec<usize>,
    best_length: &mut i32,
    graph: &CompleteGraph<i32>,
) {
    // Each stack frame consists of (start, i, forward_direction)
    // - start: position we're working on
    // - i: current element being swapped with start
    // - forward_direction: true if we're going down (making swaps), false if we're unwinding
    let mut stack: Vec<(usize, usize, bool)> = Vec::new();

    // Initialize the stack with the first position
    stack.push((0, 0, true));

    while let Some((start, i, forward)) = stack.pop() {
        if forward {
            // println!("subtour: {:?}", &nodes[0..i]);
            if start == nodes.len() {
                // We have a complete permutation, evaluate it
                let mut tour = vec![0]; // Start from node 0
                tour.extend_from_slice(nodes);

                let tour_length = calculate_tour_length(&tour, graph);
                if tour_length < *best_length {
                    *best_tour = tour;
                    *best_length = tour_length;
                    // println!("New best tour with length {}", best_length);
                }
            } else if i < nodes.len() {
                // Swap elements
                nodes.swap(start, i);

                // Push backtracking state
                stack.push((start, i, false)); // Will undo this swap later

                // Move to next position
                stack.push((start + 1, start + 1, true));

                // Also schedule the next swap at this level
                if i + 1 < nodes.len() {
                    stack.push((start, i + 1, true));
                }
            }
        } else {
            // Undo swap (backtrack)
            nodes.swap(start, i);
        }
    }
}

/// Solves the TSP using dynamic programming (Held-Karp algorithm)
/// Time complexity: O(n^2 * 2^n), space complexity: O(n * 2^n)
/// Appropriate for problems with up to ~20-25 cities
fn solve_dynamic_programming(graph: &CompleteGraph<i32>) -> (Vec<usize>, i32) {
    let n = graph.num_nodes();
    if n <= 1 {
        return (vec![0], 0);
    }

    // For larger n, we would run out of memory
    if n > 25 {
        println!("Warning: Dynamic programming approach may run out of memory for n > 25.");
        println!("Consider using nearest neighbor heuristic for larger instances.");
    }

    // Initialize DP table: dp[S][i] = min cost to visit all vertices in subset S and end at vertex i
    // We represent S as a bitmask where bit j is set if vertex j is in the subset
    let total_states = 1 << (n - 1); // We only need to store states for vertices 1 to n-1
    let mut dp = vec![vec![i32::MAX; n]; total_states];

    // Initialize parent pointers to reconstruct the optimal tour
    let mut parent = vec![vec![usize::MAX; n]; total_states];

    // Base case: cost to go from 0 to j
    for j in 1..n {
        dp[1 << (j - 1)][j] = graph[(0, j)];
        parent[1 << (j - 1)][j] = 0;
    } // Iterate over all subsets of vertices (excluding vertex 0)
    // Iterate over all subsets of vertices (excluding vertex 0)
    // Iterate over all subsets of vertices (excluding vertex 0)
    for subset_size in 2..n {
        // Generate all subsets of size subset_size manually
        for subset in 0..((1 << (n - 1)) as usize) {
            // Count the number of bits set (i.e., the size of the subset)
            if subset.count_ones() != subset_size as u32 {
                continue;
            }

            // For each ending vertex in the subset
            for j in 1..n {
                // Check if j is in the subset
                if (subset & (1 << (j - 1))) != 0 {
                    // Remove j from the subset
                    let prev_subset = subset & !(1 << (j - 1));

                    // Try all possible previous vertices
                    for k in 1..n {
                        // Check if k is in the subset and is not j
                        if (prev_subset & (1 << (k - 1))) != 0 {
                            let cost = dp[prev_subset][k] + graph[(k, j)];
                            if cost < dp[subset][j] {
                                dp[subset][j] = cost;
                                parent[subset][j] = k;
                            }
                        }
                    }
                }
            }
        }
    }

    // Find the optimal cost to visit all vertices and return to vertex 0
    let full_subset = (1 << (n - 1)) - 1;
    let mut last_vertex = 1;
    let mut min_cost = dp[full_subset][1] + graph[(1, 0)];

    for j in 2..n {
        let cost = dp[full_subset][j] + graph[(j, 0)];
        if cost < min_cost {
            min_cost = cost;
            last_vertex = j;
        }
    }

    // Reconstruct the optimal tour
    let mut tour = Vec::with_capacity(n + 1);
    tour.push(0); // Start at vertex 0

    let mut curr_vertex = last_vertex;
    let mut curr_subset = full_subset;

    while curr_subset > 0 {
        tour.push(curr_vertex);
        let next_vertex = parent[curr_subset][curr_vertex];
        curr_subset &= !(1 << (curr_vertex - 1));
        curr_vertex = next_vertex;
    }

    tour.reverse(); // Reverse to get correct order (0 -> ... -> last_vertex)
    // tour.push(0); // Complete the cycle back to vertex 0

    (tour, min_cost)
}

/// Creates a test graph with the specified number of cities in a challenging configuration
fn create_test_graph(num_cities: usize) -> CompleteGraph<i32> {
    let mut graph = CompleteGraph::new(num_cities);

    // Populate the graph with distances that might trick greedy algorithms
    match num_cities {
        3 => {
            // Simple triangle with non-uniform distances
            *graph.get_mut(0, 1) = 10;
            *graph.get_mut(1, 0) = 10;
            *graph.get_mut(0, 2) = 15;
            *graph.get_mut(2, 0) = 15;
            *graph.get_mut(1, 2) = 20;
            *graph.get_mut(2, 1) = 20;
        }
        4 => {
            // Square with crossing paths - greedy can get trapped
            *graph.get_mut(0, 1) = 10;
            *graph.get_mut(1, 0) = 10;
            *graph.get_mut(1, 2) = 10;
            *graph.get_mut(2, 1) = 10;
            *graph.get_mut(2, 3) = 10;
            *graph.get_mut(3, 2) = 10;
            *graph.get_mut(3, 0) = 10;
            *graph.get_mut(0, 3) = 10;
            // Crossing diagonals with different costs
            *graph.get_mut(0, 2) = 25;
            *graph.get_mut(2, 0) = 25;
            *graph.get_mut(1, 3) = 20;
            *graph.get_mut(3, 1) = 20;
        }
        5 => {
            // Pentagon with some shortcuts - designed so nearest neighbor gets trapped
            for i in 0..5 {
                for j in 0..5 {
                    if i == j {
                        continue;
                    }

                    // Default high edge costs - connection between non-adjacent cities
                    let mut cost = 30;

                    // Adjacent nodes on the perimeter have lower cost (10)
                    if j == (i + 1) % 5 || i == (j + 1) % 5 {
                        cost = 10;
                    }

                    // Add a trap for nearest neighbor algorithm
                    // From node 0, the nearest is node 3 (cost 8)
                    // But this leads to a sub-optimal tour
                    if (i == 0 && j == 3) || (i == 3 && j == 0) {
                        cost = 8; // Tempting shortcut that leads to suboptimal solution
                    }

                    // The optimal route is clockwise/counterclockwise around the pentagon

                    *graph.get_mut(i, j) = cost;
                }
            }
        }
        _ => {
            // For other sizes, create a random but consistent graph
            for i in 0..num_cities {
                for j in 0..num_cities {
                    if i != j {
                        // A simple pattern that ensures the graph is symmetric
                        let distance = ((i as i32 + 1) * (j as i32 + 1)) % 30 + 1;
                        *graph.get_mut(i, j) = distance;
                    }
                }
            }
        }
    }

    graph
}

/// Run test cases for different algorithms with various city counts
fn run_test_cases() {
    println!("\n--- Running Test Cases ---");

    for num_cities in [3, 4, 5, 6, 7, 8, 9, 10 /*, 11, 12, 13, 14, 15*/] {
        // println!("\nTest case with {} cities:", num_cities);
        let graph = create_test_graph(num_cities);

        // Run nearest neighbor
        let start = Instant::now();
        let (nn_tour, nn_length) = solve_nearest_neighbor(&graph);
        let nn_duration = start.elapsed();

        // Run brute force
        let start = Instant::now();
        let (bf_tour, bf_length) = solve_brute_force(&graph);
        let bf_duration = start.elapsed();

        // Run dynamic programming
        let start = Instant::now();
        let (dp_tour, dp_length) = solve_dynamic_programming(&graph);
        let dp_duration = start.elapsed();

        // Print results
        println!(
            "Nearest neighbor   : tour={:?}, length={} (in {:?})",
            nn_tour, nn_length, nn_duration
        );
        println!(
            "Dynamic programming: tour={:?}, length={} (in {:?})",
            dp_tour, dp_length, dp_duration
        );
        println!(
            "Brute force        : tour={:?}, length={} (in {:?})",
            bf_tour, bf_length, bf_duration
        );

        // Calculate and display the optimality gap for nearest neighbor
        if bf_length < nn_length {
            let gap_percent = (nn_length - bf_length) as f64 / bf_length as f64 * 100.0;
            println!(
                "NN optimality gap: {}% (NN is {}% worse than optimal)",
                gap_percent.round(),
                gap_percent.round()
            );
        } else {
            println!("Nearest neighbor found an optimal solution!");
        }

        // Verify brute force is optimal
        assert!(
            bf_length <= nn_length,
            "Brute force solution (length={}) should be at least as good as nearest neighbor (length={})",
            bf_length,
            nn_length
        );

        // Verify dynamic programming gives the same optimal result as brute force
        assert!(
            bf_length == dp_length,
            "Dynamic programming solution (length={}) should be exactly as good as brute force (length={})",
            dp_length,
            bf_length
        );
    }

    println!(
        "\nAll tests passed! ✅ Both dynamic programming and brute force solutions were optimal in all cases."
    );
}
