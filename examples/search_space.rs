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
//! # What the colours say
//!
//! A cell is drawn in a shade of the cell holding it. The hue is picked within
//! a span of the parent's and the span narrows at every level, so a family of
//! cells reads as a family however deep it is cut, and the level reads by how
//! light the shade is. Everything the search did inside a cell takes that
//! cell's hue: the arcs it relaxed, the nodes it reached, the nodes it settled
//! and the step of the way it came away with, each at its own brightness. So
//! the colour says where, and the height says how coarse.
//!
//! # What goes in the file
//!
//! One feature collection, every feature carrying `kind`, `level`, a `colour`,
//! and the two heights it is to be drawn between. The heights and the colours
//! are worked out here rather than in the viewer, so that the viewer only has
//! to draw what it is given.
//!
//! | `kind`     | what it is                                          |
//! |------------|-----------------------------------------------------|
//! | `cell`     | the outline of a cell the search stepped over       |
//! | `link`     | two cells of a level the search crossed between     |
//! | `relaxed`  | an arc the search relaxed and kept                  |
//! | `reached`  | a node it put on its queue and never took off       |
//! | `settled`  | a node it took off its queue                        |
//! | `packed`   | the way a step stands for, at the step's height     |
//! | `riser`    | where the way changes level, drawn upright          |
//! | `unpacked` | the whole way through the graph, on the ground      |
//!
//! Everything but `unpacked` is a polygon, since nothing draws a line or a
//! point at a height: what a map draws at a height is a polygon pushed up
//! between two of them.

use std::{collections::BTreeSet, env::args, fs::File, io::BufWriter};

use geojson::{Feature, FeatureWriter, Geometry, GeometryValue, JsonObject, JsonValue, Position};
use rustc_hash::{FxHashMap, FxHashSet};

