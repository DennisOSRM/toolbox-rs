use std::{fs::File, io::BufWriter};

use geojson::{feature::Id, Feature, FeatureWriter, Geometry, Value};
use itertools::Itertools;
use toolbox_rs::{
    bounding_box::BoundingBox, geometry::primitives::FPCoordinate, partition::PartitionID,
};

pub(crate) fn convex_cell_hull_geojson(
    hulls: &[(Vec<FPCoordinate>, BoundingBox, &PartitionID)],
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
            .unwrap_or_else(|_| panic!("error writing feature: {id}"));
    }
    writer.finish().expect("error writing file");
}

pub(crate) fn boundary_geometry_geojson(coordinates: &[FPCoordinate], filename: &str) {
    let file = BufWriter::new(File::create(filename).expect("output file cannot be opened"));
    let mut writer = FeatureWriter::from_writer(file);
    for coordinate in coordinates {
        // serialize convex hull polygons as geojson
        let geometry = Geometry::new(Value::Point(coordinate.to_lon_lat_vec()));

        writer
            .write_feature(&Feature {
                bbox: None,
                geometry: Some(geometry),
                id: None,
                // Features tbd
                properties: None,
                foreign_members: None,
            })
            .unwrap_or_else(|_| panic!("error writing feature: {coordinate}"));
    }
    writer.finish().expect("error writing file");
}
