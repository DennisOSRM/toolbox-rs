mod command_line;

use actix_web::{App, HttpResponse, HttpServer, Responder, web};
use command_line::Arguments;
use env_logger::{Builder, Env};
use log::{debug, info, warn};
use prost::Message;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use std::{
    error::Error,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use tile::{Feature, GeomType, Layer, Value};
use toolbox_rs::{
    edge::InputEdge,
    geometry::FPCoordinate,
    graph::{Graph, NodeID},
    io,
    mvt::GeometryEncoder,
    one_to_many_dijkstra::OneToManyDijkstra,
    partition_id::PartitionID,
    static_graph::{self, StaticGraph},
    vector_tile::{TILE_SIZE, coordinate_to_tile_number, degree_to_pixel_lat, degree_to_pixel_lon},
    wgs84::{FloatCoordinate, FloatLatitude, FloatLongitude},
};

// Include the generated protobuf code
include!(concat!(env!("OUT_DIR"), "/vector_tile.rs"));

const INDEX_HTML: &str = include_str!("../client/index.html");

/// The extent a tile draws its geometry on. It matches the grid the pixel
/// conversions of the library work in, so a global pixel coordinate minus the
/// origin of the tile is already the number a tile carries.
const TILE_EXTENT: u32 = TILE_SIZE as u32;

/// The name of the layer the cells are drawn into. The style of the client
/// refers to it by this name.
const CELL_LAYER: &str = "cells";

/// The name of the layer the border nodes are drawn into.
const BORDER_LAYER: &str = "border_nodes";

/// The name of the layer that holds every arc, tinted by the cell it belongs
/// to. The cut alone is a handful of arcs per cell and reads as scattered
/// dashes, whereas the arcs inside a cell fill it in and make it a region.
const INTERIOR_LAYER: &str = "interior";

/// The zoom level the arcs are bucketed by. A request at this level or above
/// falls into exactly one bucket, which is all that has to be looked at. The
/// client asks for nothing below it.
const INDEX_ZOOM: u32 = 12;

/// How many distances a popup is handed. A cell can have far more border nodes
/// than fit on a screen, so the closest ones are handed over and the rest is
/// reported as a count.
const POPUP_DISTANCES: usize = 12;

/// How far outside of a tile geometry is still drawn, in tile units. Renderers
/// need a margin to draw the width of a line whose center lies outside.
const TILE_MARGIN: f64 = 128.;

/// An arc of the graph whose endpoints lie in different cells, i.e. a piece of
/// the boundary between two cells.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BoundaryArc {
    source: FPCoordinate,
    target: FPCoordinate,
    cell: PartitionID,
    /// the cell on the other side, so that a level at which both sides fall
    /// into the same cell can drop the arc
    other: PartitionID,
}

/// A node that an arc leaves its cell on. The distances between the border
/// nodes of a cell are what a cell is summarized by, so these are the nodes
/// worth asking about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct BorderNode {
    coordinate: FPCoordinate,
    cell: PartitionID,
    node: NodeID,
}

/// The part of the input the tile requests are answered from.
struct TileData {
    boundary: Vec<BoundaryArc>,
    border_nodes: Vec<BorderNode>,
    /// the nodes that fall into a tile of [`INDEX_ZOOM`], so that a request
    /// only has to look at the arcs that can reach it
    nodes_by_tile: FxHashMap<(u32, u32), Vec<NodeID>>,
}

/// The cell that a cell of the leaf level falls into at the given level of the
/// hierarchy. An id carries one bit per level below the root, so walking up is
/// dropping the lower ones. Note that this is not `parent_at_level`, which
/// clears those bits instead of shifting them out and so keeps the id on the
/// level it started at.
fn cell_at_level(cell: PartitionID, level: u32) -> PartitionID {
    let Some(steps) = u32::from(cell.level()).checked_sub(level) else {
        // already at or above the level that was asked for
        return cell;
    };
    PartitionID::new((cell.0 >> steps).max(PartitionID::root().0))
}

/// The tile of [`INDEX_ZOOM`] that the given tile lies in.
fn index_tile_of(zoom: u32, x: u32, y: u32) -> (u32, u32) {
    if zoom <= INDEX_ZOOM {
        let up = INDEX_ZOOM - zoom;
        (x << up, y << up)
    } else {
        let down = zoom - INDEX_ZOOM;
        (x >> down, y >> down)
    }
}

impl TileData {
    /// Collects the arcs that leave their cell together with the nodes they
    /// leave on. Those arcs are what separates one cell from the next, so
    /// drawing them draws the partition. Each pair of nodes is taken once, as
    /// the graph holds both directions of an arc.
    fn new(
        graph: &StaticGraph<usize>,
        coordinates: &[FPCoordinate],
        partition_ids: &[PartitionID],
    ) -> Self {
        let mut boundary = Vec::new();
        let mut border_nodes = Vec::new();
        let mut nodes_by_tile: FxHashMap<(u32, u32), Vec<NodeID>> = FxHashMap::default();
        for source in graph.node_range() {
            let (lon, lat) = coordinates[source].to_lon_lat_pair();
            let tile = coordinate_to_tile_number(
                FloatCoordinate {
                    lat: FloatLatitude(lat),
                    lon: FloatLongitude(lon),
                },
                INDEX_ZOOM,
            );
            nodes_by_tile.entry(tile).or_default().push(source);

            let mut leaves_cell = false;
            for edge in graph.edge_range(source) {
                let target = graph.target(edge);
                if partition_ids[source] == partition_ids[target] {
                    continue;
                }
                leaves_cell = true;
                // the reverse of this arc carries the same segment
                if source < target {
                    boundary.push(BoundaryArc {
                        source: coordinates[source],
                        target: coordinates[target],
                        cell: partition_ids[source],
                        other: partition_ids[target],
                    });
                }
            }
            if leaves_cell {
                border_nodes.push(BorderNode {
                    coordinate: coordinates[source],
                    cell: partition_ids[source],
                    node: source,
                });
            }
        }
        boundary.shrink_to_fit();
        border_nodes.shrink_to_fit();
        for nodes in nodes_by_tile.values_mut() {
            // the nodes were collected in order, which is what the membership
            // test of the interior arcs relies on
            debug_assert!(nodes.windows(2).all(|pair| pair[0] < pair[1]));
            nodes.shrink_to_fit();
        }
        Self {
            boundary,
            border_nodes,
            nodes_by_tile,
        }
    }
}

/// Converts a coordinate into the grid that the tile at the given position
/// draws on. Coordinates outside of the tile keep their offset instead of being
/// clamped onto its border, which is what lets a line that crosses the border
/// leave it at the right angle.
fn to_tile_coordinate(coordinate: FPCoordinate, zoom: u32, tile_x: u32, tile_y: u32) -> (i32, i32) {
    let (lon, lat) = coordinate.to_lon_lat_pair();
    let x =
        degree_to_pixel_lon(FloatLongitude(lon), zoom) - f64::from(tile_x) * f64::from(TILE_EXTENT);
    let y =
        degree_to_pixel_lat(FloatLatitude(lat), zoom) - f64::from(tile_y) * f64::from(TILE_EXTENT);

    // the grid is far smaller than the range of an i32, but a coordinate of a
    // broken input should not wrap around into a plausible looking one
    (
        x.round().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32,
        y.round().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32,
    )
}