use toolbox_rs::{
    convex_hull::monotone_chain,
    customization::Customization,
    geometry::FPCoordinate,
    graph::NodeID,
    heap_stats::Frontier,
    io,
    level_directory::{CellId, LevelDirectory},
    mld_query::MldSearch,
    packed_partition::PackedPartition,
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

/// What stands where above the sheet of a level, in thicknesses of it.
///
/// Everything a search did at a level is drawn over that level's sheet, and
/// what stands taller is what is worth seeing first. The arcs lie flattest,
/// a node it only reached is a stub, a node it settled stands up, and the way
/// it came away with floats clear over all of it.
const ARC_TOP: f64 = 1.15;
const LINK_BASE: f64 = 1.15;
const LINK_TOP: f64 = 1.28;
const REACHED_TOP: f64 = 1.3;
const SETTLED_TOP: f64 = 1.9;
const WAY_BASE: f64 = 2.2;
const WAY_TOP: f64 = 3.1;

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
    let mut query = MldSearch::<Frontier>::new();
    query.run(&customization, source, &[target]);
    let packed = query
        .retrieve_packed_path(target)
        .expect("the target was not reached");
    let way = unpack(&customization, &packed).expect("the cells offer what they said");
    let settled: FxHashSet<NodeID> = query.stats().settled().iter().copied().collect();
    println!(
        "{} settled of {} reached, {} steps, {} nodes once put back, costing {}",
        settled.len(),
        query.stats().reached().len(),
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

    // What a node is coloured by: the cell it was stepped over in, or the
    // finest cell holding it where the search was walking arcs and there is no
    // such level. Either way it is a cell of the partition, so everything on
    // screen is coloured by where it happened.
    let shade = |node: NodeID, saturation: f64, lightness: f64| -> String {
        let level = partition
            .query_level(source_word, target_word, node)
            .unwrap_or(0);
        hsl(
            hue_of(partition, node, level, levels),
            saturation,
            lightness,
        )
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

    // where each drawn cell sits and what it came out, for the links between
    // them to be drawn from and coloured by
    let mut centre_of: FxHashMap<(usize, CellId), ((f64, f64), String)> = FxHashMap::default();
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
        // the middle of the outline rather than of the nodes: a link is drawn
        // between two shapes on screen, so it should leave from the middle of
        // the shape and not from wherever the nodes happen to crowd
        let middle = (
            hull.iter().map(|c| f64::from(c.lon) / 1e6).sum::<f64>() / hull.len() as f64,
            hull.iter().map(|c| f64::from(c.lat) / 1e6).sum::<f64>() / hull.len() as f64,
        );
        // a sheet is the dark end of its family, so that everything the search
        // did on it is the same colour only brighter
        let colour = hsl(
            hue_of(partition, nodes[0], *level, levels),
            0.45,
            0.20 + 0.075 * (levels - 1 - *level) as f64,
        );
        writer
            .write_feature(&feature(
                GeometryValue::Polygon {
                    coordinates: vec![ring],
                },
                "cell",
                *level as isize,
                base,
                base + SHEET,
                &colour,
            ))
            .expect("the cell cannot be written");
        centre_of.insert((*level, *cell), (middle, colour));
        written += 1;
    }

    // Every arc the search relaxed and kept.
    //
    // The queue holds, for each node it ever held, the node that node was
    // reached from, so the arcs are read back off it afterwards rather than
    // recorded as the search runs. One arc per node reached, being the last
    // one to improve it, which is the tree the search ends up with.
    //
    // These are drawn straight, unlike the way. A step of the way stands for a
    // path through a cell and is drawn as that path, but an arc of the overlay
    // stands for nothing further down until it is unpacked: it is an entry in
    // a table between two border nodes of a cell, and a straight line is what
    // an entry in a table looks like.
    let mut relaxed = 0usize;
    let mut crossings: BTreeSet<(usize, CellId, CellId)> = BTreeSet::new();
    for &node in query.stats().reached() {
        let Some(parent) = query.parent(node) else {
            continue;
        };
        // the source was reached from nowhere
        if parent == node {
            continue;
        }
        // An arc whose ends are at one level in two cells is the search
        // leaving the one and entering the other, which is the whole of what
        // it means for two cells to be next to each other here. Nothing else
        // has to be asked of the partition: the arcs the search relaxed
        // already say which cells it found its way between.
        if let (Some(from), Some(to)) = (
            partition.query_level(source_word, target_word, parent),
            partition.query_level(source_word, target_word, node),
        ) && from == to
        {
            let (here, there) = (
                partition.cell_of(parent, from),
                partition.cell_of(node, from),
            );
            if here != there {
                crossings.insert((from, here.min(there), here.max(there)));
            }
        }
        let (level, height) = height_of(parent);
        let ring = ribbon_along(&[at(parent), at(node)], width * 0.3);
        if ring.is_empty() {
            continue;
        }
        writer
            .write_feature(&feature(
                GeometryValue::Polygon {
                    coordinates: vec![ring],
                },
                "relaxed",
                level,
                height + SHEET,
                height + SHEET * ARC_TOP,
                &shade(parent, 0.5, 0.38),
            ))
            .expect("the arc cannot be written");
        written += 1;
        relaxed += 1;
    }
    println!("{relaxed} arcs were relaxed and kept");

    // The cells of a level, joined where the search crossed between them.
    //
    // Drawn on their own the cells of a level are a scattering of islands and
    // nothing says which one leads to which, though that is what the search
    // spent its time on: it steps across a cell, leaves by an arc into the
    // next, and steps across that one. A link says the two are next to each
    // other and the search went that way.
    //
    // Dashed, because it is not a thing on the ground. A step of the way is a
    // path through a cell and an arc is an entry in a table, but a link is
    // neither -- it is two shapes on screen having something to do with each
    // other -- and a dashed line is how a drawing says so. The gaps are cut
    // here: what a map draws at a height is a polygon, a polygon is solid, so
    // every dash has to be one of its own.
    let mut linked = 0usize;
    for (level, here, there) in &crossings {
        let (Some((from, tint)), Some((to, other))) = (
            centre_of.get(&(*level, *here)),
            centre_of.get(&(*level, *there)),
        ) else {
            // a cell the search crossed into and never settled in was never
            // drawn, and a link to a shape that is not there is a line to
            // nowhere
            continue;
        };
        let cut = dashes(*from, *to, width * 0.45, width * 4.0);
        if cut.is_empty() {
            continue;
        }
        let base = (*level + 1) as f64 * LEVEL_HEIGHT;
        // one feature for the link and its dashes as the parts of it, rather
        // than one feature per dash: a link is one thing about the partition,
        // and cut into pieces it would be counted and hidden as many
        writer
            .write_feature(&feature(
                GeometryValue::MultiPolygon {
                    coordinates: cut.into_iter().map(|ring| vec![ring]).collect(),
                },
                "link",
                *level as isize,
                base + SHEET * LINK_BASE,
                base + SHEET * LINK_TOP,
                &blend(tint, other),
            ))
            .expect("the link cannot be written");
        written += 1;
        linked += 1;
    }
    println!(
        "{linked} pairs of cells were crossed between, of {} the search found",
        crossings.len()
    );

    // What the search reached and never got round to, and what it settled. A
    // stub for the one and something standing up for the other: the search
    // knows a distance to a node it settled and only a bound for one it
    // reached, and the difference is most of what a search does.
    for &node in query.stats().reached() {
        let (level, height) = height_of(node);
        let settled_here = settled.contains(&node);
        let (top, size, colour) = if settled_here {
            (SETTLED_TOP, 0.55, shade(node, 0.85, 0.62))
        } else {
            (REACHED_TOP, 0.4, shade(node, 0.4, 0.42))
        };
        writer
            .write_feature(&feature(
                GeometryValue::Polygon {
                    coordinates: vec![footprint(at(node), width * size)],
                },
                if settled_here { "settled" } else { "reached" },
                level,
                height + SHEET,
                height + SHEET * top,
                &colour,
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
                // clear over the sheet its level is drawn as and over
                // everything standing on it, rather than inside any of it
                height + SHEET * WAY_BASE,
                height + SHEET * WAY_TOP,
                // the brightest the cell's family goes, so the way is the
                // first thing read at a level and still plainly of that level
                &shade(pair[0], 1.0, 0.66),
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
        // the colour of the end it is climbing to, so a riser reads as
        // arriving somewhere rather than as leaving somewhere
        let arrived = if arriving >= leaving {
            node
        } else {
            packed[index - 1]
        };
        writer
            .write_feature(&feature(
                GeometryValue::Polygon {
                    coordinates: vec![footprint(at(node), width * 0.75)],
                },
                "riser",
                level,
                low + SHEET * WAY_BASE,
                high + SHEET * WAY_TOP,
                &shade(arrived, 1.0, 0.66),
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
            // the one thing on screen that is not of a cell, being the answer
            // rather than a part of the working
            "#f2f2f0",
        ))
        .expect("the way cannot be written");
    written += 1;

    writer.finish().expect("the file cannot be closed");
    println!("wrote {written} features to {out_path}");
}

/// A feature carrying what the viewer draws it by.
fn feature(
    geometry: GeometryValue,
    kind: &str,
    level: isize,
    base: f64,
    height: f64,
    colour: &str,
) -> Feature {
    let mut properties = JsonObject::new();
    properties.insert("kind".to_string(), JsonValue::from(kind));
    properties.insert("level".to_string(), JsonValue::from(level));
    properties.insert("base".to_string(), JsonValue::from(base));
    properties.insert("height".to_string(), JsonValue::from(height));
    properties.insert("colour".to_string(), JsonValue::from(colour));
    Feature {
        bbox: None,
        geometry: Some(Geometry::new(geometry)),
        id: None,
        properties: Some(properties),
        foreign_members: None,
    }
}

/// The hue a cell is drawn in, which is a hue within its parent's.
///
/// The walk starts at the coarsest level and comes down to the one asked for,
/// each cell along the way moving the hue within a span of where its parent
/// left it, and the span narrowing at every level. So two cells of one parent
/// come out near one another, two cells of one grandparent further apart, and
/// how far apart two cells look is how far apart they are in the partition.
///
/// The narrowing is what makes it a shade of the parent rather than a colour
/// of its own. It also means the finest levels have little room left, which is
/// right: at that depth what is worth seeing is which coarse cell something
/// belongs to, not which of a thousand fine ones.
fn hue_of(partition: &PackedPartition, node: NodeID, level: usize, levels: usize) -> f64 {
    let mut hue = 0.0;
    let mut span = 360.0;
    for above in (level..levels).rev() {
        hue += span * (scatter(partition.cell_of(node, above)) - 0.5);
        span *= 0.42;
    }
    hue
}

/// A number in zero to one from a cell.
///
/// Cells that are next to one another are numbered next to one another, and a
/// hue taken straight from the number would draw half a country in one sweep
/// of the spectrum. Stirring the bits first is what makes neighbours tell
/// apart.
fn scatter(cell: CellId) -> f64 {
    let mut hash = u64::from(cell).wrapping_add(0x9e37_79b9_7f4a_7c15);
    hash = (hash ^ (hash >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash = (hash ^ (hash >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^= hash >> 31;
    (hash >> 11) as f64 / (1u64 << 53) as f64
}

/// A colour as `#rrggbb`, from a hue in degrees and a saturation and lightness
/// in zero to one.
///
/// Hue, saturation and lightness rather than red, green and blue because the
/// three things being said are which family, how much of the search, and how
/// coarse, and those are three knobs here and none there.
fn hsl(hue: f64, saturation: f64, lightness: f64) -> String {
    let sixth = hue.rem_euclid(360.0) / 60.0;
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let second = chroma * (1.0 - (sixth % 2.0 - 1.0).abs());
    let (red, green, blue) = match sixth as usize {
        0 => (chroma, second, 0.0),
        1 => (second, chroma, 0.0),
        2 => (0.0, chroma, second),
        3 => (0.0, second, chroma),
        4 => (second, 0.0, chroma),
        _ => (chroma, 0.0, second),
    };
    let base = lightness - chroma / 2.0;
    let byte = |value: f64| ((value + base).clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", byte(red), byte(green), byte(blue))
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

/// A line from one place to another, cut into dashes.
///
/// `dash` is how long a dash is meant to be, and it is met as nearly as a
/// whole number of them fits the distance, so that dashes come out the same
/// length whether a link is short or long. Half of each is drawn and half is
/// left, which is what makes it read as dashed rather than as dotted.
fn dashes(from: (f64, f64), to: (f64, f64), width: f64, dash: f64) -> Vec<Vec<Position>> {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = dx.hypot(dy);
    // two cells whose outlines have the same middle are not two shapes to
    // draw a line between
    if length < f64::EPSILON {
        return Vec::new();
    }
    // and one too short to break up is drawn whole rather than dropped: it is
    // as much a crossing as a long one, and a link that is not drawn reads as
    // two cells the search never went between
    if length < dash * 2.0 {
        let whole = ribbon_along(&[from, to], width);
        return if whole.is_empty() {
            Vec::new()
        } else {
            vec![whole]
        };
    }
    let count = (length / (dash * 2.0)).round().max(1.0);
    let at = |along: f64| (from.0 + dx * along, from.1 + dy * along);
    (0..count as usize)
        .map(|step| {
            let start = step as f64 / count;
            ribbon_along(&[at(start), at(start + 0.5 / count)], width)
        })
        .filter(|ring| !ring.is_empty())
        .collect()
}

/// The colour halfway between two, for something belonging to both.
fn blend(one: &str, other: &str) -> String {
    let band = |colour: &str, at: usize| {
        u16::from_str_radix(&colour[at..at + 2], 16).expect("a colour is six hex digits")
    };
    let mixed: Vec<String> = (0..3)
        .map(|index| {
            let at = 1 + index * 2;
            format!("{:02x}", (band(one, at) + band(other, at)) / 2)
        })
        .collect();
    format!("#{}", mixed.concat())
}

/// The footprint a riser or a node is pushed up from.
fn footprint(at: (f64, f64), width: f64) -> Vec<Position> {
    vec![
        Position::from(vec![at.0 - width, at.1 - width]),
        Position::from(vec![at.0 + width, at.1 - width]),
        Position::from(vec![at.0 + width, at.1 + width]),
        Position::from(vec![at.0 - width, at.1 + width]),
        Position::from(vec![at.0 - width, at.1 - width]),
    ]
}
