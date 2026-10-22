//! What it costs to tabulate the cells of an edge-based overlay.
//!
//!     edge_based_customize <graph> <coordinates> <level directory> [levels]
//!
//! The cells are asked for level by level, since a coarse one is built out of
//! the cells below it and asking for it first would customize those on the way
//! anyway, without saying which level the time went to.
use std::{env::args, time::Instant};

use toolbox_rs::{
    edge_based::AnglePenalty,
    edge_based_overlay::EdgeBasedOverlay,
    geometry::FPCoordinate,
    graph::Graph,
    io,
    level_directory::{CellId, LevelDirectory},
    overlay::{CellTable, Overlay},
    static_graph::StaticGraph,
};

fn main() {
    let graph = args().nth(1).expect("a graph");
    let coordinates = args().nth(2).expect("coordinates");
    let directory = args().nth(3).expect("a level directory");
    let upto: usize = args()
        .nth(4)
        .map_or(usize::MAX, |given| given.parse().expect("a level count"));

    let graph = StaticGraph::new(io::read_edges_from_file(&graph));
    let coordinates = io::read_vec_from_file::<FPCoordinate>(&coordinates);
    let directory: LevelDirectory = io::read_from_file(&directory);
    println!(
        "{} nodes, {} arcs, {} levels",
        graph.number_of_nodes(),
        graph.number_of_edges(),
        directory.levels()
    );

    let started = Instant::now();
    let overlay =
        EdgeBasedOverlay::new(graph, coordinates, AnglePenalty::new(30., 100), &directory);
    println!(
        "built the overlay in {:.1} s",
        started.elapsed().as_secs_f64()
    );

    println!(
        "\n{:>6} {:>10} {:>14} {:>16} {:>10}",
        "level", "cells", "ways out", "entries", "seconds"
    );
    for level in 0..directory.levels().min(upto) {
        let started = Instant::now();
        let (mut ways_out, mut entries, mut tabled) = (0_u64, 0_u128, 0_u64);
        for cell in 0..overlay.cells_on_level(level) {
            let Some(table) = overlay.distances_of(level, cell as CellId) else {
                continue;
            };
            let wide = table.border_nodes().len() as u64;
            ways_out += wide;
            entries += u128::from(table.entries() as u64);
            tabled += 1;
        }
        println!(
            "{level:>6} {:>10} {ways_out:>14} {entries:>16} {:>9.1}s",
            tabled,
            started.elapsed().as_secs_f64()
        );
    }
}
