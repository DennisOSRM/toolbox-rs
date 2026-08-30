//! Drawing one tile: what of a partition reaches it, and as which layer.
//!
//! A tile carries five layers, and they are not five views of one thing. The
//! cut and the mesh come from the arcs of the graph, the border nodes from its
//! nodes, and the hulls and the shapes from the cells themselves. Each is
//! worked out on its own here, and `build_tile` is what puts them together.

use rustc_hash::FxHashMap;

use toolbox_rs::{
    graph::{Graph, NodeID},
    level_directory::CellId,
    mvt::GeometryEncoder,
    tile_geometry::{
        TILE_EXTENT, box_reaches_tile, clip_ring_to_tile, clip_to_tile, is_within_tile,
        ring_reaches_tile, tile_bounds, to_tile_coordinate,
    },
};

use crate::state::{Hull, ServerState};
use crate::tile_index::INDEX_ZOOM;
use crate::{
    Tile,
    tile::{Feature, GeomType, Layer, Value},
};

/// The name of the layer the cells are drawn into. The style of the client
/// refers to it by this name.
pub(crate) const CELL_LAYER: &str = "cells";

/// The name of the layer the border nodes are drawn into.
pub(crate) const BORDER_LAYER: &str = "border_nodes";

/// The name of the layer that holds every arc, tinted by the cell it belongs
/// to. The cut alone is a handful of arcs per cell and reads as scattered
/// dashes, whereas the arcs inside a cell fill it in and make it a region.
pub(crate) const INTERIOR_LAYER: &str = "interior";

/// The name of the layer that holds the alpha shape of each cell: the hull a
/// disc of a given radius carves out of the convex one, which follows the cell
/// into its bays and may leave it in several pieces.
pub(crate) const SHAPE_LAYER: &str = "shapes";

/// The name of the layer that holds the convex hull of each cell. A hull is
/// the shape the cell would have if it were convex, which it is not: hulls of
/// neighbouring cells overlap, and heavily, which is what the region layer
/// avoids and what makes the two worth having next to each other.
pub(crate) const HULL_LAYER: &str = "hulls";

/// How many levels up the cell a cell takes its colour from sits. Each level
/// of the sizes a partition is usually asked for is a factor of four or five in
/// area, so three levels is a hundredfold: a colour then covers a region a
/// reader takes in at a glance while the cells inside it keep their own shade
/// of it. Fewer, and the colours are as busy as the cells themselves.
pub(crate) const COLOUR_LEVELS: usize = 3;

/// The zoom level from which on the arcs inside a cell are drawn. Below it a
/// cell covers a few pixels, its roads fall on top of each other, and drawing
/// them costs megabytes per tile for a smear. The cut is drawn at every level,
/// as that is what the shape of a cell is made of.
pub(crate) const MIN_INTERIOR_ZOOM: u32 = 10;

/// The zoom below which the layers that read the bucket index are left out.
///
/// A tile of a low zoom covers the whole index: at zoom zero the range is the
/// four thousand buckets of each axis, which is sixteen million lookups for a
/// picture in which no arc is a pixel long. The hulls are drawn from the cells
/// themselves rather than from the index, so a tile down there carries those
/// and nothing else, which is all that can be seen at that size anyway.
pub(crate) const MIN_BUCKET_ZOOM: u32 = 6;

/// Which tile of the pyramid is being drawn.
#[derive(Clone, Copy)]
struct At {
    zoom: u32,
    x: u32,
    y: u32,
}

/// The range of buckets of the index that a tile covers.
struct Buckets {
    from_x: u32,
    from_y: u32,
    to_x: u32,
    to_y: u32,
}

impl Buckets {
    fn across(&self) -> impl Iterator<Item = (u32, u32)> + use<'_> {
        (self.from_x..=self.to_x)
            .flat_map(move |bucket_x| (self.from_y..=self.to_y).map(move |y| (bucket_x, y)))
    }

    fn holds(&self, x: u32, y: u32) -> bool {
        (self.from_x..=self.to_x).contains(&x) && (self.from_y..=self.to_y).contains(&y)
    }
}

