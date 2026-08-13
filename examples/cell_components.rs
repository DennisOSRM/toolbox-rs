//! Counts the connected components of every cell of a partition. A cell that
//! falls apart into several of them cannot be crossed without leaving it, which
//! is what the distance table of a cell assumes it can do.
use std::env;
use toolbox_rs::{
    edge::InputEdge,
    graph::Graph,
    io,
    partition_id::PartitionID,
    static_graph::{self, StaticGraph},
};

/// The cell of a leaf at the given level, i.e. the id with the lower levels
/// shifted out.
fn cell_at_level(cell: PartitionID, level: u32) -> PartitionID {
    let Some(steps) = u32::from(cell.level()).checked_sub(level) else {
        return cell;
    };
    PartitionID::new((cell.0 >> steps).max(1))
}

fn main() {
    let args = env::args().collect::<Vec<_>>();
    let (graph_path, assignment_path) = (&args[1], &args[2]);

    let edges = io::read_vec_from_file::<InputEdge<usize>>(graph_path);
    let partition_ids = io::read_vec_from_file::<PartitionID>(assignment_path);
    let graph: StaticGraph<usize> = static_graph::StaticGraph::new(edges);
    println!(
        "{} nodes, {} arcs, {} partition ids",
        graph.number_of_nodes(),
        graph.number_of_edges(),
        partition_ids.len()
    );

    let max_level = partition_ids
        .iter()
        .map(|cell| u32::from(cell.level()))
        .max()
        .unwrap();
    println!("levels: {max_level}");

    // the sizes of the leaf cells say whether the cuts came out balanced
    let mut size_of: std::collections::HashMap<PartitionID, usize> =
        std::collections::HashMap::new();
    for cell in &partition_ids {
        *size_of.entry(*cell).or_insert(0) += 1;
    }
    let mut sizes = size_of.values().copied().collect::<Vec<_>>();
    sizes.sort_unstable();
    let tiny = sizes.iter().filter(|&&size| size < 10).count();
    let under_minimum = sizes.iter().filter(|&&size| size < 100).count();
    println!(
        "leaf cells: {}, smallest {}, median {}, largest {}",
        sizes.len(),
        sizes[0],
        sizes[sizes.len() / 2],
        sizes[sizes.len() - 1]
    );
    println!(
        "            {tiny} cells under 10 nodes, {under_minimum} under the minimum cell size of 100"
    );

    for level in (2..=max_level).rev().step_by(4).chain(std::iter::once(1)) {
        let cells = partition_ids
            .iter()
            .map(|cell| cell_at_level(*cell, level))
            .collect::<Vec<_>>();

        // walk each cell, staying inside it, and count how many walks it takes
        let mut singletons = 0;
        let mut isolated_in_graph = 0;
        let mut seen = vec![false; graph.number_of_nodes()];
        let mut stack = Vec::new();
        let mut components_of: std::collections::HashMap<PartitionID, (usize, usize, usize)> =
            std::collections::HashMap::new();

        for start in graph.node_range() {
            if seen[start] {
                continue;
            }
            let cell = cells[start];
            let mut size = 0;
            seen[start] = true;
            stack.push(start);
            while let Some(node) = stack.pop() {
                size += 1;
                for edge in graph.edge_range(node) {
                    let target = graph.target(edge);
                    if !seen[target] && cells[target] == cell {
                        seen[target] = true;
                        stack.push(target);
                    }
                }
            }
            if size == 1 {
                singletons += 1;
                // a piece of one node either has no arc at all in the graph, or
                // every arc of it leaves the cell it was put into
                if graph.out_degree(start) == 0 {
                    isolated_in_graph += 1;
                }
            }
            let entry = components_of.entry(cell).or_insert((0, 0, 0));
            entry.0 += 1; // components
            entry.1 += size; // nodes
            entry.2 = entry.2.max(size); // largest component
        }

        let total = components_of.len();
        let split = components_of
            .values()
            .filter(|(components, _, _)| *components > 1)
            .count();
        let worst = components_of
            .values()
            .map(|(components, _, _)| *components)
            .max()
            .unwrap_or(0);
        // how much of a split cell lies outside of its largest component
        let stranded: usize = components_of
            .values()
            .map(|(_, nodes, largest)| nodes - largest)
            .sum();

        println!(
            "level {level:>2}: {total:>7} cells, {split:>7} of them in pieces ({:>5.1}%), \
             worst {worst:>5} pieces, {stranded:>8} nodes outside the largest piece",
            100. * split as f64 / total as f64
        );
        println!(
            "          {singletons:>8} pieces of a single node, {isolated_in_graph} of which have no arc in the graph at all"
        );
    }
}
