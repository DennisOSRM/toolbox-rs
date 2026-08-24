//! What a query costs when nothing has been worked out yet.
//!
//! ```text
//! on_demand <graph> <directory> <source> <target> [queries]
//! ```
//!
//! The overlay is worked out as it is asked for: nothing is customized when
//! the instance is built, and a cell is tabulated the first time a search
//! wants it. So an instance is ready as soon as its files are read, and the
//! cost of customizing is spread over the queries that turn out to need it.
//!
//! What that leaves is a first query that pays for everything it touches, and
//! this says how much: how long the instance takes to build, what the first
//! query costs against the hundredth, and how many cells each one had to work
//! out before it could answer.

use std::{env::args, time::Instant};

use toolbox_rs::{
    customization::Customization, io, level_directory::LevelDirectory, mld_query::MldQuery,
    static_graph::StaticGraph,
};

fn main() {
    env_logger::init();
    let mut argv = args().skip(1);
    let mut next = |what: &str| {
        argv.next().unwrap_or_else(|| {
            panic!(
                "usage: on_demand <graph> <directory> <source> <target> [queries]: missing {what}"
            )
        })
    };
    let graph_path = next("graph");
    let directory_path = next("directory");
    let source: usize = next("source").parse().expect("a source node");
    let target: usize = next("target").parse().expect("a target node");
    let queries: usize = argv
        .next()
        .map_or(100, |count| count.parse().expect("a count"));

    let reading = Instant::now();
    let graph = StaticGraph::new(io::read_edges_from_file(&graph_path));
    let directory: LevelDirectory = io::read_from_file(&directory_path);
    let read = reading.elapsed();

    // what the instance costs to stand up, which is what a startup pays
    let building = Instant::now();
    let customization = Customization::new(graph, directory);
    let built = building.elapsed();
    println!(
        "read the files in {read:.2?}, stood the instance up in {built:.2?}, \
         {} cells customized so far",
        customization.customized_cells()
    );

    // What a level costs to work out, apart from the cells of it.
    //
    // A cell is tabulated on demand, but the cells of a level are not: the
    // first cell of a level anyone asks for works out which cell every node of
    // the graph sits in, which nodes each cell holds, and which of them sit on
    // a border, and the last of those walks every arc there is. So a query
    // that reaches the coarsest level pays that six times over before it
    // tabulates anything at all.
    if std::env::var("TOOLBOX_TIME_LEVELS").is_ok() {
        let mut whole = std::time::Duration::ZERO;
        for level in 0..customization.directory().levels() {
            let started = Instant::now();
            let cells = customization.level(level);
            let elapsed = started.elapsed();
            whole += elapsed;
            println!(
                "  level {level}: {:>8} for {} cells, {} of the nodes on a border",
                format!("{elapsed:.2?}"),
                cells.cells(),
                (0..cells.of_node.len())
                    .filter(|&node| cells.on_border(node))
                    .count()
            );
        }
        println!("  the levels alone: {whole:.2?}");
    }

    let mut query = MldQuery::new();
    let mut before = 0;
    for round in 0..queries {
        query.clear();
        let started = Instant::now();
        let reached = query.run(&customization, source, &[target]);
        let elapsed = started.elapsed();
        let cells = customization.customized_cells();
        // the first few are the ones worth naming, and then every hundredth
        if round < 4 || (round + 1) % 100 == 0 || round + 1 == queries {
            println!(
                "query {:>4}: {:>9} {:>7} cells worked out, {} in all{}",
                round + 1,
                format!("{elapsed:.2?}"),
                cells - before,
                cells,
                if reached { "" } else { ", target not reached" }
            );
        }
        before = cells;
    }
}
