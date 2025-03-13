mod command_line;

use actix_web::{App, HttpResponse, HttpServer, Responder, web};
use command_line::Arguments;
use env_logger::{Builder, Env};
use fxhash::FxHashMap;
use log::info;
use prost::Message;
use std::error::Error;
use tile::{Feature, GeomType, Layer, Value};
use toolbox_rs::{
    cell::Cell,
    edge::InputEdge,
    geometry::FPCoordinate,
    graph::Graph,
    io,
    math::zigzag_encode,
    one_to_many_dijkstra::OneToManyDijkstra,
    partition_id::PartitionID,
    r_tree::RTree,
    run_iterator::RunIterator,
    static_graph::{self, StaticGraph},
    unidirectional_dijkstra::UnidirectionalDijkstra, // unidirectional_dijkstra::UnidirectionalDijkstra,
};

// Include the generated protobuf code
include!(concat!(env!("OUT_DIR"), "/vector_tile.rs"));

const INDEX_HTML: &str = include_str!("../client/index.html");

// Tile request handler
async fn get_tile(path: web::Path<(String, u32, u32, u32)>) -> impl Responder {
    let (tileset_id, zoom, x, y) = path.into_inner();
    println!("Requesting tile: {tileset_id} at z={zoom} x={x} y={y}");

    // Create a sample tile
    let tile = Tile {
        layers: vec![Layer {
            version: 2,
            name: "speeds".to_string(),
            extent: Some(256),
            features: vec![Feature {
                id: Some(1),
                r#type: Some(GeomType::Linestring.into()),
                geometry: vec![
                    ((1 & 0x7) | (1 << 3)) as u32, // MoveTo (1) for 1 coordinate
                    zigzag_encode(5),
                    zigzag_encode(5),              // Move to (5,5)
                    ((2 & 0x7) | (3 << 3)) as u32, // LineTo (2) for 3 coordinates
                    zigzag_encode(1),
                    zigzag_encode(0), // Line to (6,5)
                    zigzag_encode(0),
                    zigzag_encode(1), // Line to (6,6)
                    zigzag_encode(-1),
                    zigzag_encode(0), // Line to (5,6)
                    15,               // ClosePath
                ],
                tags: vec![0, 0, 1, 1, 2, 1],
            }],
            keys: vec![
                "is_small".to_string(),
                "is_small".to_string(),
                "is_small".to_string(),
            ],
            values: vec![
                Value {
                    bool_value: Some(true),
                    ..Default::default()
                },
                Value {
                    bool_value: Some(true),
                    ..Default::default()
                },
            ],
        }],
    };

    // Encode the tile to protobuf format
    let mut buf = Vec::new();
    tile.encode(&mut buf).unwrap();

    HttpResponse::Ok()
        .content_type("application/x-protobuf")
        .body(buf)
}

async fn index() -> HttpResponse {
    HttpResponse::Ok().body(INDEX_HTML)
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

    HttpServer::new(|| {
        App::new()
            .route("/", web::get().to(index))
            .route("/{tileset_id}/{zoom}/{x}/{y}.mvt", web::get().to(get_tile))
    })
    .bind("127.0.0.1:5000")?
    .run()
    .await?;

    Ok(())
}
