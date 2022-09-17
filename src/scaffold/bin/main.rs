mod command_line;
mod deserialize;

use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::BufWriter,
};

use command_line::Arguments;
use env_logger::{Builder, Env};
use geojson::{feature::Id, Feature, FeatureWriter, Geometry, Value};
use itertools::Itertools;
use log::info;
use toolbox_rs::{
    bounding_box::BoundingBox, convex_hull::monotone_chain, io, partition::PartitionID,
    space_filling_curve::zorder_cmp,
};

use crate::deserialize::binary_partition_file;

// TODO: tool that generate all the runtime data

pub fn main() {
    Builder::from_env(Env::default().default_filter_or("info")).init();

    println!(r#"   ___                       __      __             _        _ "#);
    println!(r#"  / __|    __     __ _      / _|    / _|   ___     | |    __| |"#);
    println!(r#"  \__ \   / _|   / _` |    |  _|   |  _|  / _ \    | |   / _` |"#);
    println!(r#"  |___/   \__|_  \__,_|   _|_|_   _|_|_   \___/   _|_|_  \__,_|"#);
    println!(r#"_|"""""|_|"""""|_|"""""|_|"""""|_|"""""|_|"""""|_|"""""|_|"""""|"#);
    println!(r#""`-0-0-'"`-0-0-'"`-0-0-'"`-0-0-'"`-0-0-'"`-0-0-'"`-0-0-'"`-0-0-'"#);

    // parse and print command line parameters
    let args = <Arguments as clap::Parser>::parse();
    info!("{args}");

    let partition_ids = binary_partition_file(&args.partition_file);
    info!("loaded {} partitions", partition_ids.len());

    let coordinates = io::read_coordinates(&args.coordinates_file);
    info!("loaded {} coordinates", coordinates.len());

    info!("creating and sorting proxy vector");
    let mut known_ids = HashSet::new();
    let mut proxy_vector = Vec::new();
    for (i, partition_id) in partition_ids.iter().enumerate() {
        if !known_ids.contains(partition_id) {
            proxy_vector.push(i);
            known_ids.insert(partition_id);
        }
    }

    info!("number of unique cell ids is {}", proxy_vector.len());

    let mut cells: HashMap<PartitionID, Vec<usize>> = HashMap::new();
    for (i, partition_id) in partition_ids.iter().enumerate() {
        if !cells.contains_key(partition_id) {
            cells.insert(*partition_id, Vec::new());
        }
        cells.get_mut(partition_id).unwrap().push(i);
    }
    let mut hulls = cells
        .iter()
        .map(|(id, indexes)| {
            let cell_coordinates = indexes.iter().map(|i| coordinates[*i]).collect_vec();
            let convex_hull = monotone_chain(&cell_coordinates);
            let bbox = BoundingBox::from_coordinates(&convex_hull);

            (convex_hull, bbox, id)
        })
        .collect_vec();

    if !args.convex_cells_geojson.is_empty() {
        info!("sorting convex cell hulls by Z-order");
        hulls.sort_by(|a, b| zorder_cmp(a.1.center(), b.1.center()));
        info!("writing to {}", &args.convex_cells_geojson);
        serialize_convex_cell_hull_geojson(&hulls, &args.convex_cells_geojson);
    }

    // generate bounding boxes
    //  - serialize list of boxes as geojson
    // sort vector of boxes along space filling curve
    // construct
    // sort proxy vector
    // remove duplicates from proxy vector
    // copy unique ids and their rank to
    info!("done.");
}

fn serialize_convex_cell_hull_geojson(
    hulls: &[(
        Vec<toolbox_rs::geometry::primitives::FPCoordinate>,
        BoundingBox,
        &PartitionID,
    )],
    filename: &str,
) {
    let file = BufWriter::new(File::create(filename).expect("output file cannot be opened"));
    let mut writer = FeatureWriter::from_writer(file);
    for (convex_hull, bbox, id) in hulls {
        // map n + 1 points of the closed polygon into a format that is geojson compliant
        let convex_hull = convex_hull
            .iter()
            .cycle()
            .take(convex_hull.len() + 1)
            .map(|c| {
                // TODO: should this be implemented via the Into<> trait?
                c.to_lon_lat_vec()
            })
            .collect_vec();

        // serialize convex hull polygons as geojson
        let geometry = Geometry::new(Value::Polygon(vec![convex_hull]));

        writer
            .write_feature(&Feature {
                bbox: Some(bbox.into()),
                geometry: Some(geometry),
                id: Some(Id::String(id.to_string())),
                // Features tbd
                properties: None,
                foreign_members: None,
            })
            .unwrap_or_else(|_| panic!("error writing feature: {}", id));
    }
    writer.finish().expect("error writing file");
}
