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
//! level it was taken at, and where it changes level a riser joins the two
//! heights, so that the way climbs from the ground to each cell it steps over
//! and comes back down. Followed end to end it never breaks, and every jump in
//! it is a level the search changed at.
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
//! | `riser`    | where the way changes level, drawn upright        |

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

    // how wide a drawn way is, taken from how much ground it covers, so that a
    // way across a town and a way across a continent both read. Narrow enough
    // that the shape of the way is what shows rather than the width of it.
    let (mut west, mut east) = (f64::MAX, f64::MIN);
    let (mut south, mut north) = (f64::MAX, f64::MIN);
    for &node in &way {
        let (lon, lat) = at(node);
        west = west.min(lon);
        east = east.max(lon);
        south = south.min(lat);
        north = north.max(lat);
    }
    let width = (((east - west).max(north - south)) * 0.0025).max(0.000_02);

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

    // The way the search found, each step drawn at the height it was taken at.
    //
    // A step is drawn as the way it stands for and not as the line between its
    // ends. The two are nothing alike -- a step over a coarse cell is a
    // straight line across a country, and the way beneath it runs where the
    // roads run -- and the straight one says a road runs where none does. Told
    // truthfully, a level shows the same way as the ground does, coarsened
    // only in where it may leave a cell.
    //
    // Which arcs a step stands for is not asked again here. The way was put
    // back by laying the steps end to end, so it comes apart at the nodes the
    // search handed over, each of which it holds once.
    let mut seam = Vec::with_capacity(packed.len());
    seam.push(0usize);
    for &node in &packed[1..] {
        let from = seam.last().expect("a seam was pushed before the loop") + 1;
        let found = way[from..]
            .iter()
            .position(|&held| held == node)
            .map(|offset| from + offset)
            .expect("the way does not hold a node the search stepped to");
        seam.push(found);
    }
    assert_eq!(
        seam.last().copied(),
        Some(way.len() - 1),
        "the way holds more than the steps it was laid out of"
    );

    for (step, pair) in packed.windows(2).enumerate() {
        let (level, height) = height_of(pair[0]);
        let along: Vec<(f64, f64)> = way[seam[step]..=seam[step + 1]]
            .iter()
            .map(|&node| at(node))
            .collect();
        let ring = ribbon_along(&along, width);
        if ring.is_empty() {
            continue;
        }
        writer
            .write_feature(&feature(
                GeometryValue::Polygon {
                    coordinates: vec![ring],
                },
                "packed",
                level,
                // on top of the sheet its level is drawn as, not inside it:
                // the sheet is the thicker of the two and would swallow it
                height + SHEET,
                height + SHEET * 1.35,
            ))
            .expect("the step cannot be written");
        written += 1;
    }
    // Where the way changes level, a riser at the node the two steps share.
    //
    // This is what makes the way one thing rather than a handful of pieces
    // floating at heights that have to be matched up by eye: it starts on the
    // ground, climbs to each cell it steps over, and comes back down to the
    // ground at the far end. Both ends of every riser are the ends of steps
    // already drawn, so the whole of it can be followed without a break.
    //
    // A riser is upright because a map has no way to draw anything else. What
    // is drawn at a height is a polygon pushed up between two of them, and a
    // polygon has one footprint, so a way from one height to another can only
    // go straight up. It reads well enough: the way runs level over a cell and
    // jumps where it changes level, which is what it does.
    for index in 1..packed.len().saturating_sub(1) {
        let node = packed[index];
        let (was, leaving) = height_of(packed[index - 1]);
        let (level, arriving) = height_of(node);
        // two steps at the same level are already laid end to end
        if was == level {
            continue;
        }
        let (low, high) = (leaving.min(arriving), leaving.max(arriving));
        writer
            .write_feature(&feature(
                GeometryValue::Polygon {
                    coordinates: vec![footprint(at(node), width * 0.75)],
                },
                "riser",
                level,
                low + SHEET,
                high + SHEET * 1.35,
            ))
            .expect("the riser cannot be written");
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

/// A way drawn as a ribbon, since a line cannot be lifted off the ground.
///
/// Nothing draws a line at a height: what a map draws at a height is a polygon
/// pushed up between two of them. So the way is widened about itself, out along
/// one side and back along the other, and pushed up as a sheet, which reads as
/// a line from anywhere but directly on.
///
/// The widening is done with longitude scaled to the latitude it is at, so that
/// a way running east is drawn as wide as one running north. Away from the
/// equator a degree of longitude is the shorter of the two, and a ribbon that
/// does not say so is fat one way and thin the other.
fn ribbon_along(points: &[(f64, f64)], width: f64) -> Vec<Position> {
    // a way that stands still has no direction to be widened about
    let mut along: Vec<(f64, f64)> = Vec::with_capacity(points.len());
    for &point in points {
        if along.last() != Some(&point) {
            along.push(point);
        }
    }
    if along.len() < 2 {
        return Vec::new();
    }

    let middle = along.iter().map(|&(_, lat)| lat).sum::<f64>() / along.len() as f64;
    let stretch = middle.to_radians().cos().max(0.01);
    let flat: Vec<(f64, f64)> = along.iter().map(|&(x, y)| (x * stretch, y)).collect();

    // the way each step of it faces, turned a quarter
    let sideways: Vec<(f64, f64)> = flat
        .windows(2)
        .map(|step| {
            let (dx, dy) = (step[1].0 - step[0].0, step[1].1 - step[0].1);
            let length = dx.hypot(dy);
            (-dy / length, dx / length)
        })
        .collect();

    // and at a node, the two steps meeting there, averaged. A corner comes out
    // narrower than a straight, which is what a corner should look like.
    let out_at = |index: usize| -> (f64, f64) {
        let before = sideways[index.saturating_sub(1)];
        let after = sideways[index.min(sideways.len() - 1)];
        let (x, y) = (before.0 + after.0, before.1 + after.1);
        let length = x.hypot(y);
        if length < 1e-12 {
            after
        } else {
            (x / length * width, y / length * width)
        }
    };

    let mut ring: Vec<Position> = Vec::with_capacity(flat.len() * 2 + 1);
    for (index, point) in flat.iter().enumerate() {
        let (dx, dy) = out_at(index);
        ring.push(Position::from(vec![(point.0 + dx) / stretch, point.1 + dy]));
    }
    for (index, point) in flat.iter().enumerate().rev() {
        let (dx, dy) = out_at(index);
        ring.push(Position::from(vec![(point.0 - dx) / stretch, point.1 - dy]));
    }
    ring.push(ring[0].clone());
    ring
}

/// The footprint a riser is pushed up from.
fn footprint(at: (f64, f64), width: f64) -> Vec<Position> {
    vec![
        Position::from(vec![at.0 - width, at.1 - width]),
        Position::from(vec![at.0 + width, at.1 - width]),
        Position::from(vec![at.0 + width, at.1 + width]),
        Position::from(vec![at.0 - width, at.1 + width]),
        Position::from(vec![at.0 - width, at.1 - width]),
    ]
}