/// Builds the tile that covers the given position of the tile pyramid. The
/// boundary arcs are grouped by the cell they belong to, so that a client can
/// give each cell a color of its own.
pub fn build_tile(state: &ServerState, level: u32, zoom: u32, x: u32, y: u32) -> Tile {
    let of_level = state.level(level as usize);
    let cells = &of_level.of_node;
    let at = At { zoom, x, y };
    let (from_x, from_y, to_x, to_y) = crate::index_tiles_of(zoom, x, y);
    let buckets = Buckets {
        from_x,
        from_y,
        to_x,
        to_y,
    };

    let (cut, interior) = arcs_of_tile(state, cells, at, &buckets);
    let (features, values) = linestrings_of(cut);
    let (interior_features, interior_values) = linestrings_of(interior);
    let (node_features, node_values) = border_nodes_of(state, cells, at, &buckets);

    // how far up the cell a cell takes its colour from sits
    let above = (level as usize + COLOUR_LEVELS).min(state.max_level as usize);
    let hulls = state.hulls(level as usize);
    let reaching = state
        .cell_tree(level as usize)
        .map_or_else(Vec::new, |tree| {
            tree.intersecting(tile_bounds(zoom, x, y))
                .map(|held| held.cell)
                .collect::<Vec<_>>()
        });
    let (hull_features, hull_values) = hulls_of(state, level, above, &reaching, &hulls, at);
    let (shape_features, shape_values) = shapes_of(state, level, above, &reaching, &hulls, at);

    let layer = |name: &str, features, keys: &[&str], values| Layer {
        version: 2,
        name: name.to_string(),
        extent: Some(TILE_EXTENT),
        features,
        keys: keys.iter().map(|&key| key.to_string()).collect(),
        values,
    };
    Tile {
        layers: vec![
            layer(
                SHAPE_LAYER,
                shape_features,
                &["cell", "above"],
                shape_values,
            ),
            layer(HULL_LAYER, hull_features, &["cell", "above"], hull_values),
            layer(
                INTERIOR_LAYER,
                interior_features,
                &["cell"],
                interior_values,
            ),
            layer(CELL_LAYER, features, &["cell"], values),
            layer(BORDER_LAYER, node_features, &["cell", "node"], node_values),
        ],
    }
}

/// The cell a cell takes its colour from, which is the one `above - level`
/// steps up from it.
fn ancestor_of(state: &ServerState, cell: CellId, level: u32, above: usize) -> CellId {
    let mut ancestor = cell;
    for climbed in level as usize..above {
        ancestor = state.directory().parents_on_level(climbed)[ancestor as usize];
    }
    ancestor
}

/// One line string feature per cell, which is the shape both the cut and the
/// mesh come out in.
fn linestrings_of(geometries: FxHashMap<CellId, GeometryEncoder>) -> (Vec<Feature>, Vec<Value>) {
    let mut features = Vec::with_capacity(geometries.len());
    let mut values = Vec::with_capacity(geometries.len());
    for (cell, geometry) in geometries {
        features.push(Feature {
            id: Some(u64::from(cell)),
            r#type: Some(GeomType::Linestring.into()),
            geometry: geometry.build(),
            tags: vec![
                0,
                u32::try_from(values.len()).expect("too many cells on one tile"),
            ],
        });
        values.push(Value {
            uint_value: Some(u64::from(cell)),
            ..Default::default()
        });
    }
    (features, values)
}

