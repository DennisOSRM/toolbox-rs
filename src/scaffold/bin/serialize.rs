use std::{fs::File, io::BufWriter};

use geojson::{Feature, FeatureWriter, Geometry, GeometryValue, feature::Id};
use itertools::Itertools;
use toolbox_rs::{bounding_box::BoundingBox, geometry::FPCoordinate, partition_id::PartitionID};

/// The ten colours a cell is drawn in, the same pastels the tile server uses.
/// They are pastels on purpose: a cell covers a good part of the picture, and
/// ten saturated colours next to each other fight rather than tell a cell from
/// its neighbour.
const PALETTE: [[i32; 3]; 10] = [
    [244, 164, 164],
    [164, 214, 164],
    [250, 226, 156],
    [164, 186, 232],
    [246, 196, 150],
    [206, 168, 224],
    [160, 222, 222],
    [240, 176, 224],
    [214, 232, 160],
    [200, 200, 214],
];

/// How many cuts up the cell a colour is taken from lies. Each cut halves a
/// cell, so six of them is a cell sixty four times the size: large enough to
/// read as a region, small enough that a picture holds several of them.
const CUTS_UP: usize = 6;

/// How far a cell may stray from the colour of the cell it lies in, per shade.
/// Small on purpose: a cell should read as one of its parent's rather than as a
/// colour of its own, so this lightens or darkens and leaves the hue alone.
const SHADE_STEP: i32 = 11;

/// The colour of a cell: the colour of the cell a few cuts above it, lightened
/// or darkened by which of five shades this one falls into. Neighbouring cells
/// mostly share an ancestor, so a region reads as one colour and the cells
/// inside it as its shades.
fn colour_of(id: &PartitionID) -> String {
    let mut family = *id;
    for _ in 0..CUTS_UP {
        family = family.parent();
    }
    let base = PALETTE[family.0 as usize % PALETTE.len()];
    let shade = (id.0 as i32 % 5 - 2) * SHADE_STEP;
    let channel = |value: i32| (value + shade).clamp(0, 255);
    format!(
        "#{:02x}{:02x}{:02x}",
        channel(base[0]),
        channel(base[1]),
        channel(base[2])
    )
}

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
                geojson::Position::from(c.to_lon_lat_vec())
            })
            .collect_vec();

        // serialize convex hull polygons as geojson
        let geometry = Geometry::new(GeometryValue::Polygon {
            coordinates: vec![convex_hull],
        });

        // the properties a geojson viewer reads a colour off, so that a file
        // dropped into one is coloured the way the tile server colours a cell
        let mut properties = geojson::JsonObject::new();
        let colour = colour_of(id);
        properties.insert("fill".to_string(), colour.clone().into());
        properties.insert("fill-opacity".to_string(), 0.6.into());
        properties.insert("stroke".to_string(), colour.into());
        properties.insert("stroke-width".to_string(), 1.into());
        properties.insert("cell".to_string(), id.to_string().into());

        writer
            .write_feature(&Feature {
                bbox: Some(bbox.into()),
                geometry: Some(geometry),
                id: Some(Id::String(id.to_string())),
                properties: Some(properties),
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
        let geometry = Geometry::new(GeometryValue::Point {
            coordinates: geojson::Position::from(coordinate.to_lon_lat_vec()),
        });

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

#[cfg(test)]
mod tests {
    use super::*;

    /// A cell six cuts below the one it takes its colour from.
    fn under(family: PartitionID, path: &[bool]) -> PartitionID {
        let mut id = family;
        for &right in path {
            id = if right {
                id.right_child()
            } else {
                id.left_child()
            };
        }
        id
    }

    fn channels(colour: &str) -> [i32; 3] {
        let hex = colour.trim_start_matches('#');
        [0, 2, 4].map(|at| i32::from_str_radix(&hex[at..at + 2], 16).expect("not a colour"))
    }

    #[test]
    fn a_colour_is_a_hex_triplet() {
        let colour = colour_of(&PartitionID::root().left_child());
        assert_eq!(colour.len(), 7, "{colour}");
        assert!(colour.starts_with('#'));
        for channel in channels(&colour) {
            assert!((0..=255).contains(&channel), "{colour}");
        }
    }

    /// The point of the shading: two cells of one family are the same colour
    /// give or take a shade, rather than two colours out of the palette.
    #[test]
    fn cells_of_one_family_keep_close_together() {
        let family = PartitionID::root().left_child().right_child();
        let siblings = [
            under(family, &[false, false, false, false, false, false]),
            under(family, &[false, false, false, false, false, true]),
            under(family, &[true, false, true, false, true, false]),
        ];

        let colours = siblings.map(|cell| channels(&colour_of(&cell)));
        for pair in colours.windows(2) {
            for (channel, (one, other)) in pair[0].iter().zip(&pair[1]).enumerate() {
                let apart = (one - other).abs();
                assert!(
                    apart <= 4 * SHADE_STEP,
                    "two cells of one family are {apart} apart on channel {channel}"
                );
            }
        }
    }

    /// And a shade is a shade: it moves every channel the same way, which
    /// lightens or darkens without turning the colour into another one.
    #[test]
    fn a_shade_moves_the_channels_together() {
        let family = PartitionID::root().left_child();
        let one = under(family, &[false; 6]);
        let other = under(family, &[false, false, false, false, false, true]);

        let (a, b) = (channels(&colour_of(&one)), channels(&colour_of(&other)));
        let moved = a[0] - b[0];
        for channel in 1..3 {
            // a channel that ran into the end of the range moves less, so this
            // only asks that none of them moved the other way
            assert!(
                (a[channel] - b[channel]).signum() * moved.signum() >= 0,
                "channel {channel} moved against the others"
            );
        }
    }
}
