//! Which level the query steps over each node it settles, and whether it ever
//! goes finer than it had to.
//!
//! A search over the cells is meant to hold the coarsest level it can and
//! descend only where it must: inside the cell holding the source and inside
//! the cell holding the target. Every level of a partition is sound to step
//! over, so a query that descended early would give exactly the same distances
//! and no test of those distances would say a word. It would simply do more
//! work. This is the check that says otherwise.
use std::env::args;

use toolbox_rs::{
    customization::Customization, graph::NodeID, heap_stats::SettledNodes, io,
    level_directory::LevelDirectory, mld_query::MldSearch, static_graph::StaticGraph,
};

fn main() {
    let mut argv = args().skip(1);
    let graph_path = argv
        .next()
        .expect("usage: query_levels <graph> <directory> <pairs.csv>");
    let directory_path = argv
        .next()
        .expect("usage: query_levels <graph> <directory> <pairs.csv>");
    let pairs_path = argv
        .next()
        .expect("usage: query_levels <graph> <directory> <pairs.csv>");

    let edges = io::read_edges_from_file(&graph_path);
    let graph = StaticGraph::new(edges);
    let directory: LevelDirectory = io::read_from_file(&directory_path);
    let levels = directory.levels();
    let top = levels - 1;
    println!("{levels} levels over {} nodes", directory.number_of_nodes());

    let pairs: Vec<(NodeID, NodeID)> = std::fs::read_to_string(&pairs_path)
        .expect("the pairs cannot be read")
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut field = line.split(',');
            let source = field.next()?.trim().parse().ok()?;
            let target = field.next()?.trim().parse().ok()?;
            Some((source, target))
        })
        .collect();
    println!("{} pairs", pairs.len());

    let customization = Customization::new(graph, directory);
    let directory = customization.directory();
    let mut query = MldSearch::<SettledNodes>::new();

    // how many settled nodes were stepped over at each level, and how many at
    // no level at all because even the finest cell held an end
    let mut at_level = vec![0_u64; levels + 1];
    let mut descended_outside = 0_u64;
    let mut settled_total = 0_u64;

    for &(source, target) in &pairs {
        query.run(&customization, source, &[target]);
        let source_top = directory.cell_of(source, top);
        let target_top = directory.cell_of(target, top);

        for &node in query.stats().settled() {
            settled_total += 1;
            // the highest level whose cell holds neither end, worked out here
            // rather than asked of the query, so that this checks the rule and
            // not the code that implements it
            let used = (0..levels).rev().find(|&level| {
                let cell = directory.cell_of(node, level);
                cell != directory.cell_of(source, level) && cell != directory.cell_of(target, level)
            });
            at_level[used.map_or(levels, |level| level)] += 1;

            if used != Some(top) {
                // it went finer than the top, so it had better be inside one
                // of the two cells that force it to
                let cell = directory.cell_of(node, top);
                if cell != source_top && cell != target_top {
                    descended_outside += 1;
                }
            }
        }
    }

    println!("\n{settled_total} nodes settled over all pairs");
    for (level, &count) in at_level.iter().take(levels).enumerate() {
        let share = 100.0 * count as f64 / settled_total as f64;
        println!("  stepped over at level {level}: {count:>10} ({share:5.1}%)");
    }
    println!(
        "  no level to step over:     {:>10} ({:5.1}%)",
        at_level[levels],
        100.0 * at_level[levels] as f64 / settled_total as f64
    );

    println!(
        "\nsettled below the top level while outside both ends' top cells: {descended_outside}"
    );
    assert_eq!(
        descended_outside, 0,
        "the query went finer than the top level somewhere it did not have to"
    );
    println!("the query holds the top level everywhere outside the two cells that force it down");
}
