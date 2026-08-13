//! Checks a level directory against the graph it was built from: whether every
//! cell of every level holds together, and how large the cells came out.
//!
//! A cell that falls into several pieces cannot be crossed without leaving it,
//! which is what the distance table of a cell assumes it can do. Merging only
//! neighbouring cells is what keeps that from happening, and this is how the
//! result is checked from the outside rather than trusted.
//!
//! The pieces counted here are the ones a walk that pays no attention to the
//! direction of an arc finds, which is what merging along arcs promises. A road
//! network is directed, and on the PTV Europe network one arc in twenty carries
//! no reverse, so a cell that holds together this way can still have border
//! nodes that cannot reach each other without leaving it. That is a stricter
//! property than this checks and than the assembly promises.
//!
//! ```text
//! cargo run --release --example cell_components -- graph.toolbox levels.bin
//! ```
use std::env;
use toolbox_rs::{
    edge::InputEdge, graph::Graph, io, level_directory::LevelDirectory, static_graph,
};

fn main() {
    let args = env::args().collect::<Vec<_>>();
    assert!(args.len() >= 3, "usage: cell_components <graph> <levels>");

    let edges = io::read_vec_from_file::<InputEdge<usize>>(&args[1]);
    // walk the arcs both ways round, as a cell is built out of the cells it has
    // an arc to whichever way that arc runs
    let both_ways = edges
        .iter()
        .flat_map(|edge| {
            [
                InputEdge::new(edge.source, edge.target, 1_usize),
                InputEdge::new(edge.target, edge.source, 1_usize),
            ]
        })
        .collect::<Vec<_>>();
    let graph = static_graph::StaticGraph::new(both_ways);
    let directory: LevelDirectory = io::read_from_file(&args[2]);
    println!(
        "{} nodes, {} arcs, a directory of {} levels over {} nodes",
        graph.number_of_nodes(),
        graph.number_of_edges(),
        directory.levels(),
        directory.number_of_nodes()
    );
    assert_eq!(
        graph.number_of_nodes(),
        directory.number_of_nodes(),
        "the directory was built from another graph"
    );

    for level in 0..directory.levels() {
        let cells = (0..graph.number_of_nodes())
            .map(|node| directory.cell_of(node, level))
            .collect::<Vec<_>>();

        // walk each cell without leaving it and count how many walks it takes
        let mut seen = vec![false; graph.number_of_nodes()];
        let mut stack = Vec::new();
        let count = directory.cells_on_level(level);
        let mut pieces_of = vec![0_usize; count];
        let mut held_by = vec![0_usize; count];
        let mut largest_of = vec![0_usize; count];

        for start in graph.node_range() {
            if seen[start] {
                continue;
            }
            let cell = cells[start] as usize;
            let mut size = 0;
            seen[start] = true;
            stack.push(start);
            while let Some(node) = stack.pop() {
                size += 1;
                for edge in graph.edge_range(node) {
                    let target = graph.target(edge);
                    if !seen[target] && cells[target] as usize == cell {
                        seen[target] = true;
                        stack.push(target);
                    }
                }
            }
            pieces_of[cell] += 1;
            held_by[cell] += size;
            largest_of[cell] = largest_of[cell].max(size);
        }

        let split = pieces_of.iter().filter(|&&pieces| pieces > 1).count();
        let worst = pieces_of.iter().copied().max().unwrap_or(0);
        let stranded: usize = held_by
            .iter()
            .zip(&largest_of)
            .map(|(held, largest)| held - largest)
            .sum();

        let mut sizes = held_by.clone();
        sizes.sort_unstable();
        println!(
            "level {level:>2}: {:>8} cells, sizes {}/{}/{} (smallest/median/largest)",
            sizes.len(),
            sizes.first().copied().unwrap_or(0),
            sizes.get(sizes.len() / 2).copied().unwrap_or(0),
            sizes.last().copied().unwrap_or(0),
        );
        if split == 0 {
            println!("          every cell holds together");
        } else {
            println!(
                "          {split} cells in pieces, worst {worst}, {stranded} nodes outside the largest piece"
            );
        }
    }
}
