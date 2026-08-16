//! What the server answers, and on which path.

use actix_web::{HttpResponse, Responder, web};
use prost::Message as _;
use serde::{Deserialize, Serialize};

use log::debug;
use toolbox_rs::{graph::NodeID, level_directory::CellId};

use crate::state::ServerState;
use crate::tile_builder::build_tile;

pub(crate) const INDEX_HTML: &str = include_str!("../client/index.html");

/// How many distances a popup is handed. A cell can have far more border nodes
/// than fit on a screen, so the closest ones are handed over and the rest is
/// reported as a count.
pub(crate) const POPUP_DISTANCES: usize = 12;

// Tile request handler
/// What a tile request may ask for beyond the tile itself.
#[derive(Deserialize)]
pub struct TileQuery {
    /// the level of the hierarchy to look at, the leaves when absent
    level: Option<u32>,
}

pub async fn get_tile(
    path: web::Path<(String, u32, u32, u32)>,
    query: web::Query<TileQuery>,
    state: web::Data<ServerState>,
) -> impl Responder {
    let (tileset_id, zoom, x, y) = path.into_inner();
    // The level rides in the path rather than in a query, as a query is part of
    // the url to this server and not to a tile reader: maplibre drops it when
    // it works out which tile to ask for, so the slider moved and nothing did.
    // In the path it is part of what names the tile, which is what it is.
    let level = tileset_id
        .strip_prefix("cells")
        .and_then(|rest| rest.strip_prefix('-'))
        .and_then(|level| level.parse::<u32>().ok())
        .map_or_else(
            || state.level_or_finest(query.level),
            |level| state.level_or_finest(Some(level)),
        );
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

pub async fn index() -> HttpResponse {
    HttpResponse::Ok().body(INDEX_HTML)
}

/// What the partition looks like, so that the client can offer the levels it
/// actually has.
#[derive(Serialize)]
struct Meta {
    max_level: u32,
    cells: usize,
}

pub async fn get_meta(state: web::Data<ServerState>) -> impl Responder {
    HttpResponse::Ok().json(Meta {
        max_level: state.max_level,
        cells: state.directory().cells_on_level(0),
    })
}

/// Registers the routes of the server. Both the server and the tests below are
/// built from this, so that a test cannot pass against a route that the server
/// does not actually serve.
pub fn routes(config: &mut web::ServiceConfig) {
    config
        .route("/", web::get().to(index))
        .route("/meta.json", web::get().to(get_meta))
        .route("/node/{node}.json", web::get().to(get_node_distances))
        .route("/{tileset_id}/{zoom}/{x}/{y}.mvt", web::get().to(get_tile));
}

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
    cell: CellId,
    coordinate: [f64; 2],
    border_node_count: usize,
    /// the closest border nodes, at most [`POPUP_DISTANCES`] of them
    nearest: Vec<Reachable>,
    /// border nodes of the cell that this one cannot reach at all
    unreachable_count: usize,
}

/// Answers what the distances from one border node into its cell are. The
/// client asks for this when the cursor comes to rest on a node.
pub async fn get_node_distances(
    path: web::Path<NodeID>,
    query: web::Query<TileQuery>,
    state: web::Data<ServerState>,
) -> impl Responder {
    let node = path.into_inner();
    if node >= state.directory().number_of_nodes() {
        return HttpResponse::NotFound().body(format!("no node {node}"));
    }
    let level = state.level_or_finest(query.level) as usize;
    let cell = state.directory().cell_of(node, level);
    let Some(distances) = state.distances_of(level, cell) else {
        return HttpResponse::NotFound()
            .body(format!("cell {cell} of level {level} has no border"));
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
        cell,
        coordinate: coordinate(node),
        border_node_count: distances.border_nodes.len(),
        nearest,
        unreachable_count,
    })
}
