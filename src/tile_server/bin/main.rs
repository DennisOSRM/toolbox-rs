use actix_web::{web, App, HttpResponse, HttpServer, Responder};
use std::error::Error;

const INDEX_HTML: &str = include_str!("../client/index.html");

// Tile request handler
async fn get_tile(path: web::Path<(String, u32, u32, u32)>) -> impl Responder {
    let (tileset_id, zoom, x, y) = path.into_inner();
    
    // TODO: Implement actual tile fetching logic here
    // For now, just return a placeholder response
    println!("Requesting tile: {tileset_id} at z={zoom} x={x} y={y}");
    HttpResponse::Ok()
        .content_type("application/x-protobuf")
        .body(Vec::new()) // Replace with actual MVT data
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