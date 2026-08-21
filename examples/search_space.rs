//! Writes out what a search over the cells looked at, for looking at.
//!
//! ```text
//! search_space <graph> <directory> <coordinates> <source> <target> <out.geojson>
//! ```
//!
//! # What is drawn, and why in three dimensions
//!
//! A search over the cells is a thing of levels: it steps over the coarsest
//! cell it may, drops to a finer one as it nears an end, and reaches the arcs
//! of the graph only inside the two cells holding the ends. Drawn flat, all of
//! that lands on top of itself and the picture says nothing about which level
//! anything happened at, which is the one thing worth seeing.
//!
//! So each level is given a height. The arcs of the graph lie on the ground,
//! the cells of the finest level float above them, and each level above that
//! floats higher again. A step across a cell is drawn at the height of the
//! level it was taken at, and under each of its ends a column runs down to the
//! ground, so the way the search took can be followed from the coarse step it
//! made down to the arcs that step really stands for.
//!
//! # What goes in the file
//!
//! One feature collection, every feature carrying `kind`, `level`, and the two
//! heights it is to be drawn between. The heights are worked out here rather
//! than in the viewer, so that the viewer only has to draw what it is given.
//!
//! | `kind`     | what it is                                        |
//! |------------|---------------------------------------------------|
//! | `cell`     | the outline of a cell the search stepped over     |
//! | `settled`  | a node the search took off its queue              |
//! | `packed`   | a step of the way the search found                |
//! | `unpacked` | the way through the graph that step stands for    |
//! | `column`   | the drop from a packed node to the ground         |

use std::{collections::BTreeSet, env::args, fs::File, io::BufWriter};

use geojson::{Feature, FeatureWriter, Geometry, GeometryValue, JsonObject, JsonValue, Position};
use rustc_hash::FxHashMap;

use toolbox_rs::{
    convex_hull::monotone_chain,
    customization::Customization,
    geometry::FPCoordinate,
    graph::NodeID,
    heap_stats::SettledNodes,
    io,
    level_directory::{CellId, LevelDirectory},
    mld_query::MldSearch,
    path_unpacking::{cost_of_way, unpack},
    static_graph::StaticGraph,
};

/// How high a level floats above the one below it, in metres.
///
/// Large enough that the levels do not run into one another when the whole of
/// a continent is on screen, which is the view this is drawn for.
const LEVEL_HEIGHT: f64 = 60_000.0;

/// How thick a floating sheet is drawn, so that it reads as a surface rather
/// than as nothing at all.
const SHEET: f64 = 4_000.0;

/// A cell of more nodes than this has its outline taken from a sample of them.
/// A hull is decided by the points on the outside, and a coarse cell holds
/// millions of nodes that are nowhere near it.
const HULL_SAMPLE: usize = 200_000;

