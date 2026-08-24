//! Two things a store would like to be true of a numbering, asked rather than
//! assumed.
//!
//! ```text
//! layout_check <graph> <directory>
//! ```
//!
//! Under [`Numbering::CellPath`] every cell is one run of numbers. Two further
//! things follow if the run is laid out the way it looks like it is, and each
//! is worth a lot to a store:
//!
//! **The nodes of a cell are the run itself.** If the numbers in a cell are
//! exactly the numbers of the run, then nothing has to say which nodes a cell
//! holds: the range says it. A store keeps a first and a count instead of a
//! list, and so does the cell tree, whose list is a number per node per level.
//!
//! **The border nodes lead the run.** A table is addressed by where a node
//! sits in it, so a store has to be able to turn a place into a node. If the
//! border nodes of a cell are the first of its run, that is an addition, and
//! the list of border nodes -- four bytes for every one of them, at every
//! level -- is not needed at all.
//!
//! Neither is true under the other numbering, and neither is guaranteed by
//! contiguity alone, so this checks both.
//!
//! [`Numbering::CellPath`]: toolbox_rs::node_ordering::Numbering::CellPath

use std::env::args;

use toolbox_rs::{
    customization::Customization, io, level_directory::LevelDirectory, static_graph::StaticGraph,
};

fn directory_of(customization: &Customization) -> &LevelDirectory {
    customization.directory()
}

fn main() {
    env_logger::init();
    let mut argv = args().skip(1);
    let mut next = |what: &str| {
        argv.next()
            .unwrap_or_else(|| panic!("usage: layout_check <graph> <directory>: missing {what}"))
    };
    let graph = StaticGraph::new(io::read_edges_from_file(&next("graph")));
    let directory: LevelDirectory = io::read_from_file(&next("directory"));
    let levels = directory.levels();
    let customization = Customization::new(graph, directory);

    println!(
        "{:>5} {:>9} {:>16} {:>16} {:>12}",
        "level", "cells", "a run of numbers", "border in front", "border nodes"
    );
    let mut all_runs = true;
    let mut all_in_front = true;
    for level in 0..levels {
        let holding = customization.level(level);
        let cells = holding.cells();
        let mut runs = 0_usize;
        let mut in_front = 0_usize;
        let mut border_nodes = 0_u64;
        for cell in 0..cells {
            let nodes = holding.nodes_of(cell as u32);
            if nodes.is_empty() {
                runs += 1;
                in_front += 1;
                continue;
            }
            // the run itself: every number from the first to the last, in order
            let first = nodes[0];
            if nodes
                .iter()
                .enumerate()
                .all(|(at, &node)| node == first + at)
            {
                runs += 1;
            }
            // and the ones on the border are the front of it
            let border = nodes
                .iter()
                .filter(|&&node| holding.on_border(node))
                .count();
            border_nodes += border as u64;
            if nodes[..border].iter().all(|&node| holding.on_border(node))
                && nodes[border..].iter().all(|&node| !holding.on_border(node))
            {
                in_front += 1;
            }
        }
        all_runs &= runs == cells;
        all_in_front &= in_front == cells;
        println!(
            "{level:>5} {cells:>9} {:>16} {:>16} {border_nodes:>12}",
            format!("{runs} ({:.1}%)", 100.0 * runs as f64 / cells.max(1) as f64),
            format!(
                "{in_front} ({:.1}%)",
                100.0 * in_front as f64 / cells.max(1) as f64
            ),
        );
    }

    // and whether the cells of a level are laid out under their parents, which
    // is what makes a cell's children a run rather than a list
    let mut children_run = true;
    for level in 1..levels {
        let above = directory_of(&customization)
            .parents_on_level(level - 1)
            .to_vec();
        children_run &= above.windows(2).all(|pair| pair[0] <= pair[1]);
    }

    println!();
    println!(
        "the children of a cell are a run:       {}",
        if children_run {
            "yes, everywhere"
        } else {
            "NO"
        }
    );
    println!(
        "the nodes of a cell are the run itself: {}",
        if all_runs { "yes, everywhere" } else { "NO" }
    );
    println!(
        "the border nodes lead the run:          {}",
        if all_in_front {
            "yes, everywhere"
        } else {
            "NO"
        }
    );
    if all_runs && all_in_front {
        println!(
            "\nso a cell wants a first and a count, and a place in a table is a\n\
             node without being told: nothing has to store which nodes a cell\n\
             holds, nor which of them are on its border."
        );
    }
}
