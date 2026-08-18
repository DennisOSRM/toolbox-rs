//! What the assembled levels actually look like, cell by cell.
//!
//! The query pays for border nodes: stepping over a cell walks its clique, so
//! one entry costs as many relaxations as the cell has border nodes. Which
//! cells get entered is not uniform — a cell is reached in rough proportion to
//! how much boundary it has — so what an entry costs on average is the
//! size-biased mean of the boundary, `E[B^2]/E[B]`, and not `E[B]`. The gap
//! between those two numbers is what a skewed assembly costs.
use std::env::args;

use toolbox_rs::{edge::InputEdge, io, level_directory::LevelDirectory};

fn main() {
    let mut argv = args().skip(1);
    let graph_path = argv.next().expect("usage: cell_stats <graph> <directory>");
    let directory_path = argv.next().expect("usage: cell_stats <graph> <directory>");

    let edges = io::read_vec_from_file::<InputEdge<usize>>(&graph_path);
    let directory: LevelDirectory = io::read_from_file(&directory_path);
    println!(
        "{} arcs, {} nodes, {} levels",
        edges.len(),
        directory.number_of_nodes(),
        directory.levels()
    );

    let nodes = directory.number_of_nodes();
    for level in 0..directory.levels() {
        let cells = directory.cells_on_level(level);
        let of_node: Vec<u32> = (0..nodes)
            .map(|node| directory.cell_of(node, level) as u32)
            .collect();

        let mut size = vec![0_u32; cells];
        for &cell in &of_node {
            size[cell as usize] += 1;
        }

        // a node is on the border of its cell while an arc leaves it or
        // reaches it, which is the definition the overlay is built on
        let mut on_border = vec![false; nodes];
        for edge in &edges {
            if of_node[edge.source] != of_node[edge.target] {
                on_border[edge.source] = true;
                on_border[edge.target] = true;
            }
        }
        let mut border = vec![0_u32; cells];
        for (node, &is_border) in on_border.iter().enumerate() {
            if is_border {
                border[of_node[node] as usize] += 1;
            }
        }

        let total: u64 = border.iter().map(|&b| u64::from(b)).sum();
        let squared: u64 = border.iter().map(|&b| u64::from(b) * u64::from(b)).sum();
        let mut sorted = border.clone();
        sorted.sort_unstable();
        let median = sorted[sorted.len() / 2];
        let mean = total as f64 / cells as f64;
        // what one entry into a cell of this level costs on average, given
        // that a cell is entered in proportion to its boundary
        let biased = squared as f64 / total as f64;

        let mut node_sizes = size.clone();
        node_sizes.sort_unstable();

        println!(
            "level {level}: {cells} cells, {} nodes each on average (median {}, max {})",
            nodes / cells,
            node_sizes[node_sizes.len() / 2],
            node_sizes[node_sizes.len() - 1],
        );
        println!(
            "          border: {total} in total, mean {mean:.1}, median {median}, max {}",
            sorted[sorted.len() - 1],
        );
        println!(
            "          an entry walks {biased:.1} on average, {:.2}x the mean; \
             balancing would save {:.0}%",
            biased / mean,
            100.0 * (1.0 - mean / biased),
        );
    }
}