/// Whether a position lies on the tile, margin included. A point either sits on
/// the tile or it does not, so unlike a segment it cannot span it.
fn is_within_tile(position: (i32, i32)) -> bool {
    let low = -TILE_MARGIN;
    let high = f64::from(TILE_EXTENT) + TILE_MARGIN;
    let within = |value: i32| f64::from(value) >= low && f64::from(value) <= high;
    within(position.0) && within(position.1)
}

/// Cuts a segment down to the part of it that lies on the tile, margin
/// included, and hands back `None` for one that misses the tile altogether.
///
/// An arc of a road network is far longer than a tile at a low zoom level, and
/// an endpoint of it can land thousands of units outside the grid. A reader
/// only honours a buffer of its own around the tile and drops or mangles what
/// reaches past it, so the part that hangs over is cut off here rather than
/// handed over.
///
/// The segment is clipped against the four edges by the Liang-Barsky method:
/// the segment is walked as `source + t * (target - source)` for `t` in
/// `[0, 1]`, and each edge either moves the near end forward or the far end
/// back until the interval either is the part that lies on the tile or has
/// closed, in which case the segment never touches it.
fn clip_to_tile(source: (i32, i32), target: (i32, i32)) -> Option<((i32, i32), (i32, i32))> {
    let low = -TILE_MARGIN;
    let high = f64::from(TILE_EXTENT) + TILE_MARGIN;

    let (x, y) = (f64::from(source.0), f64::from(source.1));
    let (dx, dy) = (f64::from(target.0) - x, f64::from(target.1) - y);

    let mut near = 0_f64;
    let mut far = 1_f64;
    // one pair per edge: how fast the segment approaches it, and how far the
    // near end still is from it
    for (speed, distance) in [
        (-dx, x - low),
        (dx, high - x),
        (-dy, y - low),
        (dy, high - y),
    ] {
        if speed == 0. {
            // parallel to this edge, so it either lies on the tile or misses it
            // no matter how far it is walked
            if distance < 0. {
                return None;
            }
            continue;
        }
        let crossing = distance / speed;
        if speed < 0. {
            if crossing > far {
                return None;
            }
            near = near.max(crossing);
        } else {
            if crossing < near {
                return None;
            }
            far = far.min(crossing);
        }
    }

    let at = |t: f64| ((x + t * dx).round() as i32, (y + t * dy).round() as i32);
    let (from, to) = (at(near), at(far));

    // a segment whose ends round onto the same position of the grid draws
    // nothing, which is what thins out a tile of a low zoom level
    (from != to).then_some((from, to))
}