fn main() {
    let mut argv = args().skip(1);
    let mut next = |what: &str| {
        argv.next()
            .unwrap_or_else(|| panic!("usage: search_space <graph> <directory> <coordinates> <source> <target> <out.geojson>: missing {what}"))
    };
    let graph_path = next("graph");
    let directory_path = next("directory");
    let coordinates_path = next("coordinates");
    let source: NodeID = next("source").parse().expect("source is a node id");
    let target: NodeID = next("target").parse().expect("target is a node id");
    let out_path = next("output");

    let graph = StaticGraph::new(io::read_edges_from_file(&graph_path));
    let directory: LevelDirectory = io::read_from_file(&directory_path);
    let coordinates = io::read_vec_from_file::<FPCoordinate>(&coordinates_path);
    let levels = directory.levels();
    println!(
        "{} nodes over {levels} levels, {} coordinates",
        directory.number_of_nodes(),
        coordinates.len()
    );

    let customization = Customization::new(graph, directory);
    let mut query = MldSearch::<SettledNodes>::new();
    query.run(&customization, source, &[target]);
    let packed = query
        .retrieve_packed_path(target)
        .expect("the target was not reached");
    let way = unpack(&customization, &packed).expect("the cells offer what they said");
    println!(
        "{} settled, {} steps, {} nodes once put back, costing {}",
        query.stats().settled().len(),
        packed.len(),
        way.len(),
        query.distance(target)
    );
    assert_eq!(
        cost_of_way(customization.graph(), &way),
        Some(query.distance(target)),
        "the way put back is not the way that was found"
    );

    let partition = customization.partition();
    let source_word = partition.word(source);
    let target_word = partition.word(target);
    // the level a node was stepped over at, which is the height it belongs at
    let height_of = |node: NodeID| -> (isize, f64) {
        match partition.query_level(source_word, target_word, node) {
            // the arcs of the graph lie on the ground, so a node the search
            // walked arcs at belongs there too
            None => (-1, 0.0),
            Some(level) => (level as isize, (level + 1) as f64 * LEVEL_HEIGHT),
        }
    };

    let at = |node: NodeID| -> (f64, f64) {
        let c = coordinates[node];
        (f64::from(c.lon) / 1e6, f64::from(c.lat) / 1e6)
    };

    let file = BufWriter::new(File::create(&out_path).expect("output file cannot be opened"));
    let mut writer = FeatureWriter::from_writer(file);
    let mut written = 0usize;

    // how wide a drawn line is, taken from how much ground is on screen, so
    // that a way across a town and a way across a continent both read
    let mut west = f64::MAX;
    let mut east = f64::MIN;
    for &node in &way {
        let (lon, _) = at(node);
        west = west.min(lon);
        east = east.max(lon);
    }
    let width = ((east - west) * 0.004).max(0.000_02);

    // the cells the search stepped over, per level
    let mut stepped: BTreeSet<(usize, CellId)> = BTreeSet::new();
    for &node in query.stats().settled() {
        if let Some(level) = partition.query_level(source_word, target_word, node) {
            stepped.insert((level, partition.cell_of(node, level)));
        }
    }
    println!("{} cells were stepped over", stepped.len());

    let mut nodes_of: FxHashMap<usize, std::sync::Arc<toolbox_rs::customization::Level>> =
        FxHashMap::default();
    for (level, cell) in &stepped {
        let holding = nodes_of
            .entry(*level)
            .or_insert_with(|| customization.level(*level))
            .clone();
        let nodes = &holding.nodes_of_cell[*cell as usize];
        let step = (nodes.len() / HULL_SAMPLE).max(1);
        let points: Vec<FPCoordinate> = nodes
            .iter()
            .step_by(step)
            .map(|&node| coordinates[node])
            .collect();
        let hull = monotone_chain(&points);
        if hull.len() < 3 {
            continue;
        }
        let ring: Vec<Position> = hull
            .iter()
            .chain(std::iter::once(&hull[0]))
            .map(|c| Position::from(vec![f64::from(c.lon) / 1e6, f64::from(c.lat) / 1e6]))
            .collect();
        let base = (*level + 1) as f64 * LEVEL_HEIGHT;
        writer
            .write_feature(&feature(
                GeometryValue::Polygon {
                    coordinates: vec![ring],
                },
                "cell",
                *level as isize,
                base,
                base + SHEET,
            ))
            .expect("the cell cannot be written");
        written += 1;
    }

    // what the search settled, at the height of the level it settled it at
    for &node in query.stats().settled() {
        let (level, height) = height_of(node);
        let (lon, lat) = at(node);
        writer
            .write_feature(&feature(
                GeometryValue::Point {
                    coordinates: Position::from(vec![lon, lat]),
                },
                "settled",
                level,
                height,
                height,
            ))
            .expect("the node cannot be written");
        written += 1;
    }

    // the way the search found, each step at the height it was taken at, and a
    // column under each of its ends running down to the ground
    for pair in packed.windows(2) {
        let (from, to) = (pair[0], pair[1]);
        let (level, height) = height_of(from);
        writer
            .write_feature(&feature(
                GeometryValue::Polygon {
                    coordinates: vec![ribbon(at(from), at(to), width)],
                },
                "packed",
                level,
                height,
                height + SHEET / 4.0,
            ))
            .expect("the step cannot be written");
        written += 1;
    }
    for &node in &packed {
        let (level, height) = height_of(node);
        if height <= 0.0 {
            continue;
        }
        writer
            .write_feature(&feature(
                GeometryValue::Polygon {
                    coordinates: vec![column(at(node), width * 0.6)],
                },
                "column",
                level,
                0.0,
                height,
            ))
            .expect("the column cannot be written");
        written += 1;
    }

    // and the way through the graph that all of it stands for, on the ground
    let ground: Vec<Position> = way
        .iter()
        .map(|&node| {
            let (lon, lat) = at(node);
            Position::from(vec![lon, lat])
        })
        .collect();
    writer
        .write_feature(&feature(
            GeometryValue::LineString {
                coordinates: ground,
            },
            "unpacked",
            -1,
            0.0,
            0.0,
        ))
        .expect("the way cannot be written");
    written += 1;

    writer.finish().expect("the file cannot be closed");
    println!("wrote {written} features to {out_path}");
}

/// A feature carrying what the viewer draws it by.
fn feature(geometry: GeometryValue, kind: &str, level: isize, base: f64, height: f64) -> Feature {
    let mut properties = JsonObject::new();
    properties.insert("kind".to_string(), JsonValue::from(kind));
    properties.insert("level".to_string(), JsonValue::from(level));
    properties.insert("base".to_string(), JsonValue::from(base));
    properties.insert("height".to_string(), JsonValue::from(height));
    Feature {
        bbox: None,
        geometry: Some(Geometry::new(geometry)),
        id: None,
        properties: Some(properties),
        foreign_members: None,
    }
}

/// A step drawn as a quad, since a line cannot be lifted off the ground.
///
/// Nothing draws a line at a height: what a map draws at a height is a
/// polygon pushed up between two of them. So a step is widened into a quad
/// about its own direction and pushed up as a sheet, which reads as a line
/// from anywhere but directly on.
fn ribbon(from: (f64, f64), to: (f64, f64), width: f64) -> Vec<Position> {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = dx.hypot(dy);
    // a step from a node to itself has no direction to be widened about
    let (nx, ny) = if length < f64::EPSILON {
        (width, 0.0)
    } else {
        (-dy / length * width, dx / length * width)
    };
    vec![
        Position::from(vec![from.0 + nx, from.1 + ny]),
        Position::from(vec![to.0 + nx, to.1 + ny]),
        Position::from(vec![to.0 - nx, to.1 - ny]),
        Position::from(vec![from.0 - nx, from.1 - ny]),
        Position::from(vec![from.0 + nx, from.1 + ny]),
    ]
}

/// The footprint of the drop from a node to the ground.
fn column(at: (f64, f64), width: f64) -> Vec<Position> {
    vec![
        Position::from(vec![at.0 - width, at.1 - width]),
        Position::from(vec![at.0 + width, at.1 - width]),
        Position::from(vec![at.0 + width, at.1 + width]),
        Position::from(vec![at.0 - width, at.1 + width]),
        Position::from(vec![at.0 - width, at.1 - width]),
    ]
}
