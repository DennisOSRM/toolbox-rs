mod command_line;

use actix_web::{App, HttpResponse, HttpServer, Responder, web};
use command_line::Arguments;
use env_logger::{Builder, Env};
use log::info;
use prost::Message;
use rustc_hash::FxHashMap;
use std::error::Error;
use tile::{Feature, GeomType, Layer, Value};
use toolbox_rs::{
    cell::Cell,
    edge::InputEdge,
    geometry::FPCoordinate,
    graph::Graph,
    io,
    mvt::GeometryEncoder,
    one_to_many_dijkstra::OneToManyDijkstra,
    partition_id::PartitionID,
    r_tree::RTree,
    run_iterator::RunIterator,
    static_graph::{self, StaticGraph},
    unidirectional_dijkstra::UnidirectionalDijkstra,
};

// Include the generated protobuf code
include!(concat!(env!("OUT_DIR"), "/vector_tile.rs"));

const INDEX_HTML: &str = include_str!("../client/index.html");

/// The extent a tile is drawn on. Geometry is expressed in this grid rather
/// than in the coordinate system of the tile, so a reader scales it to whatever
/// size it renders at.
const TILE_EXTENT: u32 = 4096;

/// Builds the tile that covers the given position of the tile pyramid.
///
/// TODO: this still draws a fixed square instead of the cells of the partition.
/// It is wired to the geometry encoder so that the shape of the response is the
/// one a reader expects once the real geometry is put in.
fn build_tile(_zoom: u32, _x: u32, _y: u32) -> Tile {
    let mut geometry = GeometryEncoder::new();
    geometry.move_to(&[(5, 5)]);
    geometry.line_to(&[(6, 5), (6, 6), (5, 6)]);
    geometry.close_path();

    Tile {
        layers: vec![Layer {
            version: 2,
            name: "speeds".to_string(),
            extent: Some(TILE_EXTENT),
            features: vec![Feature {
                id: Some(1),
                r#type: Some(GeomType::Polygon.into()),
                geometry: geometry.build(),
                // a tag is a pair of indices, the first into keys and the
                // second into values
                tags: vec![0, 0],
            }],
            keys: vec!["is_small".to_string()],
            values: vec![Value {
                bool_value: Some(true),
                ..Default::default()
            }],
        }],
    }
}

// Tile request handler
async fn get_tile(path: web::Path<(String, u32, u32, u32)>) -> impl Responder {
    let (tileset_id, zoom, x, y) = path.into_inner();
    info!("requesting tile: {tileset_id} at z={zoom} x={x} y={y}");

    // Encode the tile to protobuf format
    let mut buf = Vec::new();
    build_tile(zoom, x, y)
        .encode(&mut buf)
        .expect("a tile does not fit into its buffer");

    HttpResponse::Ok()
        .content_type("application/x-protobuf")
        .body(buf)
}

async fn index() -> HttpResponse {
    HttpResponse::Ok().body(INDEX_HTML)
}