/// Builds the tile that covers the given position of the tile pyramid. The
/// boundary arcs are grouped by the cell they belong to, so that a client can
/// give each cell a color of its own.
fn build_tile(state: &ServerState, level: u32, zoom: u32, x: u32, y: u32) -> Tile {
    let data = &state.tiles;
    let mut geometries: FxHashMap<PartitionID, GeometryEncoder> = FxHashMap::default();

    for arc in &data.boundary {
        let cell = cell_at_level(arc.cell, level);
        if cell == cell_at_level(arc.other, level) {
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
        let geometry = geometries.entry(cell).or_default();
        geometry.move_to(&[source]);
        geometry.line_to(&[target]);
    }

    // the tags of a feature index into the key and value tables of its layer
    let mut features = Vec::with_capacity(geometries.len());
    let mut values = Vec::with_capacity(geometries.len());
    for (cell, geometry) in geometries {
        features.push(Feature {
            id: Some(u64::from(cell.0)),
            r#type: Some(GeomType::Linestring.into()),
            geometry: geometry.build(),
            tags: vec![
                0,
                u32::try_from(values.len()).expect("too many cells on one tile"),
            ],
        });
        values.push(Value {
            uint_value: Some(u64::from(cell.0)),
            ..Default::default()
        });
    }

    // The border nodes go into a layer of their own, so that the client can
    // put a cursor on one and ask what the distances of its cell are.
    let mut node_features = Vec::new();
    let mut node_values = Vec::new();
    for border in &data.border_nodes {
        let position = to_tile_coordinate(border.coordinate, zoom, x, y);
        if !is_within_tile(position) {
            continue;
        }
        // a node of the leaf border only stays on the border while an arc of
        // it still leaves the cell of this level
        let cell = cell_at_level(border.cell, level);
        if !state
            .graph
            .edge_range(border.node)
            .any(|edge| cell_at_level(state.partition_ids[state.graph.target(edge)], level) != cell)
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
            uint_value: Some(u64::from(cell.0)),
            ..Default::default()
        });
        node_values.push(Value {
            uint_value: Some(border.node as u64),
            ..Default::default()
        });
    }

    // Every arc that stays inside its cell, so that a cell reads as a region
    // rather than as the handful of dashes its cut consists of. Only the nodes
    // of the index tile that this one falls into can reach it.
    let mut interior: FxHashMap<PartitionID, GeometryEncoder> = FxHashMap::default();
    if let Some(nodes) = data.nodes_by_tile.get(&index_tile_of(zoom, x, y)) {
        for &node in nodes {
            let cell = cell_at_level(state.partition_ids[node], level);
            let from = to_tile_coordinate(state.coordinates[node], zoom, x, y);
            for edge in state.graph.edge_range(node) {
                let target = state.graph.target(edge);
                // the cut is drawn by the layer above this one
                if cell_at_level(state.partition_ids[target], level) != cell {
                    continue;
                }
                // The graph holds both directions of an arc, so one of them has
                // to be dropped. Dropping by node id alone would drop an arc
                // that leaves the bucket for good, as the bucket holding its
                // other end is not the one being drawn, which tore a seam into
                // every boundary between two buckets. An arc is therefore only
                // dropped when the end it would be drawn from sits in this very
                // bucket. The list is in node order, so it can be searched.
                if target <= node && nodes.binary_search(&target).is_ok() {
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

    let mut interior_features = Vec::with_capacity(interior.len());
    let mut interior_values = Vec::with_capacity(interior.len());
    for (cell, geometry) in interior {
        interior_features.push(Feature {
            id: Some(u64::from(cell.0)),
            r#type: Some(GeomType::Linestring.into()),
            geometry: geometry.build(),
            tags: vec![
                0,
                u32::try_from(interior_values.len()).expect("too many cells on one tile"),
            ],
        });
        interior_values.push(Value {
            uint_value: Some(u64::from(cell.0)),
            ..Default::default()
        });
    }

    Tile {
        layers: vec![
            Layer {
                version: 2,
                name: INTERIOR_LAYER.to_string(),
                extent: Some(TILE_EXTENT),
                features: interior_features,
                keys: vec!["cell".to_string()],
                values: interior_values,
            },
            Layer {
                version: 2,
                name: CELL_LAYER.to_string(),
                extent: Some(TILE_EXTENT),
                features,
                keys: vec!["cell".to_string()],
                values,
            },
            Layer {
                version: 2,
                name: BORDER_LAYER.to_string(),
                extent: Some(TILE_EXTENT),
                features: node_features,
                keys: vec!["cell".to_string(), "node".to_string()],
                values: node_values,
            },
        ],
    }
}

// Tile request handler
/// What a tile request may ask for beyond the tile itself.
#[derive(Deserialize)]
struct TileQuery {
    /// the level of the hierarchy to look at, the leaves when absent
    level: Option<u32>,
}

async fn get_tile(
    path: web::Path<(String, u32, u32, u32)>,
    query: web::Query<TileQuery>,
    state: web::Data<ServerState>,
) -> impl Responder {
    let (tileset_id, zoom, x, y) = path.into_inner();
    let level = state.level_or_leaves(query.level);
    debug!("requesting tile: {tileset_id} at z={zoom} x={x} y={y} on level {level}");

    // Encode the tile to protobuf format
    let mut buf = Vec::new();
    build_tile(&state, level, zoom, x, y)
        .encode(&mut buf)
        .expect("a tile does not fit into its buffer");

    HttpResponse::Ok()
        .content_type("application/x-protobuf")
        .body(buf)
}

async fn index() -> HttpResponse {
    HttpResponse::Ok().body(INDEX_HTML)
}

/// What the partition looks like, so that the client can offer the levels it
/// actually has.
#[derive(Serialize)]
struct Meta {
    max_level: u32,
    cells: usize,
}

async fn get_meta(state: web::Data<ServerState>) -> impl Responder {
    HttpResponse::Ok().json(Meta {
        max_level: state.max_level,
        cells: state.nodes_by_cell.len(),
    })
}

/// Registers the routes of the server. Both the server and the tests below are
/// built from this, so that a test cannot pass against a route that the server
/// does not actually serve.
fn routes(config: &mut web::ServiceConfig) {
    config
        .route("/", web::get().to(index))
        .route("/meta.json", web::get().to(get_meta))
        .route("/node/{node}.json", web::get().to(get_node_distances))
        .route("/{tileset_id}/{zoom}/{x}/{y}.mvt", web::get().to(get_tile));
}

/// The distances between the border nodes of one cell, in the order the border
/// nodes are listed in.
struct CellDistances {
    border_nodes: Vec<NodeID>,
    matrix: Vec<usize>,
}

impl CellDistances {
    fn distance(&self, source: usize, target: usize) -> usize {
        self.matrix[source * self.border_nodes.len() + target]
    }
}

/// Everything the handlers are answered from.
struct ServerState {
    tiles: TileData,
    graph: StaticGraph<usize>,
    coordinates: Vec<FPCoordinate>,
    partition_ids: Vec<PartitionID>,
    /// the nodes of a cell, so that its subgraph can be built on request
    nodes_by_cell: FxHashMap<PartitionID, Vec<NodeID>>,
    /// A cell is customized the first time it is asked about and kept
    /// afterwards. Doing it up front would mean walking every cell of the
    /// input before the first tile can be served.
    tabulated: Mutex<FxHashMap<PartitionID, Arc<CellDistances>>>,
    /// how many cells have been customized so far, and how long that took in
    /// total. The customization runs cell by cell as the cells are asked
    /// about, so the sum is what the whole of it would have cost up front.
    customized_cells: AtomicUsize,
    customization_nanos: AtomicU64,
    /// the deepest level the partition carries, i.e. the level of its leaves
    max_level: u32,
}

impl ServerState {
    fn new(
        graph: StaticGraph<usize>,
        coordinates: Vec<FPCoordinate>,
        partition_ids: Vec<PartitionID>,
    ) -> Self {
        let tiles = TileData::new(&graph, &coordinates, &partition_ids);

        let mut nodes_by_cell: FxHashMap<PartitionID, Vec<NodeID>> = FxHashMap::default();
        for (node, cell) in partition_ids.iter().enumerate() {
            nodes_by_cell.entry(*cell).or_default().push(node);
        }

        let max_level = partition_ids
            .iter()
            .map(|cell| u32::from(cell.level()))
            .max()
            .unwrap_or(0);

        Self {
            tiles,
            graph,
            coordinates,
            partition_ids,
            nodes_by_cell,
            tabulated: Mutex::new(FxHashMap::default()),
            customized_cells: AtomicUsize::new(0),
            customization_nanos: AtomicU64::new(0),
            max_level,
        }
    }

    /// The level a request is answered at: the one that was asked for, held
    /// within what the partition carries, and the leaves when none was asked
    /// for.
    fn level_or_leaves(&self, level: Option<u32>) -> u32 {
        level.unwrap_or(self.max_level).clamp(1, self.max_level)
    }

    /// Hands out the distances of a cell, tabulating them on the first request.
    fn distances_of(&self, cell: PartitionID) -> Option<Arc<CellDistances>> {
        if let Some(distances) = self
            .tabulated
            .lock()
            .expect("the tabulation cache is poisoned")
            .get(&cell)
        {
            return Some(distances.clone());
        }

        let distances = Arc::new(self.tabulate(cell)?);
        self.tabulated
            .lock()
            .expect("the tabulation cache is poisoned")
            .insert(cell, distances.clone());
        Some(distances)
    }

    /// Builds the subgraph of a cell and runs a search from each of its border
    /// nodes. A cell is a small part of the input, so this is quick enough to
    /// happen while a request waits for it.
    fn tabulate(&self, cell: PartitionID) -> Option<CellDistances> {
        let started = Instant::now();
        let nodes = self.nodes_by_cell.get(&cell)?;

        // the border nodes lead the numbering, so that they are the leading
        // rows and columns of the matrix
        let mut border_nodes = Vec::new();
        for &node in nodes {
            if self
                .graph
                .edge_range(node)
                .any(|edge| self.partition_ids[self.graph.target(edge)] != cell)
            {
                border_nodes.push(node);
            }
        }
        if border_nodes.is_empty() {
            debug!("cell {} has no border nodes", cell.0);
            return None;
        }

        // the box the cell covers, in the order a bbox is usually written in
        let mut west = f64::MAX;
        let mut south = f64::MAX;
        let mut east = f64::MIN;
        let mut north = f64::MIN;
        for &node in nodes {
            let (lon, lat) = self.coordinates[node].to_lon_lat_pair();
            west = west.min(lon);
            east = east.max(lon);
            south = south.min(lat);
            north = north.max(lat);
        }
        // TODO: faster hashmap implementation using tabhash or fibonacci hash
        let mut node_map = FxHashMap::default();
        for &node in &border_nodes {
            node_map.insert(node, node_map.len());
        }
        let mut edges = Vec::new();
        for &node in nodes {
            for edge in self.graph.edge_range(node) {
                let target = self.graph.target(edge);
                if self.partition_ids[target] != cell {
                    continue;
                }
                let next = node_map.len();
                let source = *node_map.entry(node).or_insert(next);
                let next = node_map.len();
                let target = *node_map.entry(target).or_insert(next);
                edges.push(InputEdge::new(source, target, *self.graph.data(edge)));
            }
        }

        // TODO: find a way to avoid relocations
        let cell_graph = StaticGraph::new(edges);
        let border = (0..border_nodes.len()).collect::<Vec<_>>();
        let mut matrix = vec![usize::MAX; border_nodes.len() * border_nodes.len()];
        let mut dijkstra = OneToManyDijkstra::new();
        for &source in &border {
            dijkstra.run(&cell_graph, source, &border);
            for &target in &border {
                matrix[source * border_nodes.len() + target] = dijkstra.distance(target);
            }
        }

        // the searches are what the customization of a cell costs, so the
        // clock is read once they are done
        let elapsed = started.elapsed();
        let cells = self.customized_cells.fetch_add(1, Ordering::Relaxed) + 1;
        let total = Duration::from_nanos(
            self.customization_nanos
                .fetch_add(elapsed.as_nanos() as u64, Ordering::Relaxed),
        ) + elapsed;
        info!(
            "customized cell {} on level {} in {elapsed:.1?}: {} nodes, {} of them on the border, bbox {west:.6},{south:.6},{east:.6},{north:.6}",
            cell.0,
            cell.level(),
            nodes.len(),
            border_nodes.len()
        );
        info!("customization so far: {cells} cells in {total:.1?}");

        Some(CellDistances {
            border_nodes,
            matrix,
        })
    }
}

/// One row of the answer a popup shows: a border node of the same cell and how
/// far it is from the one that was asked about.
#[derive(Serialize)]
struct Reachable {
    node: NodeID,
    coordinate: [f64; 2],
    distance: usize,
}

/// The answer a popup shows.
#[derive(Serialize)]
struct NodeDistances {
    node: NodeID,
    cell: u32,
    coordinate: [f64; 2],
    border_node_count: usize,
    /// the closest border nodes, at most [`POPUP_DISTANCES`] of them
    nearest: Vec<Reachable>,
    /// border nodes of the cell that this one cannot reach at all
    unreachable_count: usize,
}

/// Answers what the distances from one border node into its cell are. The
/// client asks for this when the cursor comes to rest on a node.
async fn get_node_distances(
    path: web::Path<NodeID>,
    state: web::Data<ServerState>,
) -> impl Responder {
    let node = path.into_inner();
    let Some(&cell) = state.partition_ids.get(node) else {
        return HttpResponse::NotFound().body(format!("no node {node}"));
    };
    let Some(distances) = state.distances_of(cell) else {
        return HttpResponse::NotFound().body(format!("cell {} has no border", cell.0));
    };
    let Some(source) = distances
        .border_nodes
        .iter()
        .position(|&border| border == node)
    else {
        return HttpResponse::NotFound().body(format!("node {node} is not on the border"));
    };

    let coordinate = |node: NodeID| {
        let (lon, lat) = state.coordinates[node].to_lon_lat_pair();
        [lon, lat]
    };

    let mut nearest = distances
        .border_nodes
        .iter()
        .enumerate()
        .filter(|&(target, _)| target != source)
        .map(|(target, &node)| Reachable {
            node,
            coordinate: coordinate(node),
            distance: distances.distance(source, target),
        })
        .collect::<Vec<_>>();
    let unreachable_count = nearest
        .iter()
        .filter(|reachable| reachable.distance == usize::MAX)
        .count();
    nearest.retain(|reachable| reachable.distance != usize::MAX);
    nearest.sort_unstable_by_key(|reachable| reachable.distance);
    nearest.truncate(POPUP_DISTANCES);

    HttpResponse::Ok().json(NodeDistances {
        node,
        cell: cell.0,
        coordinate: coordinate(node),
        border_node_count: distances.border_nodes.len(),
        nearest,
        unreachable_count,
    })
}

#[actix_web::main]
async fn main() -> Result<(), Box<dyn Error>> {
    Builder::from_env(Env::default().default_filter_or("info")).init();

    println!(r#" __   __                   _                     "#);
    println!(r#" \ \ / /   ___     __     | |_     ___      _ _  "#);
    println!(r#"  \ V /   / -_)   / _|    |  _|   / _ \    | '_| "#);
    println!(r#"  _\_/_   \___|   \__|_   _\__|   \___/   _|_|_  "#);
    println!(r#"_| """"|_|"""""|_|"""""|_|"""""|_|"""""|_|"""""| "#);
    println!(r#""`-0-0-'"`-0-0-'"`-0-0-'"`-0-0-'"`-0-0-'"`-0-0-' "#);
    println!("build: {}", env!("GIT_HASH"));
    // parse and print command line parameters
    let args = <Arguments as clap::Parser>::parse();

    let edges = io::read_vec_from_file::<InputEdge<usize>>(&args.graph);
    info!("loaded {} graph edges", edges.len());

    let partition_ids = io::read_vec_from_file::<PartitionID>(&args.assignment);
    info!("loaded {} partition ids", partition_ids.len());

    let coordinates = io::read_vec_from_file::<FPCoordinate>(&args.coordinates);
    info!("loaded {} coordinates", coordinates.len());

    let static_graph = static_graph::StaticGraph::new(edges);
    info!(
        "loaded static graph with {} nodes and {} edges",
        static_graph.number_of_nodes(),
        static_graph.number_of_edges()
    );

    // the coordinate the debug output below reports on
    let probe = FPCoordinate::new_from_lat_lon(50.20731, 8.57747);
    let nearest = coordinates
        .iter()
        .zip(&partition_ids)
        .min_by(|(left, _), (right, _)| {
            left.distance_to(&probe)
                .total_cmp(&right.distance_to(&probe))
        });
    if let Some((coordinate, cell)) = nearest {
        info!(
            "closest node to {probe}: {coordinate} in cell {}, {:.3} km away",
            cell.0,
            coordinate.distance_to(&probe)
        );
    }

    let state = web::Data::new(ServerState::new(static_graph, coordinates, partition_ids));
    info!(
        "{} arcs on the boundary between {} cells, on {} border nodes",
        state.tiles.boundary.len(),
        state.nodes_by_cell.len(),
        state.tiles.border_nodes.len()
    );
    if state.tiles.boundary.is_empty() {
        warn!("the partition has no boundary, so the tiles will be empty");
    }

    let address = args.listen.clone();
    info!("serving on http://{address}");
    info!("press Ctrl+C to stop the server");

    HttpServer::new(move || App::new().app_data(state.clone()).configure(routes))
        .bind(address)?
        .run()
        .await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    // `test` is aliased, as importing it plainly would shadow the `#[test]`
    // attribute with the actix macro of the same name
    use actix_web::{App, body::to_bytes, http::StatusCode, test as actix_test};
    use toolbox_rs::mvt::{LINE_TO, MOVE_TO, command_and_count};

    /// Two cells that meet in the middle of a tile of Frankfurt, with one arc
    /// crossing between them.
    const ZOOM: u32 = 14;
    /// the level the ids of the fixtures sit on
    const LEAF_LEVEL: u32 = 2;
    const LAT: f64 = 50.20731;
    const LON: f64 = 8.57747;

    /// A state carrying one arc between two cells, for the handlers that draw
    /// tiles. The graph holds that arc, as whether a node is still on the
    /// border of the level being looked at is read off it.
    fn state_with_one_arc() -> ServerState {
        let edges = vec![InputEdge::new(0, 1, 1_usize), InputEdge::new(1, 0, 1_usize)];
        ServerState {
            tiles: data_with_one_arc(),
            graph: StaticGraph::new(edges),
            coordinates: vec![
                FPCoordinate::new_from_lat_lon(LAT, LON),
                FPCoordinate::new_from_lat_lon(LAT + 0.002, LON + 0.002),
            ],
            partition_ids: vec![PartitionID::new(7), PartitionID::new(6)],
            nodes_by_cell: FxHashMap::default(),
            tabulated: Mutex::new(FxHashMap::default()),
            customized_cells: AtomicUsize::new(0),
            customization_nanos: AtomicU64::new(0),
            max_level: LEAF_LEVEL,
        }
    }

    fn cell_layer(tile: &Tile) -> &Layer {
        tile.layers
            .iter()
            .find(|layer| layer.name == CELL_LAYER)
            .expect("no cell layer")
    }

    fn tile_of_probe() -> (u32, u32) {
        use toolbox_rs::vector_tile::coordinate_to_tile_number;
        use toolbox_rs::wgs84::FloatCoordinate;
        coordinate_to_tile_number(
            FloatCoordinate {
                lat: FloatLatitude(LAT),
                lon: FloatLongitude(LON),
            },
            ZOOM,
        )
    }

    fn data_with_one_arc() -> TileData {
        TileData {
            boundary: vec![BoundaryArc {
                source: FPCoordinate::new_from_lat_lon(LAT, LON),
                target: FPCoordinate::new_from_lat_lon(LAT + 0.002, LON + 0.002),
                cell: PartitionID::new(7),
                other: PartitionID::new(6),
            }],
            border_nodes: vec![BorderNode {
                coordinate: FPCoordinate::new_from_lat_lon(LAT, LON),
                cell: PartitionID::new(7),
                node: 0,
            }],
            nodes_by_tile: FxHashMap::default(),
        }
    }

    /// Two cells of two nodes each, joined by a single arc between them, so
    /// that every node is a border node.
    ///     0 - 1 === 2 - 3
    fn state_of_two_cells() -> ServerState {
        let edges = vec![
            InputEdge::new(0, 1, 3_usize),
            InputEdge::new(1, 0, 3_usize),
            InputEdge::new(1, 2, 7_usize),
            InputEdge::new(2, 1, 7_usize),
            InputEdge::new(2, 3, 5_usize),
            InputEdge::new(3, 2, 5_usize),
        ];
        let coordinates = (0..4)
            .map(|index| FPCoordinate::new_from_lat_lon(LAT, LON + 0.001 * f64::from(index)))
            .collect::<Vec<_>>();
        let partition_ids = vec![
            PartitionID::new(1),
            PartitionID::new(1),
            PartitionID::new(2),
            PartitionID::new(2),
        ];
        ServerState::new(StaticGraph::new(edges), coordinates, partition_ids)
    }

    #[test]
    fn boundary_holds_the_arcs_that_leave_their_cell() {
        // 0 - 1 - 2, with the cut between node 1 and node 2
        let edges = vec![
            InputEdge::new(0, 1, 1_usize),
            InputEdge::new(1, 0, 1_usize),
            InputEdge::new(1, 2, 1_usize),
            InputEdge::new(2, 1, 1_usize),
        ];
        let graph = StaticGraph::new(edges);
        let coordinates = vec![
            FPCoordinate::new_from_lat_lon(50.0, 8.0),
            FPCoordinate::new_from_lat_lon(50.1, 8.1),
            FPCoordinate::new_from_lat_lon(50.2, 8.2),
        ];
        let partition_ids = vec![
            PartitionID::new(1),
            PartitionID::new(1),
            PartitionID::new(2),
        ];

        let data = TileData::new(&graph, &coordinates, &partition_ids);

        // only the arc between the cells is on the boundary, and it is taken
        // once although the graph holds both of its directions
        assert_eq!(data.boundary.len(), 1);
        assert_eq!(data.boundary[0].source, coordinates[1]);
        assert_eq!(data.boundary[0].target, coordinates[2]);
        assert_eq!(data.boundary[0].cell, PartitionID::new(1));
    }

    #[test]
    fn a_partition_of_one_cell_has_no_boundary() {
        let edges = vec![InputEdge::new(0, 1, 1_usize), InputEdge::new(1, 0, 1_usize)];
        let graph = StaticGraph::new(edges);
        let coordinates = vec![
            FPCoordinate::new_from_lat_lon(50.0, 8.0),
            FPCoordinate::new_from_lat_lon(50.1, 8.1),
        ];
        let partition_ids = vec![PartitionID::new(1), PartitionID::new(1)];

        assert!(
            TileData::new(&graph, &coordinates, &partition_ids)
                .boundary
                .is_empty()
        );
    }

    #[test]
    fn the_corner_of_a_tile_sits_at_its_origin() {
        let (x, y) = tile_of_probe();
        let bounds = toolbox_rs::vector_tile::get_tile_bounds(ZOOM, x, y);
        let corner = FPCoordinate::new_from_lat_lon(bounds.min_lat.0, bounds.min_lon.0);

        let (tile_x, tile_y) = to_tile_coordinate(corner, ZOOM, x, y);
        // the north west corner of a tile is the origin of its grid
        assert!(tile_x.abs() <= 1, "x of the corner is {tile_x}");
        assert!(tile_y.abs() <= 1, "y of the corner is {tile_y}");
    }

    #[test]
    fn a_coordinate_of_the_tile_lands_inside_its_extent() {
        let (x, y) = tile_of_probe();
        let (tile_x, tile_y) =
            to_tile_coordinate(FPCoordinate::new_from_lat_lon(LAT, LON), ZOOM, x, y);

        assert!((0..TILE_EXTENT as i32).contains(&tile_x), "x is {tile_x}");
        assert!((0..TILE_EXTENT as i32).contains(&tile_y), "y is {tile_y}");
    }

    #[test]
    fn a_segment_that_draws_nothing_is_dropped() {
        assert!(clip_to_tile((10, 10), (10, 10)).is_none());
        assert!(clip_to_tile((10, 10), (11, 10)).is_some());
    }

    #[test]
    fn segments_off_one_side_are_dropped() {
        let outside = TILE_EXTENT as i32 * 4;
        assert!(clip_to_tile((-outside, 10), (-outside - 5, 10)).is_none());
        assert!(clip_to_tile((outside, 10), (outside + 5, 10)).is_none());
        assert!(clip_to_tile((10, -outside), (10, -outside - 5)).is_none());
        assert!(clip_to_tile((10, outside), (10, outside + 5)).is_none());
    }

    /// The reason the clipping exists: an arc far longer than the tile has to
    /// arrive cut down to the part of it that lies on the tile, or a reader
    /// drops it for reaching past the buffer it keeps around the tile.
    #[test]
    fn a_segment_across_the_tile_is_cut_to_it() {
        let outside = TILE_EXTENT as i32 * 4;
        let extent = TILE_EXTENT as i32;
        let margin = TILE_MARGIN as i32;

        let (from, to) = clip_to_tile((-outside, 2048), (outside, 2048)).expect("crosses the tile");
        assert_eq!(from, (-margin, 2048));
        assert_eq!(to, (extent + margin, 2048));

        let (from, to) = clip_to_tile((2048, -outside), (2048, outside)).expect("crosses the tile");
        assert_eq!(from, (2048, -margin));
        assert_eq!(to, (2048, extent + margin));
    }

    #[test]
    fn only_the_end_that_hangs_over_is_cut() {
        let extent = TILE_EXTENT as i32;
        let margin = TILE_MARGIN as i32;
        // starts on the tile and runs far off to the east
        let (from, to) =
            clip_to_tile((1000, 1000), (extent * 5, 1000)).expect("starts on the tile");
        assert_eq!(from, (1000, 1000), "the end on the tile is left alone");
        assert_eq!(to, (extent + margin, 1000), "the end off it is cut back");
    }

    #[test]
    fn a_segment_within_the_tile_is_left_alone() {
        let segment = ((100, 200), (3000, 3500));
        assert_eq!(clip_to_tile(segment.0, segment.1), Some(segment));
    }

    #[test]
    fn a_diagonal_keeps_its_direction_when_cut() {
        let extent = TILE_EXTENT as i32;
        let (from, to) = clip_to_tile((-extent, -extent), (2 * extent, 2 * extent))
            .expect("crosses the tile diagonally");
        // the segment runs at 45 degrees, so the cut ends do too
        assert_eq!(from.0, from.1);
        assert_eq!(to.0, to.1);
        assert!(from.0 < to.0);
    }

    /// A clipped segment has to stay within the grid a reader accepts.
    #[test]
    fn what_is_handed_over_stays_within_the_margin() {
        let outside = TILE_EXTENT as i32 * 9;
        let bound = TILE_EXTENT as i32 + TILE_MARGIN as i32;
        for segment in [
            ((-outside, -outside), (outside, outside)),
            ((-outside, 2048), (outside, 2048)),
            ((2048, outside), (2048, -outside)),
            ((-outside, 4095), (outside, 0)),
        ] {
            let (from, to) = clip_to_tile(segment.0, segment.1).expect("crosses the tile");
            for point in [from, to] {
                assert!(point.0 >= -bound && point.0 <= bound, "x of {point:?}");
                assert!(point.1 >= -bound && point.1 <= bound, "y of {point:?}");
            }
        }
    }

    #[test]
    fn a_cell_becomes_a_feature_of_the_tile() {
        let (x, y) = tile_of_probe();
        let tile = build_tile(&state_with_one_arc(), LEAF_LEVEL, ZOOM, x, y);

        assert_eq!(
            tile.layers.len(),
            3,
            "one layer of interior arcs, one of the cut, one of nodes"
        );
        let layer = cell_layer(&tile);
        assert_eq!(layer.name, CELL_LAYER);
        assert_eq!(layer.extent, Some(TILE_EXTENT));
        assert_eq!(layer.features.len(), 1);
        assert_eq!(layer.keys, vec!["cell".to_string()]);
        assert_eq!(layer.values[0].uint_value, Some(7));
        assert_eq!(
            layer.features[0].r#type,
            Some(i32::from(GeomType::Linestring))
        );
    }

    #[test]
    fn arcs_of_one_cell_share_a_feature() {
        let mut data = data_with_one_arc();
        // a second arc of the same cell, next to the first one
        data.boundary.push(BoundaryArc {
            source: FPCoordinate::new_from_lat_lon(LAT + 0.0005, LON),
            target: FPCoordinate::new_from_lat_lon(LAT + 0.0025, LON + 0.002),
            cell: PartitionID::new(7),
            other: PartitionID::new(6),
        });
        // and one of another cell
        data.boundary.push(BoundaryArc {
            source: FPCoordinate::new_from_lat_lon(LAT + 0.001, LON),
            target: FPCoordinate::new_from_lat_lon(LAT + 0.003, LON + 0.002),
            cell: PartitionID::new(6),
            other: PartitionID::new(5),
        });

        let (x, y) = tile_of_probe();
        let mut state = state_with_one_arc();
        state.tiles = data;
        let tile = build_tile(&state, LEAF_LEVEL, ZOOM, x, y);
        let layer = cell_layer(&tile);

        assert_eq!(layer.features.len(), 2, "one feature per cell");
        let mut cells = layer
            .values
            .iter()
            .map(|value| value.uint_value.expect("cell id is not a number"))
            .collect::<Vec<_>>();
        cells.sort_unstable();
        assert_eq!(cells, vec![6, 7]);

        // the cell of two arcs draws two line strings within one feature
        let two = layer
            .features
            .iter()
            .find(|feature| feature.id == Some(7))
            .expect("cell 7 is missing");
        let move_tos = commands_of(&two.geometry)
            .iter()
            .filter(|&&(id, _)| id == MOVE_TO)
            .count();
        assert_eq!(move_tos, 2);
    }

    #[test]
    fn a_tile_elsewhere_stays_empty() {
        // the same data, but a tile on the other side of the planet
        let tile = build_tile(&state_with_one_arc(), LEAF_LEVEL, ZOOM, 1, 1);
        assert!(cell_layer(&tile).features.is_empty());
        assert!(cell_layer(&tile).values.is_empty());
    }

    /// Walks a geometry and hands back the commands it is made of, checking
    /// that each one is followed by the number of parameters it announces.
    fn commands_of(geometry: &[u32]) -> Vec<(u32, u32)> {
        let mut commands = Vec::new();
        let mut index = 0;
        while index < geometry.len() {
            let (id, count) = command_and_count(geometry[index]);
            assert!(count > 0, "a command that repeats zero times is rejected");
            let parameters = match id {
                MOVE_TO | LINE_TO => 2 * count as usize,
                other => panic!("unexpected command id {other}"),
            };
            assert!(
                index + 1 + parameters <= geometry.len(),
                "command runs past the end of the geometry"
            );
            commands.push((id, count));
            index += 1 + parameters;
        }
        commands
    }

    #[test]
    fn geometry_is_a_well_formed_command_sequence() {
        let (x, y) = tile_of_probe();
        let tile = build_tile(&state_with_one_arc(), LEAF_LEVEL, ZOOM, x, y);
        // a single arc is one MoveTo onto its start and one LineTo to its end
        assert_eq!(
            commands_of(&cell_layer(&tile).features[0].geometry),
            vec![(MOVE_TO, 1), (LINE_TO, 1)]
        );
    }

    #[test]
    fn tags_stay_within_keys_and_values() {
        let (x, y) = tile_of_probe();
        let tile = build_tile(&state_with_one_arc(), LEAF_LEVEL, ZOOM, x, y);
        for layer in &tile.layers {
            for feature in &layer.features {
                assert_eq!(feature.tags.len() % 2, 0, "tags are pairs");
                for pair in feature.tags.chunks_exact(2) {
                    assert!((pair[0] as usize) < layer.keys.len(), "key out of range");
                    assert!(
                        (pair[1] as usize) < layer.values.len(),
                        "value out of range"
                    );
                }
            }
        }
    }

    #[actix_web::test]
    async fn index_is_served() {
        let app = actix_test::init_service(App::new().configure(routes)).await;
        let request = actix_test::TestRequest::get().uri("/").to_request();
        let response = actix_test::call_service(&app, request).await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body()).await.expect("empty body");
        let body = String::from_utf8_lossy(&body);
        assert!(body.contains("<html"));
        // The client asks for the layer the server actually serves, and it
        // builds the URL from the origin. A relative one would be handed to a
        // worker, which has no document to resolve it against and rejects it
        // with "Failed to parse URL".
        assert!(
            body.contains("location.origin + \"/cells/{z}/{x}/{y}.mvt\""),
            "the tile URL has to be absolute"
        );
        assert!(
            !body.contains("[\"/cells/"),
            "a relative tile URL cannot be fetched from a worker"
        );
        // and keeps the position of the map in the URL
        assert!(body.contains("hash: true"));
    }

    #[actix_web::test]
    async fn served_tile_decodes_again() {
        let (x, y) = tile_of_probe();
        let app = actix_test::init_service(
            App::new()
                .app_data(web::Data::new(state_with_one_arc()))
                .configure(routes),
        )
        .await;
        let request = actix_test::TestRequest::get()
            .uri(&format!("/cells/{ZOOM}/{x}/{y}.mvt"))
            .to_request();
        let response = actix_test::call_service(&app, request).await;

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get("content-type")
                .expect("no content type"),
            "application/x-protobuf"
        );

        let body = to_bytes(response.into_body()).await.expect("empty body");
        let tile = Tile::decode(&body[..]).expect("served tile does not decode");
        assert_eq!(cell_layer(&tile).features.len(), 1);
    }

    #[test]
    fn border_nodes_are_the_nodes_an_arc_leaves_on() {
        let state = state_of_two_cells();
        let mut nodes = state
            .tiles
            .border_nodes
            .iter()
            .map(|border| border.node)
            .collect::<Vec<_>>();
        nodes.sort_unstable();
        // only the two nodes of the arc between the cells sit on a border
        assert_eq!(nodes, vec![1, 2]);
    }

    #[test]
    fn distances_within_a_cell_are_tabulated_on_request() {
        let state = state_of_two_cells();
        let distances = state
            .distances_of(PartitionID::new(1))
            .expect("cell 1 has a border");

        // node 1 is the only border node of its cell, so the matrix is 1x1 and
        // the distance to itself is zero
        assert_eq!(distances.border_nodes, vec![1]);
        assert_eq!(distances.distance(0, 0), 0);
    }

    #[test]
    fn a_tabulated_cell_is_kept() {
        let state = state_of_two_cells();
        let first = state.distances_of(PartitionID::new(1)).expect("no cell 1");
        let second = state.distances_of(PartitionID::new(1)).expect("no cell 1");
        // the second request is answered from the same tabulation
        assert!(Arc::ptr_eq(&first, &second));
    }

    #[test]
    fn a_cell_without_a_border_is_not_tabulated() {
        // one cell holding the whole graph, so no arc ever leaves it
        let edges = vec![InputEdge::new(0, 1, 1_usize), InputEdge::new(1, 0, 1_usize)];
        let coordinates = vec![
            FPCoordinate::new_from_lat_lon(LAT, LON),
            FPCoordinate::new_from_lat_lon(LAT, LON + 0.001),
        ];
        let partition_ids = vec![PartitionID::new(1), PartitionID::new(1)];
        let state = ServerState::new(StaticGraph::new(edges), coordinates, partition_ids);

        assert!(state.distances_of(PartitionID::new(1)).is_none());
    }

    #[test]
    fn border_nodes_reach_the_tile() {
        let (x, y) = tile_of_probe();
        let tile = build_tile(&state_with_one_arc(), LEAF_LEVEL, ZOOM, x, y);

        let layer = tile
            .layers
            .iter()
            .find(|layer| layer.name == BORDER_LAYER)
            .expect("no border node layer");
        assert_eq!(layer.features.len(), 1);
        assert_eq!(layer.features[0].id, Some(0));
        assert_eq!(layer.features[0].r#type, Some(i32::from(GeomType::Point)));
        assert_eq!(layer.keys, vec!["cell".to_string(), "node".to_string()]);

        // the tags name the cell and the node, which is what the popup asks with
        assert_eq!(layer.features[0].tags, vec![0, 0, 1, 1]);
        assert_eq!(layer.values[0].uint_value, Some(7));
        assert_eq!(layer.values[1].uint_value, Some(0));
    }

    #[test]
    fn border_nodes_of_another_tile_are_left_out() {
        let tile = build_tile(&state_with_one_arc(), LEAF_LEVEL, ZOOM, 1, 1);
        let layer = tile
            .layers
            .iter()
            .find(|layer| layer.name == BORDER_LAYER)
            .expect("no border node layer");
        assert!(layer.features.is_empty());
    }

    #[actix_web::test]
    async fn distances_of_a_border_node_are_served() {
        let app = actix_test::init_service(
            App::new()
                .app_data(web::Data::new(state_of_two_cells()))
                .configure(routes),
        )
        .await;
        let request = actix_test::TestRequest::get()
            .uri("/node/1.json")
            .to_request();
        let response = actix_test::call_service(&app, request).await;
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body()).await.expect("empty body");
        let answer = String::from_utf8_lossy(&body);
        assert!(answer.contains("\"node\":1"), "{answer}");
        assert!(answer.contains("\"cell\":1"), "{answer}");
        assert!(answer.contains("\"border_node_count\":1"), "{answer}");
    }

    #[actix_web::test]
    async fn a_node_off_the_border_is_reported() {
        let app = actix_test::init_service(
            App::new()
                .app_data(web::Data::new(state_of_two_cells()))
                .configure(routes),
        )
        .await;
        // node 0 lies inside its cell, so it has no row of distances
        let request = actix_test::TestRequest::get()
            .uri("/node/0.json")
            .to_request();
        let response = actix_test::call_service(&app, request).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[actix_web::test]
    async fn a_node_that_does_not_exist_is_reported() {
        let app = actix_test::init_service(
            App::new()
                .app_data(web::Data::new(state_of_two_cells()))
                .configure(routes),
        )
        .await;
        let request = actix_test::TestRequest::get()
            .uri("/node/99999.json")
            .to_request();
        let response = actix_test::call_service(&app, request).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    /// An arc that leaves the bucket of the tile has to be drawn all the same.
    /// Dropping one direction of it by node id alone dropped it entirely
    /// whenever its other end sat in a neighbouring bucket, which tore a seam
    /// into every boundary between two buckets.
    #[test]
    fn an_arc_that_leaves_the_bucket_is_still_drawn() {
        // two nodes far enough apart to land in different tiles of the index,
        // with the higher id to the west so that the lower one is outside
        let west = FPCoordinate::new_from_lat_lon(LAT, 8.80);
        let east = FPCoordinate::new_from_lat_lon(LAT, 8.95);
        let edges = vec![InputEdge::new(0, 1, 1_usize), InputEdge::new(1, 0, 1_usize)];
        // node 0 is east, node 1 is west, so the arc runs from the higher id
        let coordinates = vec![east, west];
        let partition_ids = vec![PartitionID::new(4), PartitionID::new(4)];
        let state = ServerState::new(StaticGraph::new(edges), coordinates, partition_ids);

        let buckets = state.tiles.nodes_by_tile.len();
        assert_eq!(
            buckets, 2,
            "the two nodes have to land in different buckets"
        );

        // both tiles have to carry the arc between them
        for coordinate in [west, east] {
            let (lon, lat) = coordinate.to_lon_lat_pair();
            let (x, y) = coordinate_to_tile_number(
                FloatCoordinate {
                    lat: FloatLatitude(lat),
                    lon: FloatLongitude(lon),
                },
                INDEX_ZOOM,
            );
            let tile = build_tile(&state, LEAF_LEVEL, INDEX_ZOOM, x, y);
            let interior = tile
                .layers
                .iter()
                .find(|layer| layer.name == INTERIOR_LAYER)
                .expect("no interior layer");
            assert_eq!(
                interior.features.len(),
                1,
                "the arc is missing from the tile at {lon}"
            );
        }
    }

    #[test]
    fn customization_is_counted_once_per_cell() {
        let state = state_of_two_cells();
        assert_eq!(state.customized_cells.load(Ordering::Relaxed), 0);

        state.distances_of(PartitionID::new(1)).expect("no cell 1");
        assert_eq!(state.customized_cells.load(Ordering::Relaxed), 1);
        assert!(state.customization_nanos.load(Ordering::Relaxed) > 0);

        // the second cell adds to the tally
        state.distances_of(PartitionID::new(2)).expect("no cell 2");
        assert_eq!(state.customized_cells.load(Ordering::Relaxed), 2);

        // a cell that is answered from the tabulation of an earlier request
        // was not customized again
        let after = state.customization_nanos.load(Ordering::Relaxed);
        state.distances_of(PartitionID::new(1)).expect("no cell 1");
        assert_eq!(state.customized_cells.load(Ordering::Relaxed), 2);
        assert_eq!(state.customization_nanos.load(Ordering::Relaxed), after);
    }

    #[test]
    fn a_cell_walks_up_the_hierarchy() {
        // 0b1101 on level 3, so one step up is 0b110 and two are 0b11
        let leaf = PartitionID::new(0b1101);
        assert_eq!(leaf.level(), 3);
        assert_eq!(cell_at_level(leaf, 3), leaf);
        assert_eq!(cell_at_level(leaf, 2), PartitionID::new(0b110));
        assert_eq!(cell_at_level(leaf, 1), PartitionID::new(0b11));
        assert_eq!(cell_at_level(leaf, 0), PartitionID::root());
        // a level below the root cannot be walked to
        assert_eq!(cell_at_level(leaf, 9), leaf);
    }

    #[test]
    fn siblings_meet_one_level_up() {
        let (left, right) = PartitionID::new(0b110).children();
        assert_ne!(cell_at_level(left, 3), cell_at_level(right, 3));
        assert_eq!(cell_at_level(left, 2), cell_at_level(right, 2));
    }

    /// Two cells that are siblings share a parent, so the arc between them is
    /// a cut at the level of the leaves and interior one level up.
    #[test]
    fn a_cut_between_siblings_closes_one_level_up() {
        let state = state_with_one_arc();
        let cut_at = |level: u32| {
            let (x, y) = tile_of_probe();
            let tile = build_tile(&state, level, ZOOM, x, y);
            let cut = cell_layer(&tile).features.len();
            let interior = tile
                .layers
                .iter()
                .find(|layer| layer.name == INTERIOR_LAYER)
                .expect("no interior layer")
                .features
                .len();
            (cut, interior)
        };

        // cells 7 and 6 are the children of 3, so the arc separates them at
        // the level of the leaves
        assert_eq!(cut_at(LEAF_LEVEL).0, 1, "the arc is a cut at the leaves");
        // one level up both ends fall into cell 3 and it separates nothing
        assert_eq!(
            cut_at(LEAF_LEVEL - 1).0,
            0,
            "the arc still cuts one level up"
        );
    }

    #[test]
    fn the_level_of_a_request_is_held_within_the_partition() {
        let state = state_of_two_cells();
        assert_eq!(state.max_level, 1);
        // nothing asked for means the leaves
        assert_eq!(state.level_or_leaves(None), 1);
        // and anything past either end is pulled back onto the partition
        assert_eq!(state.level_or_leaves(Some(0)), 1);
        assert_eq!(state.level_or_leaves(Some(99)), 1);
    }

    #[actix_web::test]
    async fn the_levels_of_the_partition_are_served() {
        let app = actix_test::init_service(
            App::new()
                .app_data(web::Data::new(state_of_two_cells()))
                .configure(routes),
        )
        .await;
        let request = actix_test::TestRequest::get()
            .uri("/meta.json")
            .to_request();
        let response = actix_test::call_service(&app, request).await;
        assert_eq!(response.status(), StatusCode::OK);

        let body = to_bytes(response.into_body()).await.expect("empty body");
        let answer = String::from_utf8_lossy(&body);
        assert!(answer.contains("\"max_level\":1"), "{answer}");
        assert!(answer.contains("\"cells\":2"), "{answer}");
    }

    #[actix_web::test]
    async fn a_tile_can_be_asked_for_at_a_level() {
        let (x, y) = tile_of_probe();
        let app = actix_test::init_service(
            App::new()
                .app_data(web::Data::new(state_with_one_arc()))
                .configure(routes),
        )
        .await;
        for level in [LEAF_LEVEL, LEAF_LEVEL - 1] {
            let request = actix_test::TestRequest::get()
                .uri(&format!("/cells/{ZOOM}/{x}/{y}.mvt?level={level}"))
                .to_request();
            let response = actix_test::call_service(&app, request).await;
            assert_eq!(response.status(), StatusCode::OK, "level {level}");
            let body = to_bytes(response.into_body()).await.expect("empty body");
            Tile::decode(&body[..]).expect("served tile does not decode");
        }
    }
}
