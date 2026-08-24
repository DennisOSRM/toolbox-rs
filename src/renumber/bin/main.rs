//! Numbers the nodes of an instance for the searches that walk its cells.
//!
//! # What it does and why it is its own step
//!
//! A search over the cells of a partition keeps what it knows about a node in
//! an array at that node's number, and reads only the nodes on the borders of
//! coarse cells, a sixth of a continent. The numbers a graph arrives with say
//! nothing about which those are, so the reads land the whole width of arrays
//! of tens and hundreds of megabytes and nearly every one of them misses.
//!
//! [`NodeOrdering`] works out a numbering that puts those nodes first. Doing it
//! here rather than as a query starts up means it is paid once for an instance
//! rather than once per process, and that everything downstream of it -- the
//! graph, the cells, the coordinates -- is written out already agreeing on it.
//! This is the step OSRM does at the end of its partitioner.
//!
//! ```text
//! renumber -g graph.toolbox -d levels.bin -c coordinates.toolbox \
//!          --out-graph graph.renumbered.toolbox \
//!          --out-directory levels.renumbered.bin \
//!          --out-coordinates coordinates.renumbered.toolbox \
//!          --out-ordering ordering.bin
//! ```
//!
//! The numbering is written out with the rest. A caller asks about nodes of the
//! input, so without it the answers cannot be read back.

mod command_line;

use std::{error::Error, time::Instant};

use command_line::Arguments;
use env_logger::{Builder, Env};
use log::info;

use toolbox_rs::{
    edge::InputEdge,
    geometry::FPCoordinate,
    graph::{Graph, NodeID},
    io,
    level_directory::LevelDirectory,
    node_ordering::NodeOrdering,
    packed_partition::PackedPartition,
    static_graph::StaticGraph,
};

fn main() -> Result<(), Box<dyn Error>> {
    Builder::from_env(Env::default().default_filter_or("info")).init();
    let args = <Arguments as clap::Parser>::parse();
    info!("{args}");

    let edges = io::read_edges_from_file(&args.graph);
    info!("loaded {} graph edges", edges.len());
    let graph = StaticGraph::new(edges);
    info!(
        "graph of {} nodes and {} edges",
        graph.number_of_nodes(),
        graph.number_of_edges()
    );

    let directory: LevelDirectory = io::read_from_file(&args.directory);
    info!(
        "loaded a directory of {} levels over {} nodes",
        directory.levels(),
        directory.number_of_nodes()
    );
    if directory.number_of_nodes() != graph.number_of_nodes() {
        return Err("the directory was built over another graph".into());
    }

    let started = Instant::now();
    let ordering = NodeOrdering::in_order(
        &graph,
        &PackedPartition::of(&directory),
        args.numbering.into(),
    );
    info!(
        "numbered {} nodes in {:.1} s, {} of them ({:.1}%) on the border of a cell",
        ordering.len(),
        started.elapsed().as_secs_f64(),
        ordering.on_a_border(),
        100.0 * ordering.on_a_border() as f64 / ordering.len() as f64
    );

    // walked off the graph rather than held beside it: a continent is forty
    // odd million arcs, and a second copy of them costs more than the one that
    // is wanted
    let mut renumbered = Vec::with_capacity(graph.number_of_edges());
    for source in graph.node_range() {
        let moved = ordering.new_of(source);
        for edge in graph.edge_range(source) {
            renumbered.push(InputEdge::new(
                moved,
                ordering.new_of(graph.target(edge)),
                *graph.data(edge),
            ));
        }
    }
    drop(graph);
    io::write_vec_to_file(&args.out_graph, &renumbered);
    info!("wrote {} edges to {}", renumbered.len(), args.out_graph);
    drop(renumbered);

    let moved = ordering.renumber_directory(&directory);
    io::write_to_file(&args.out_directory, &moved);
    info!("wrote the directory to {}", args.out_directory);
    drop(moved);
    drop(directory);

    // A coordinate belongs to the node it is held at, so it moves with it. An
    // instance without them is renumbered all the same; there is simply
    // nothing to move.
    match (args.coordinates.is_empty(), args.out_coordinates.is_empty()) {
        (true, _) => info!("no coordinates were given, so none were moved"),
        (false, true) => return Err("coordinates were given with nowhere to write them".into()),
        (false, false) => {
            let coordinates = io::read_vec_from_file::<FPCoordinate>(&args.coordinates);
            if coordinates.len() != ordering.len() {
                return Err(format!(
                    "{} coordinates against {} nodes, so they are not this graph's",
                    coordinates.len(),
                    ordering.len()
                )
                .into());
            }
            let mut moved = vec![FPCoordinate::new(0, 0); coordinates.len()];
            for (node, coordinate) in coordinates.into_iter().enumerate() {
                moved[ordering.new_of(node as NodeID) as usize] = coordinate;
            }
            io::write_vec_to_file(&args.out_coordinates, &moved);
            info!(
                "wrote {} coordinates to {}",
                moved.len(),
                args.out_coordinates
            );
        }
    }

    io::write_to_file(&args.out_ordering, &ordering);
    info!("wrote the numbering to {}", args.out_ordering);
    Ok(())
}
