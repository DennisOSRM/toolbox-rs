mod command_line;
mod serialize;

use command_line::Arguments;
use env_logger::{Builder, Env};
use fxhash::{FxHashMap, FxHashSet};
use itertools::Itertools;
use log::info;
use rayon::prelude::*;
use toolbox_rs::{
    bounding_box::BoundingBox, convex_hull::monotone_chain, edge::InputEdge,
    geometry::FPCoordinate, io, partition::PartitionID, space_filling_curve::zorder_cmp,
};

// TODO: tool to generate all the runtime data

pub fn main() {
    Builder::from_env(Env::default().default_filter_or("info")).init();

    println!(r#"   ___                       __      __             _        _ "#);
    println!(r#"  / __|    __     __ _      / _|    / _|   ___     | |    __| |"#);
    println!(r#"  \__ \   / _|   / _` |    |  _|   |  _|  / _ \    | |   / _` |"#);
    println!(r#"  |___/   \__|_  \__,_|   _|_|_   _|_|_   \___/   _|_|_  \__,_|"#);
    println!(r#"_|"""""|_|"""""|_|"""""|_|"""""|_|"""""|_|"""""|_|"""""|_|"""""|"#);
    println!(r#""`-0-0-'"`-0-0-'"`-0-0-'"`-0-0-'"`-0-0-'"`-0-0-'"`-0-0-'"`-0-0-'"#);
    println!("build: {}", env!("GIT_HASH"));
    // parse and print command line parameters
    let args = <Arguments as clap::Parser>::parse();
    info!("{args}");

    let partition_ids = io::read_vec_from_file::<PartitionID>(&args.partition_file);
    info!("loaded {} partition ids", partition_ids.len());

    let coordinates = io::read_vec_from_file::<FPCoordinate>(&args.coordinates_file);
    info!("loaded {} coordinates", coordinates.len());

    let edges = io::read_vec_from_file::<InputEdge<usize>>(&args.graph);
    info!("loaded {} edges", edges.len());

    info!("creating and sorting proxy vector");
    let mut known_ids = FxHashSet::default();
    let mut proxy_vector = Vec::new();
    for (i, partition_id) in partition_ids.iter().enumerate() {
        if !known_ids.contains(partition_id) {
            proxy_vector.push(i);
            known_ids.insert(partition_id);
        }
    }

    proxy_vector.sort();
    info!("number of unique cell ids is {}", proxy_vector.len());

    if !args.convex_cells_geojson.is_empty() {
        info!("generating convex hulls");
        let mut cells: FxHashMap<PartitionID, Vec<usize>> = FxHashMap::default();
        for (i, partition_id) in partition_ids.iter().enumerate() {
            if !cells.contains_key(partition_id) {
                cells.insert(*partition_id, Vec::new());
            }
            cells.get_mut(partition_id).unwrap().push(i);
        }
        let mut hulls: Vec<_> = cells
            .par_iter()
            .map(|(id, indexes)| {
                let cell_coordinates = indexes.iter().map(|i| coordinates[*i]).collect_vec();
                let convex_hull = monotone_chain(&cell_coordinates);
                let bbox = BoundingBox::from_coordinates(&convex_hull);

                (convex_hull, bbox, id)
            })
            .collect();

        info!("sorting convex cell hulls by Z-order");
        hulls.sort_by(|a, b| zorder_cmp(&a.1.center(), &b.1.center()));
        info!("writing to {}", &args.convex_cells_geojson);
        serialize::convex_cell_hull_geojson(&hulls, &args.convex_cells_geojson);
    }

    if !args.boundary_nodes_geojson.is_empty() {
        info!("computing geometry of boundary nodes");
        let boundary_coordinates = edges
            .iter()
            .filter(|edge| partition_ids[edge.source] != partition_ids[edge.target])
            .map(|edge| coordinates[edge.source])
            .collect_vec();
        info!("detection {} boundary nodes", boundary_coordinates.len());

        serialize::boundary_geometry_geojson(&boundary_coordinates, &args.boundary_nodes_geojson);
    }
}
