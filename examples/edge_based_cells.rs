//! What an edge-based overlay would cost against the node-based one it is
//! built from.
//!
//!     edge_based_cells <graph> <level directory>
//!
//! An edge-based node is an arc, so a cell holds the arcs whose tail lies in
//! it, and its border is the arcs that cross into or out of it rather than the
//! nodes that sit on the boundary. A cell's table is every way in against
//! every way out, so what it costs is the entering arcs times the leaving
//! ones, where the node-based table costs the border nodes squared.
//!
//! Nothing is customized here. This counts what customizing would have to hold.
use std::env::args;

use toolbox_rs::{
    graph::{Arcs, NodeID},
    io,
    level_directory::LevelDirectory,
    static_graph::StaticGraph,
};

fn main() {
    let graph = args().nth(1).expect("a graph");
    let directory = args().nth(2).expect("a level directory");

    let graph = StaticGraph::new(io::read_edges_from_file(&graph));
    let directory: LevelDirectory = io::read_from_file(&directory);
    println!(
        "{} nodes, {} arcs, {} levels",
        graph.number_of_nodes(),
        graph.number_of_edges(),
        directory.levels()
    );

    println!(
        "\n{:>6} {:>10} {:>14} {:>12} {:>12} {:>16} {:>16} {:>16} {:>8}",
        "level",
        "cells",
        "border nodes",
        "rows",
        "columns",
        "node entries",
        "square now",
        "rectangular",
        "saving"
    );

    for level in 0..directory.levels() {
        let cells = directory.cells_on_level(level);
        // a node the boundary touches at all, one a route can enter by, and the
        // arcs that cross out of the cell
        let mut touched = vec![false; graph.number_of_nodes()];
        let mut entered = vec![false; graph.number_of_nodes()];
        let mut crossing_out = vec![0_u64; cells];

        for tail in 0..graph.number_of_nodes() {
            let mine = directory.cell_of(tail, level) as usize;
            for arc in graph.edge_range(tail) {
                let head = graph.target(arc) as NodeID;
                if mine == directory.cell_of(head, level) as usize {
                    continue;
                }
                crossing_out[mine] += 1;
                touched[tail] = true;
                touched[head] = true;
                // the arc runs into the other cell, so its head is a way in
                entered[head] = true;
            }
        }

        // what each cell holds, counted the three ways
        let mut border_nodes = vec![0_u64; cells];
        let mut leaving_touched = vec![0_u64; cells];
        let mut leaving_entered = vec![0_u64; cells];
        for node in 0..graph.number_of_nodes() {
            let cell = directory.cell_of(node, level) as usize;
            let out = graph.edge_range(node).len() as u64;
            if touched[node] {
                border_nodes[cell] += 1;
                leaving_touched[cell] += out;
            }
            if entered[node] {
                leaving_entered[cell] += out;
            }
        }

        let (mut nodes_sq, mut square, mut tighter) = (0_u128, 0_u128, 0_u128);
        let (mut rows, mut columns) = (0_u64, 0_u64);
        for cell in 0..cells {
            nodes_sq += u128::from(border_nodes[cell]) * u128::from(border_nodes[cell]);
            square += u128::from(leaving_touched[cell]) * u128::from(leaving_touched[cell]);
            tighter += u128::from(leaving_entered[cell]) * u128::from(crossing_out[cell]);
            rows += leaving_entered[cell];
            columns += crossing_out[cell];
        }
        println!(
            "{level:>6} {:>10} {:>14} {:>12} {:>12} {:>16} {:>16} {:>16} {:>7.2}x",
            cells,
            border_nodes.iter().sum::<u64>(),
            rows,
            columns,
            nodes_sq,
            square,
            tighter,
            tighter as f64 / square as f64
        );
    }
}
