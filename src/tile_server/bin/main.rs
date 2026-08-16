mod command_line;
mod handlers;
mod state;
mod tile_builder;
mod tile_index;

use actix_web::{App, HttpServer, web};
use command_line::Arguments;
use env_logger::{Builder, Env};
use log::{info, warn};
use prost::Message;
use std::{error::Error, time::Instant};

use handlers::routes;
use state::ServerState;
use tile_builder::build_tile;
use tile_index::index_tiles_of;
use toolbox_rs::{
    edge::InputEdge,
    geometry::FPCoordinate,
    graph::Graph,
    io,
    level_directory::LevelDirectory,
    static_graph::{self},
    vector_tile::coordinate_to_tile_number,
    wgs84::{FloatCoordinate, FloatLatitude, FloatLongitude},
};

// Include the generated protobuf code
include!(concat!(env!("OUT_DIR"), "/vector_tile.rs"));

/// Times the tile builder over a sweep of zoom levels and partition levels.
///
/// The first tile asked of a level pays for the convex hull and the alpha
/// shape of every cell on it, which is a cost per level rather than per tile
/// and would otherwise be smeared over whichever tile happened to be first.
/// It is therefore reported on its own, and the rest are reported as a median
/// so that one slow tile does not stand for the level.
fn run_bench(state: &ServerState, zooms: &[u32], side: u32, at: FloatCoordinate) {
    println!();
    println!(
        "sweep of {} zoom levels over {} partition levels, {}x{} tiles each, at {:.5},{:.5}",
        zooms.len(),
        state.max_level + 1,
        side,
        side,
        at.lat.0,
        at.lon.0
    );
    println!(
        "{:>5}  {:>5}  {:>10}  {:>10}  {:>10}  {:>9}  {:>8}",
        "level", "zoom", "first ms", "median ms", "slowest ms", "bytes", "features"
    );

    for level in 0..=state.max_level {
        for &zoom in zooms {
            let (centre_x, centre_y) = coordinate_to_tile_number(at, zoom);
            let span = 1u32 << zoom;
            let mut timings = Vec::new();
            let mut bytes = 0usize;
            let mut features = 0usize;

            for dy in 0..side {
                for dx in 0..side {
                    let x = (centre_x + dx).saturating_sub(side / 2).min(span - 1);
                    let y = (centre_y + dy).saturating_sub(side / 2).min(span - 1);
                    let started = Instant::now();
                    let tile = build_tile(state, level, zoom, x, y);
                    timings.push(started.elapsed().as_secs_f64() * 1000.);
                    features += tile.layers.iter().map(|l| l.features.len()).sum::<usize>();
                    let mut buf = Vec::new();
                    tile.encode(&mut buf).expect("a built tile does not encode");
                    bytes += buf.len();
                }
            }

            let first = timings[0];
            let mut rest = timings[1..].to_vec();
            rest.sort_by(f64::total_cmp);
            let median = rest.get(rest.len() / 2).copied().unwrap_or(first);
            let slowest = rest.last().copied().unwrap_or(first);
            println!(
                "{level:>5}  {zoom:>5}  {first:>10.1}  {median:>10.1}  {slowest:>10.1}  {:>9}  {features:>8}",
                bytes / timings.len()
            );
        }
    }
    println!();
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

    let directory: LevelDirectory = io::read_from_file(&args.directory);
    info!(
        "loaded a directory of {} levels over {} nodes",
        directory.levels(),
        directory.number_of_nodes()
    );

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
        .enumerate()
        .min_by(|(_, left), (_, right)| {
            left.distance_to(&probe)
                .total_cmp(&right.distance_to(&probe))
        });
    if let Some((node, coordinate)) = nearest {
        info!(
            "closest node to {probe}: {coordinate}, node {node} in cell {} of the finest level, {:.3} km away",
            directory.cell_of(node, 0),
            coordinate.distance_to(&probe)
        );
    }

    let state = web::Data::new(ServerState::new(
        static_graph,
        coordinates,
        directory,
        args.alpha,
    ));
    info!(
        "{} arcs on the boundary between {} cells of the finest level, on {} border nodes",
        state.tiles.boundary.len(),
        state.directory().cells_on_level(0),
        state.tiles.border_nodes.len()
    );
    if state.tiles.boundary.is_empty() {
        warn!("the partition has no boundary, so the tiles will be empty");
    }

    if args.bench {
        let mut parts = args.bench_at.split(',');
        let lat = parts.next().and_then(|v| v.trim().parse().ok());
        let lon = parts.next().and_then(|v| v.trim().parse().ok());
        let (Some(lat), Some(lon)) = (lat, lon) else {
            return Err("--bench-at wants lat,lon".into());
        };
        run_bench(
            &state,
            &args.bench_zooms,
            args.bench_side.max(1),
            FloatCoordinate {
                lat: FloatLatitude(lat),
                lon: FloatLongitude(lon),
            },
        );
        return Ok(());
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
    use crate::tile::{GeomType, Layer};
    use crate::tile_builder::{BORDER_LAYER, CELL_LAYER, INTERIOR_LAYER};
    use crate::tile_index::TileData;
    use crate::tile_index::{BorderNode, BoundaryArc, INDEX_ZOOM};
    use rustc_hash::FxHashMap;
    use std::sync::Mutex;
    use toolbox_rs::customization::Customization;
    use toolbox_rs::static_graph::StaticGraph;
    use toolbox_rs::tile_geometry::TILE_EXTENT;
    // `test` is aliased, as importing it plainly would shadow the `#[test]`
    // attribute with the actix macro of the same name
    use actix_web::{App, body::to_bytes, http::StatusCode, test as actix_test};
    use toolbox_rs::mvt::{CLOSE_PATH, LINE_TO, MOVE_TO, command_and_count};

    /// Two cells that meet in the middle of a tile of Frankfurt, with one arc
    /// crossing between them.
    const ZOOM: u32 = 14;
    /// the finest level, which is where a directory starts counting
    const FINEST: u32 = 0;
    const LAT: f64 = 50.20731;
    const LON: f64 = 8.57747;

    /// A state carrying one arc between two cells, for the handlers that draw
    /// tiles. The graph holds that arc, as whether a node is still on the
    /// border of the level being looked at is read off it.
    fn state_with_one_arc() -> ServerState {
        let edges = vec![InputEdge::new(0, 1, 1_usize), InputEdge::new(1, 0, 1_usize)];
        ServerState {
            tiles: data_with_one_arc(),
            hulls: Mutex::new(FxHashMap::default()),
            shapes: Mutex::new(FxHashMap::default()),
            cell_trees: Mutex::new(FxHashMap::default()),
            alpha: 300.0,
            coordinates: vec![
                FPCoordinate::new_from_lat_lon(LAT, LON),
                FPCoordinate::new_from_lat_lon(LAT + 0.002, LON + 0.002),
            ],
            customization: Customization::new(
                StaticGraph::new(edges),
                // two cells of the finest level that meet on the one above
                LevelDirectory::new(vec![0, 1], vec![vec![0, 0], vec![0, 0]]),
            ),
            max_level: 2,
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
                from: 0,
                to: 1,
            }],
            border_nodes: vec![BorderNode {
                coordinate: FPCoordinate::new_from_lat_lon(LAT, LON),
                node: 0,
            }],
            nodes_by_tile: FxHashMap::default(),
            bucket_of_node: vec![0; 2],
            // the one arc is listed under the bucket of each of its ends
            boundary_by_tile: [
                FPCoordinate::new_from_lat_lon(LAT, LON),
                FPCoordinate::new_from_lat_lon(LAT + 0.002, LON + 0.002),
            ]
            .iter()
            .map(|coordinate| {
                let (lon, lat) = coordinate.to_lon_lat_pair();
                (
                    coordinate_to_tile_number(
                        FloatCoordinate {
                            lat: FloatLatitude(lat),
                            lon: FloatLongitude(lon),
                        },
                        INDEX_ZOOM,
                    ),
                    vec![0],
                )
            })
            .collect(),
            crossing: Vec::new(),
            crossing_by_tile: FxHashMap::default(),
            // and the one border node under the bucket it falls in
            border_by_tile: [(
                {
                    let (lon, lat) = FPCoordinate::new_from_lat_lon(LAT, LON).to_lon_lat_pair();
                    coordinate_to_tile_number(
                        FloatCoordinate {
                            lat: FloatLatitude(lat),
                            lon: FloatLongitude(lon),
                        },
                        INDEX_ZOOM,
                    )
                },
                vec![0],
            )]
            .into_iter()
            .collect(),
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
        // nodes 0 and 1 in one cell, nodes 2 and 3 in the other, joined above
        let directory = LevelDirectory::new(vec![0, 0, 1, 1], vec![vec![0, 0]]);
        ServerState::new(StaticGraph::new(edges), coordinates, directory, 300.0)
    }

    #[test]
    fn a_cell_becomes_a_feature_of_the_tile() {
        let (x, y) = tile_of_probe();
        let tile = build_tile(&state_with_one_arc(), FINEST, ZOOM, x, y);

        assert_eq!(
            tile.layers.len(),
            5,
            "one layer of shapes, one of hulls, one of interior arcs, one of the cut, one of nodes"
        );
        let layer = cell_layer(&tile);
        assert_eq!(layer.name, CELL_LAYER);
        assert_eq!(layer.extent, Some(TILE_EXTENT));
        assert_eq!(layer.features.len(), 1);
        assert_eq!(layer.keys, vec!["cell".to_string()]);
        assert_eq!(layer.values[0].uint_value, Some(0));
        assert_eq!(
            layer.features[0].r#type,
            Some(i32::from(GeomType::Linestring))
        );
    }

    #[test]
    fn arcs_of_one_cell_share_a_feature() {
        let mut data = data_with_one_arc();
        // two more arcs separating the same pair of cells, next to the first
        for offset in [0.0005, 0.001] {
            data.boundary.push(BoundaryArc {
                source: FPCoordinate::new_from_lat_lon(LAT + offset, LON),
                target: FPCoordinate::new_from_lat_lon(LAT + offset + 0.002, LON + 0.002),
                from: 0,
                to: 1,
            });
        }
        let offsets = (0..data.boundary.len() as u32).collect::<Vec<_>>();
        for arcs in data.boundary_by_tile.values_mut() {
            arcs.clone_from(&offsets);
        }

        let mut state = state_with_one_arc();
        state.tiles = data;
        let (x, y) = tile_of_probe();
        let tile = build_tile(&state, FINEST, ZOOM, x, y);
        let layer = cell_layer(&tile);

        // the arcs all leave the same cell, so they share its feature
        assert_eq!(layer.features.len(), 1);
        let move_tos = commands_of(&layer.features[0].geometry)
            .iter()
            .filter(|&&(id, _)| id == MOVE_TO)
            .count();
        assert_eq!(move_tos, 3, "one line string per arc");
    }

    #[test]
    fn a_tile_elsewhere_stays_empty() {
        // the same data, but a tile on the other side of the planet
        let tile = build_tile(&state_with_one_arc(), FINEST, ZOOM, 1, 1);
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
                // a ClosePath takes none, as it goes back to where the ring
                // started rather than to a position it is handed
                CLOSE_PATH => {
                    assert_eq!(count, 1, "a ClosePath repeats exactly once");
                    0
                }
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
        let tile = build_tile(&state_with_one_arc(), FINEST, ZOOM, x, y);
        // a single arc is one MoveTo onto its start and one LineTo to its end
        assert_eq!(
            commands_of(&cell_layer(&tile).features[0].geometry),
            vec![(MOVE_TO, 1), (LINE_TO, 1)]
        );
    }

    #[test]
    fn tags_stay_within_keys_and_values() {
        let (x, y) = tile_of_probe();
        let tile = build_tile(&state_with_one_arc(), FINEST, ZOOM, x, y);
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
            body.contains("location.origin + \"/cells-0/{z}/{x}/{y}.mvt\""),
            "the tile URL has to be absolute"
        );
        assert!(
            !body.contains("[\"/cells-"),
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
    fn border_nodes_reach_the_tile() {
        let (x, y) = tile_of_probe();
        let tile = build_tile(&state_with_one_arc(), FINEST, ZOOM, x, y);

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
        assert_eq!(layer.values[0].uint_value, Some(0));
        assert_eq!(layer.values[1].uint_value, Some(0));
    }

    #[test]
    fn border_nodes_of_another_tile_are_left_out() {
        let tile = build_tile(&state_with_one_arc(), FINEST, ZOOM, 1, 1);
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
        assert!(answer.contains("\"cell\":0"), "{answer}");
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
        let directory = LevelDirectory::new(vec![0, 0], Vec::new());
        let state = ServerState::new(StaticGraph::new(edges), coordinates, directory, 300.0);

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
            let tile = build_tile(&state, FINEST, INDEX_ZOOM, x, y);
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

        // the two nodes sit in different cells of the finest level, so the arc
        // between them separates them there
        assert_eq!(cut_at(FINEST).0, 1, "the arc is a cut on the finest level");
        // one level up both of them fall into one cell and it separates nothing
        assert_eq!(cut_at(FINEST + 1).0, 0, "the arc still cuts one level up");
    }

    #[test]
    fn the_level_of_a_request_is_held_within_the_directory() {
        let state = state_of_two_cells();
        assert_eq!(state.max_level, 1);
        // nothing asked for means the finest level
        assert_eq!(state.level_or_finest(None), 0);
        // and a level past the coarsest is pulled back onto the directory
        assert_eq!(state.level_or_finest(Some(1)), 1);
        assert_eq!(state.level_or_finest(Some(99)), 1);
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
        for level in [FINEST, FINEST + 1] {
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
