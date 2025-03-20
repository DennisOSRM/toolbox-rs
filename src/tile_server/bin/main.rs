use actix_web::{App, HttpResponse, HttpServer, Responder, web};
use prost::Message;
use std::error::Error;
use tile::{Feature, GeomType, Layer, Value};
use toolbox_rs::math::zigzag_encode;

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
                    zigzag_encode(5) as u32,
                    zigzag_encode(5) as u32,       // Move to (5,5)
                    ((2 & 0x7) | (3 << 3)) as u32, // LineTo (2) for 3 coordinates
                    zigzag_encode(1) as u32,
                    zigzag_encode(0) as u32, // Line to (6,5)
                    zigzag_encode(0) as u32,
                    zigzag_encode(1) as u32, // Line to (6,6)
                    zigzag_encode(-1) as u32,
                    zigzag_encode(0) as u32, // Line to (5,6)
                    15,                      // ClosePath
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
    println!("Starting tile server on http://127.0.0.1:5000");

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