/// Registers the routes of the server. Both the server and the tests below are
/// built from this, so that a test cannot pass against a route that the server
/// does not actually serve.
fn routes(config: &mut web::ServiceConfig) {
    config
        .route("/", web::get().to(index))
        .route("/{tileset_id}/{zoom}/{x}/{y}.mvt", web::get().to(get_tile));
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

    let mut min_dist = f64::MAX;
    let mut minumum = (
        FPCoordinate::new_from_lat_lon(12., 12.),
        PartitionID::new(123),
    );
    coordinates.iter().zip(&partition_ids).for_each(|(c, p)| {
        let dist = c.distance_to(&FPCoordinate::new_from_lat_lon(50.20731, 8.57747));
        if dist < min_dist {
            min_dist = dist;
            minumum = (*c, *p);
        }
    });
    println!("min dist: {}, coordinate: {:?}", min_dist, minumum);

    // create r-tree for fast lookup of coordinates
    let rtree = RTree::from_elements(
        coordinates
            .iter()
            .cloned()
            .zip(partition_ids.iter().cloned()),
    );
    let input_coordinate = FPCoordinate::new_from_lat_lon(50.20731, 8.57747);
    let mut nearest = rtree.nearest_iter(&input_coordinate);
    println!("nearest: {:?}", nearest.next());

    println!("Starting tile server on http://127.0.0.1:5000");
    println!("Press Ctrl+C to stop the server");

    // Sort the partition ids by proxy in ascending order
    let mut partition_id_proxy = (0..partition_ids.len()).collect::<Vec<_>>();
    partition_id_proxy.sort_by_key(|&i| partition_ids[i]);

    // Create a run iterator to find runs of equal partition ids
    let cell_iterator = RunIterator::new_by(&partition_id_proxy, |&a, &b| {
        partition_ids[a] == partition_ids[b]
    });

    let pb = indicatif::ProgressBar::new(273521);
    // let mut cell_index = 0;
    let mut border_nodes = Vec::new();
    // let mut dijkstra = UnidirectionalDijkstra::new();
    let mut otm_dijkstra = OneToManyDijkstra::new();

    let mut cells = Vec::new();

    let mut cell_map = FxHashMap::default();

    // for run in cell_iterator {
    cell_iterator.enumerate().for_each(|(cell_index, run)| {
        border_nodes.clear();
        pb.set_message(format!("cell #{cell_index}"));
        // cell_index += 1;
        pb.inc(1);

        // extract the edges of the subgraph
        let source_partition_id = partition_ids[run[0]];
        let mut subgraph_edges = Vec::new();
        for &node_id in run {
            for edge in static_graph.edge_range(node_id) {
                let target = static_graph.target(edge);
                let target_partition_id = partition_ids[target];

                if target_partition_id == source_partition_id {
                    let data = static_graph.data(edge);
                    subgraph_edges.push(InputEdge::new(node_id, target, *data));
                } else {
                    border_nodes.push(node_id);
                }
            }
        }
        border_nodes.sort_unstable();
        border_nodes.dedup();

        let cell_id = partition_ids[border_nodes[0]];
        cell_map.insert(cell_id, cell_index - 1);
        // renumber source and target nodes of edges to be zero-based
        // TODO: faster hashmap implementation using tabhash or fibonacci hash
        let mut node_map = FxHashMap::default();
        for node_id in &border_nodes {
            node_map.insert(*node_id, node_map.len());
        }

        let subgraph_edges_len = subgraph_edges.len();
        for edge in &mut subgraph_edges {
            let current_len = node_map.len();
            edge.source = *node_map.entry(edge.source).or_insert(current_len);

            let current_len = node_map.len();
            edge.target = *node_map.entry(edge.target).or_insert(current_len);
            assert!(edge.source < 2 * subgraph_edges_len);
            assert!(edge.target < 2 * subgraph_edges_len);
        }
        // TODO: find a way to avoid relocations
        let cell_graph = StaticGraph::new(subgraph_edges);
        let mut cell = vec![0; border_nodes.len() * border_nodes.len()];
        let border_node_ids = (0..border_nodes.len()).collect::<Vec<_>>();
        for source in &border_node_ids {
            otm_dijkstra.run(&cell_graph, *source, &border_node_ids);
            for target in &border_node_ids {
                cell[source * border_nodes.len() + target] = otm_dijkstra.distance(*target);
            }
            // TODO: if one-to-many search checks out to be fully correct and reliable.
            // for target in &border_node_ids {
            //     if source == target {
            //         continue;
            //     }

            //     let distance = dijkstra.run(&cell_graph, *source, *target);
            //     cell[source * border_nodes.len() + target] = distance;
            // }
        }
        cells.push(Cell::new(border_nodes.clone(), cell, cell_index));
        // println!("cell: {:?}", cell);
        // panic!("stop");
    });
    info!("cells: {}", cells.len());
    info!("cell map: {}", cell_map.len());
    pb.finish_with_message("done");

    let source = cells[0].border_nodes()[0];
    let target =
        cells[cells.len() - 1].border_nodes()[cells[cells.len() - 1].border_nodes().len() - 1];
    info!(
        "first border node: {:?}, latlon: {}",
        source, coordinates[source]
    );
    info!(
        "last border node: {:?}, latlon: {}",
        target, coordinates[target]
    );

    // compute Dijkstra distance for first -> last
    let mut dijkstra = UnidirectionalDijkstra::new();
    let dijkstra_distance = dijkstra.run(&static_graph, source, target);
    info!("Dijkstra distance: {}", dijkstra_distance);

    // compute Cell distance for first -> last

    HttpServer::new(|| App::new().configure(routes))
        .bind("127.0.0.1:5000")?
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
    use toolbox_rs::mvt::{CLOSE_PATH, LINE_TO, MOVE_TO, command_and_count};

    #[actix_web::test]
    async fn index_is_served() {
        let app = actix_test::init_service(App::new().configure(routes)).await;
        let request = actix_test::TestRequest::get().uri("/").to_request();
        let response = actix_test::call_service(&app, request).await;

        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body()).await.expect("empty body");
        assert!(String::from_utf8_lossy(&body).contains("<html"));
    }

    #[actix_web::test]
    async fn tile_is_served_as_protobuf() {
        let app = actix_test::init_service(App::new().configure(routes)).await;
        let request = actix_test::TestRequest::get()
            .uri("/cells/12/2200/1345.mvt")
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
    }

    /// The response has to survive the round trip through the wire format, as
    /// that is all a client ever sees of it.
    #[actix_web::test]
    async fn served_tile_decodes_again() {
        let app = actix_test::init_service(App::new().configure(routes)).await;
        let request = actix_test::TestRequest::get()
            .uri("/cells/12/2200/1345.mvt")
            .to_request();
        let response = actix_test::call_service(&app, request).await;
        let body = to_bytes(response.into_body()).await.expect("empty body");

        let tile = Tile::decode(&body[..]).expect("served tile does not decode");
        assert_eq!(tile.layers.len(), 1);
        let layer = &tile.layers[0];
        assert_eq!(layer.name, "speeds");
        assert_eq!(layer.version, 2);
        assert_eq!(layer.extent, Some(TILE_EXTENT));
        assert_eq!(layer.features.len(), 1);
        assert_eq!(layer.features[0].r#type, Some(i32::from(GeomType::Polygon)));
    }

    /// Every tag is a pair of indices, and a reader that follows one out of
    /// range has no way to recover.
    #[test]
    fn tags_stay_within_keys_and_values() {
        let tile = build_tile(12, 2200, 1345);
        for layer in &tile.layers {
            for feature in &layer.features {
                assert_eq!(
                    feature.tags.len() % 2,
                    0,
                    "tags are pairs of a key and a value"
                );
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

    /// The geometry has to be a sequence a reader can walk: a command followed
    /// by exactly the number of parameters it announces.
    #[test]
    fn geometry_is_a_well_formed_command_sequence() {
        let tile = build_tile(12, 2200, 1345);
        let geometry = &tile.layers[0].features[0].geometry;

        let mut index = 0;
        let mut commands = Vec::new();
        while index < geometry.len() {
            let (id, count) = command_and_count(geometry[index]);
            assert!(count > 0, "a command that repeats zero times is rejected");
            let parameters = match id {
                MOVE_TO | LINE_TO => 2 * count as usize,
                CLOSE_PATH => {
                    assert_eq!(count, 1, "ClosePath does not repeat");
                    0
                }
                other => panic!("unknown command id {other}"),
            };
            assert!(
                index + 1 + parameters <= geometry.len(),
                "command runs past the end of the geometry"
            );
            commands.push(id);
            index += 1 + parameters;
        }

        // a polygon is a MoveTo, the line to its corners, and a closed ring
        assert_eq!(commands, vec![MOVE_TO, LINE_TO, CLOSE_PATH]);
    }

    /// A polygon has to start with a single MoveTo, or it is not one ring.
    #[test]
    fn polygon_starts_a_single_ring() {
        let tile = build_tile(0, 0, 0);
        let geometry = &tile.layers[0].features[0].geometry;
        assert_eq!(command_and_count(geometry[0]), (MOVE_TO, 1));
        assert_eq!(
            command_and_count(geometry[geometry.len() - 1]),
            (CLOSE_PATH, 1)
        );
    }
}