/// Every arc that reaches the tile, sorted into the cut and the mesh.
///
/// Three walks fill the two: the arcs of the cut that end in a bucket the tile
/// covers, the arcs that run clean over one without ending in it, and the arcs
/// that stay inside their cell. They share a walk because each arc is looked
/// at once and lands in whichever of the two it belongs to.
fn arcs_of_tile(
    state: &ServerState,
    cells: &[CellId],
    at: At,
    buckets: &Buckets,
) -> (
    FxHashMap<CellId, GeometryEncoder>,
    FxHashMap<CellId, GeometryEncoder>,
) {
    let At { zoom, x, y } = at;
    let data = &state.tiles;
    let mut cut: FxHashMap<CellId, GeometryEncoder> = FxHashMap::default();
    let mut interior: FxHashMap<CellId, GeometryEncoder> = FxHashMap::default();
    if zoom < MIN_BUCKET_ZOOM {
        return (cut, interior);
    }

    // an arc of the cut is listed under both of its ends, so the offsets of the
    // buckets this tile covers are collected and weeded out before drawing
    let mut cut_arcs = Vec::new();
    for bucket in buckets.across() {
        if let Some(arcs) = data.boundary_by_tile.get(&bucket) {
            cut_arcs.extend_from_slice(arcs);
        }
    }
    cut_arcs.sort_unstable();
    cut_arcs.dedup();

    for arc in cut_arcs
        .iter()
        .map(|&offset| &data.boundary[offset as usize])
    {
        let cell = cells[arc.from];
        if cell == cells[arc.to] {
            // both sides fall into the same cell here, so the arc separates
            // nothing and belongs to the interior
            continue;
        }
        let Some((source, target)) = clip_to_tile(
            to_tile_coordinate(arc.source, zoom, x, y),
            to_tile_coordinate(arc.target, zoom, x, y),
        ) else {
            continue;
        };
        // each arc is a line string of its own within the feature of its cell
        let geometry = cut.entry(cell).or_default();
        geometry.move_to(&[source]);
        geometry.line_to(&[target]);
    }

    // The arcs that only pass through the buckets this tile covers. Neither of
    // their ends is in one, so the walk over the nodes above never reached
    // them, and a ferry across open water was drawn by nobody.
    for bucket in buckets.across() {
        let Some(offsets) = data.crossing_by_tile.get(&bucket) else {
            continue;
        };
        for &offset in offsets {
            let (source, target) = data.crossing[offset as usize];
            let Some((from, to)) = clip_to_tile(
                to_tile_coordinate(state.coordinates[source], zoom, x, y),
                to_tile_coordinate(state.coordinates[target], zoom, x, y),
            ) else {
                continue;
            };
            let cell = cells[source];
            if cell == cells[target] {
                // inside a cell of this level, so it belongs to the mesh, which
                // is only drawn where the mesh is drawn
                if zoom >= MIN_INTERIOR_ZOOM {
                    let geometry = interior.entry(cell).or_default();
                    geometry.move_to(&[from]);
                    geometry.line_to(&[to]);
                }
            } else {
                let geometry = cut.entry(cell).or_default();
                geometry.move_to(&[from]);
                geometry.line_to(&[to]);
            }
        }
    }

    if zoom < MIN_INTERIOR_ZOOM {
        return (cut, interior);
    }

    // Every arc that stays inside its cell, so that a cell reads as a region
    // rather than as the handful of dashes its cut consists of. Only the nodes
    // of the index tile that this one falls into can reach it.
    let covers = |node: NodeID| {
        let bucket = data.bucket_of_node[node];
        buckets.holds(bucket >> INDEX_ZOOM, bucket & ((1 << INDEX_ZOOM) - 1))
    };
    for bucket in buckets.across() {
        let Some(nodes) = data.nodes_by_tile.get(&bucket) else {
            continue;
        };
        for &node in nodes {
            let cell = cells[node];
            let from = to_tile_coordinate(state.coordinates[node], zoom, x, y);
            for edge in state.graph().edge_range(node) {
                let target = state.graph().target(edge);
                // the cut is drawn by the layer above this one
                if cells[target] != cell {
                    continue;
                }
                // The graph holds both directions of an arc, so one of them has
                // to be dropped. Dropping by node id alone would drop an arc
                // whose other end lies outside of what this tile covers, as
                // nothing else draws it then, which tore a seam into every
                // boundary of the index. An arc is therefore only dropped when
                // the end it would be drawn from is covered too.
                if target <= node && covers(target) {
                    continue;
                }
                let to = to_tile_coordinate(state.coordinates[target], zoom, x, y);
                let Some((from, to)) = clip_to_tile(from, to) else {
                    continue;
                };
                let geometry = interior.entry(cell).or_default();
                geometry.move_to(&[from]);
                geometry.line_to(&[to]);
            }
        }
    }
    (cut, interior)
}

