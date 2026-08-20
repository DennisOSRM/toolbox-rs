//! What an alpha shape makes of the cells of a partition, against what a
//! convex hull makes of them.
//!
//! ```text
//! cargo run --release --example alpha_cells -- coordinates.toolbox levels.bin <level> <alpha in metres>
//! ```
use std::env;
use toolbox_rs::{
    alpha_shape::alpha_shape, convex_hull::monotone_chain, geometry::Point2D, graph::NodeID, io,
    level_directory::LevelDirectory,
};

fn main() {
    let args = env::args().collect::<Vec<_>>();
    assert!(
        args.len() >= 4,
        "usage: alpha_cells <coordinates> <levels> <level> [alpha]"
    );
    let coordinates = io::read_vec_from_file::<toolbox_rs::geometry::FPCoordinate>(&args[1]);
    let directory: LevelDirectory = io::read_from_file(&args[2]);
    let level = args[3].parse::<usize>().expect("level is a number");
    let alpha = args
        .get(4)
        .map_or(500.0, |a| a.parse::<f64>().expect("alpha is a number"));

    let mut nodes_of_cell = vec![Vec::new(); directory.cells_on_level(level)];
    for node in 0..directory.number_of_nodes() {
        nodes_of_cell[directory.cell_of(node as NodeID, level) as usize].push(node);
    }

    // metres to degrees of latitude, near enough over a cell
    let alpha_in_degrees = alpha / 111_320.0;
    let (mut hull_corners, mut shape_corners, mut in_pieces, mut done) = (0, 0, 0, 0);
    for nodes in nodes_of_cell
        .iter()
        .filter(|nodes| nodes.len() >= 3)
        .take(2000)
    {
        let points = nodes
            .iter()
            .map(|&node| Point2D {
                x: coordinates[node].to_lon_lat_pair().0,
                y: coordinates[node].to_lon_lat_pair().1,
            })
            .collect::<Vec<_>>();
        let hull = monotone_chain(&nodes.iter().map(|&n| coordinates[n]).collect::<Vec<_>>());
        let rings = alpha_shape(&points, alpha_in_degrees);

        hull_corners += hull.len();
        shape_corners += rings.iter().map(Vec::len).sum::<usize>();
        in_pieces += usize::from(rings.len() > 1);
        done += 1;
    }

    println!("level {level}, alpha {alpha} m, over {done} cells");
    println!("  convex hulls: {hull_corners} corners in all");
    println!(
        "  alpha shapes: {shape_corners} corners in all, {in_pieces} cells in more than one piece"
    );
}