/// The border nodes that fall on the tile, as points a client can put a cursor
/// on to ask what the distances of the cell are.
fn border_nodes_of(
    state: &ServerState,
    cells: &[CellId],
    at: At,
    buckets: &Buckets,
) -> (Vec<Feature>, Vec<Value>) {
    let At { zoom, x, y } = at;
    let data = &state.tiles;
    let mut node_features = Vec::new();
    let mut node_values = Vec::new();
    // a ring of one bucket wider than the tile covers, as a node that sits in
    // the margin belongs to the bucket next door and is still drawn here: that
    // is what keeps a circle that straddles the border whole on both sides
    let ring = Buckets {
        from_x: buckets.from_x.saturating_sub(1),
        from_y: buckets.from_y.saturating_sub(1),
        to_x: buckets.to_x + 1,
        to_y: buckets.to_y + 1,
    };
    for border in ring
        .across()
        .take_while(|_| zoom >= MIN_INTERIOR_ZOOM)
        .filter_map(|tile| data.border_by_tile.get(&tile))
        .flatten()
        .map(|&offset| &data.border_nodes[offset as usize])
    {
        let position = to_tile_coordinate(border.coordinate, zoom, x, y);
        if !is_within_tile(position) {
            continue;
        }
        // a node of the leaf border only stays on the border while an arc of
        // it still leaves the cell of this level
        let cell = cells[border.node];
        if !state
            .graph()
            .edge_range(border.node)
            .any(|edge| cells[state.graph().target(edge)] != cell)
        {
            continue;
        }
        let mut geometry = GeometryEncoder::with_capacity(3);
        geometry.move_to(&[position]);
        node_features.push(Feature {
            id: Some(border.node as u64),
            r#type: Some(GeomType::Point.into()),
            geometry: geometry.build(),
            tags: vec![
                0,
                u32::try_from(node_values.len()).expect("too many border nodes on one tile"),
                1,
                u32::try_from(node_values.len() + 1).expect("too many border nodes on one tile"),
            ],
        });
        node_values.push(Value {
            uint_value: Some(u64::from(cell)),
            ..Default::default()
        });
        node_values.push(Value {
            uint_value: Some(border.node as u64),
            ..Default::default()
        });
    }
    (node_features, node_values)
}

/// The convex hull of each cell that reaches this tile. They overlap, which is
/// the point of showing them: a hull is what a cell looks like if it is taken
/// for convex, and a cell of a road network is nothing like convex.
fn hulls_of(
    state: &ServerState,
    level: u32,
    above: usize,
    reaching: &[CellId],
    hulls: &[Hull],
    at: At,
) -> (Vec<Feature>, Vec<Value>) {
    let At { zoom, x, y } = at;
    let mut hull_features = Vec::new();
    let mut hull_values = Vec::new();
    for &cell in reaching {
        let (hull, corners) = &hulls[cell as usize];
        if hull.len() < 3 || !box_reaches_tile(corners, zoom, x, y) {
            continue;
        }
        let ring = hull
            .iter()
            .map(|&coordinate| to_tile_coordinate(coordinate, zoom, x, y))
            .collect::<Vec<_>>();
        let clipped = clip_ring_to_tile(&ring);
        if clipped.len() < 3 {
            continue;
        }

        let ancestor = ancestor_of(state, cell, level, above);
        let mut geometry = GeometryEncoder::with_capacity(clipped.len() * 2 + 2);
        geometry.move_to(&clipped[..1]);
        geometry.line_to(&clipped[1..]);
        geometry.close_path();
        hull_features.push(Feature {
            id: Some(u64::from(cell)),
            r#type: Some(GeomType::Polygon.into()),
            geometry: geometry.build(),
            tags: vec![
                0,
                u32::try_from(hull_values.len()).expect("too many cells on one tile"),
                1,
                u32::try_from(hull_values.len() + 1).expect("too many cells on one tile"),
            ],
        });
        hull_values.push(Value {
            uint_value: Some(u64::from(cell)),
            ..Default::default()
        });
        hull_values.push(Value {
            uint_value: Some(u64::from(ancestor)),
            ..Default::default()
        });
    }
    (hull_features, hull_values)
}

/// The same cells again, as the shape a disc of the given radius carves out of
/// the hull. Where a hull reaches over a bay this follows the cell into it, and
/// where a cell falls into pieces this says so with a ring apiece.
fn shapes_of(
    state: &ServerState,
    level: u32,
    above: usize,
    reaching: &[CellId],
    hulls: &[Hull],
    at: At,
) -> (Vec<Feature>, Vec<Value>) {
    let At { zoom, x, y } = at;
    let shapes = state.shapes(level as usize);
    let mut shape_features = Vec::new();
    let mut shape_values = Vec::new();
    for &cell in reaching {
        let at = cell as usize;
        let rings = &shapes[at];
        if rings.is_empty() || !box_reaches_tile(&hulls[at].1, zoom, x, y) {
            continue;
        }
        let mut geometry = GeometryEncoder::default();
        let mut drawn = 0;
        for ring in rings {
            let on_tile = ring
                .iter()
                .map(|&coordinate| to_tile_coordinate(coordinate, zoom, x, y))
                .collect::<Vec<_>>();
            // A ring of a shape is handed over whole rather than cut to the
            // tile. The cut is Sutherland and Hodgman, which holds for a convex
            // ring and not for these: a shape is concave by the whole point of
            // it, and cutting one welds the parts that leave the tile together
            // along its edge. A ferry to an island then comes back as a sliver
            // lying on the border rather than as the spit it is. A reader cuts
            // what hangs over anyway, and this only leaves out the rings that
            // do not reach the tile at all.
            if !ring_reaches_tile(&on_tile) || on_tile.len() < 3 {
                continue;
            }
            geometry.move_to(&on_tile[..1]);
            geometry.line_to(&on_tile[1..]);
            geometry.close_path();
            drawn += 1;
        }
        if drawn == 0 {
            continue;
        }

        let ancestor = ancestor_of(state, cell, level, above);
        shape_features.push(Feature {
            id: Some(u64::from(cell)),
            r#type: Some(GeomType::Polygon.into()),
            geometry: geometry.build(),
            tags: vec![
                0,
                u32::try_from(shape_values.len()).expect("too many cells on one tile"),
                1,
                u32::try_from(shape_values.len() + 1).expect("too many cells on one tile"),
            ],
        });
        shape_values.push(Value {
            uint_value: Some(u64::from(cell)),
            ..Default::default()
        });
        shape_values.push(Value {
            uint_value: Some(u64::from(ancestor)),
            ..Default::default()
        });
    }
    (shape_features, shape_values)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use toolbox_rs::{
        edge::InputEdge,
        geometry::FPCoordinate,
        level_directory::LevelDirectory,
        static_graph::StaticGraph,
        vector_tile::coordinate_to_tile_number,
        wgs84::{FloatCoordinate, FloatLatitude, FloatLongitude},
    };

    const LAT: f64 = 50.20;
    const LON: f64 = 8.57;
    const ZOOM: u32 = 14;

    /// Two cells of three nodes each, both under one cell on the level above,
    /// with an arc running between them so that the cut is not empty.
    fn state() -> ServerState {
        let coordinates = vec![
            FPCoordinate::new_from_lat_lon(LAT, LON),
            FPCoordinate::new_from_lat_lon(LAT + 0.01, LON),
            FPCoordinate::new_from_lat_lon(LAT, LON + 0.01),
            FPCoordinate::new_from_lat_lon(LAT + 0.002, LON + 0.002),
            FPCoordinate::new_from_lat_lon(LAT + 0.012, LON + 0.002),
            FPCoordinate::new_from_lat_lon(LAT + 0.002, LON + 0.012),
        ];
        let mut edges = Vec::new();
        for (from, to) in [(0, 1), (1, 2), (2, 0), (3, 4), (4, 5), (5, 3), (2, 3)] {
            edges.push(InputEdge::new(from, to, 1_u32));
            edges.push(InputEdge::new(to, from, 1_u32));
        }
        ServerState::new(
            StaticGraph::new(edges),
            coordinates,
            LevelDirectory::new(vec![0, 0, 0, 1, 1, 1], vec![vec![0, 0]]),
            300.0,
        )
    }

    fn tile_of_the_data() -> (u32, u32) {
        coordinate_to_tile_number(
            FloatCoordinate {
                lat: FloatLatitude(LAT),
                lon: FloatLongitude(LON),
            },
            ZOOM,
        )
    }

    fn range() -> Buckets {
        Buckets {
            from_x: 4,
            from_y: 10,
            to_x: 6,
            to_y: 11,
        }
    }

    #[test]
    fn a_bucket_range_holds_its_own_corners() {
        let held = range();
        for (x, y) in [(4, 10), (6, 11), (4, 11), (6, 10)] {
            assert!(held.holds(x, y), "({x}, {y}) is a corner of the range");
        }
        assert!(held.holds(5, 10));
    }

    #[test]
    fn a_bucket_outside_the_range_is_not_held() {
        let held = range();
        for (x, y) in [(3, 10), (7, 10), (4, 9), (4, 12)] {
            assert!(!held.holds(x, y), "({x}, {y}) is outside the range");
        }
    }

    #[test]
    fn the_range_walks_every_bucket_it_holds_exactly_once() {
        let held = range();
        let walked = held.across().collect::<Vec<_>>();
        // three across and two down, both ends included
        assert_eq!(walked.len(), 3 * 2);
        assert_eq!(walked.iter().copied().collect::<HashSet<_>>().len(), 6);
        for &(x, y) in &walked {
            assert!(held.holds(x, y), "({x}, {y}) was walked but is not held");
        }
    }

    #[test]
    fn a_range_of_one_bucket_walks_that_one() {
        let held = Buckets {
            from_x: 2,
            from_y: 3,
            to_x: 2,
            to_y: 3,
        };
        assert_eq!(held.across().collect::<Vec<_>>(), vec![(2, 3)]);
    }

    fn drawn(points: &[(i32, i32)]) -> GeometryEncoder {
        let mut geometry = GeometryEncoder::with_capacity(points.len());
        geometry.move_to(&points[..1]);
        geometry.line_to(&points[1..]);
        geometry
    }

    #[test]
    fn a_cell_becomes_one_line_string_feature() {
        let mut geometries = FxHashMap::default();
        geometries.insert(3_u32, drawn(&[(0, 0), (10, 10)]));
        let (features, values) = linestrings_of(geometries);
        assert_eq!(features.len(), 1);
        assert_eq!(values.len(), 1);
        assert_eq!(features[0].id, Some(3));
        assert_eq!(features[0].r#type, Some(GeomType::Linestring.into()));
        assert_eq!(values[0].uint_value, Some(3));
    }

    #[test]
    fn every_feature_points_at_a_value_that_is_there() {
        let mut geometries = FxHashMap::default();
        for cell in 0..5_u32 {
            geometries.insert(cell, drawn(&[(0, 0), (1, 1)]));
        }
        let (features, values) = linestrings_of(geometries);
        assert_eq!(features.len(), 5);
        for feature in &features {
            // a tag is a pair: which key, and which value of the layer
            assert_eq!(feature.tags.len(), 2);
            assert_eq!(feature.tags[0], 0, "the only key of the layer");
            let at = feature.tags[1] as usize;
            assert!(
                at < values.len(),
                "tag {at} is past the {} values",
                values.len()
            );
            assert_eq!(values[at].uint_value, feature.id);
        }
    }

    #[test]
    fn nothing_to_draw_makes_no_features() {
        let (features, values) = linestrings_of(FxHashMap::default());
        assert!(features.is_empty());
        assert!(values.is_empty());
    }

    #[test]
    fn a_cell_that_climbs_no_levels_stays_where_it_is() {
        let state = state();
        assert_eq!(ancestor_of(&state, 1, 0, 0), 1);
    }

    #[test]
    fn a_cell_climbs_to_the_one_that_holds_it() {
        let state = state();
        // both cells of the finest level sit under cell zero of the one above
        assert_eq!(ancestor_of(&state, 0, 0, 1), 0);
        assert_eq!(ancestor_of(&state, 1, 0, 1), 0);
    }

    #[test]
    fn a_tile_carries_the_five_layers_it_promises() {
        let state = state();
        let (x, y) = tile_of_the_data();
        let tile = build_tile(&state, 0, ZOOM, x, y);
        let names = tile
            .layers
            .iter()
            .map(|layer| layer.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            vec![
                SHAPE_LAYER,
                HULL_LAYER,
                INTERIOR_LAYER,
                CELL_LAYER,
                BORDER_LAYER
            ]
        );
    }

    #[test]
    fn every_layer_of_a_tile_declares_the_extent_and_the_version() {
        let state = state();
        let (x, y) = tile_of_the_data();
        let tile = build_tile(&state, 0, ZOOM, x, y);
        for layer in &tile.layers {
            assert_eq!(layer.extent, Some(TILE_EXTENT), "{}", layer.name);
            assert_eq!(layer.version, 2, "{}", layer.name);
            assert!(
                layer.features.len() >= layer.values.len().saturating_sub(layer.features.len()),
                "{} has more values than anything points at",
                layer.name
            );
        }
    }

    #[test]
    fn the_cut_of_a_tile_names_the_cells_it_drew() {
        let state = state();
        let (x, y) = tile_of_the_data();
        let tile = build_tile(&state, 0, ZOOM, x, y);
        let cut = tile
            .layers
            .iter()
            .find(|layer| layer.name == CELL_LAYER)
            .expect("no cut layer");
        assert_eq!(cut.keys, vec!["cell".to_string()]);
        for feature in &cut.features {
            assert!(!feature.geometry.is_empty(), "a feature that draws nothing");
        }
    }

    #[test]
    fn a_tile_far_from_the_data_draws_nothing() {
        let state = state();
        // the other side of the world at the same zoom
        let (x, y) = coordinate_to_tile_number(
            FloatCoordinate {
                lat: FloatLatitude(-33.86),
                lon: FloatLongitude(151.20),
            },
            ZOOM,
        );
        let tile = build_tile(&state, 0, ZOOM, x, y);
        for layer in &tile.layers {
            assert!(
                layer.features.is_empty(),
                "{} drew {} features half a world away",
                layer.name,
                layer.features.len()
            );
        }
    }
}
