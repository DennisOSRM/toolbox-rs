//! Vector tile coordinate handling and conversions
//!
//! This module provides functionality for converting between different coordinate systems
//! used in vector tile mapping:
//! - WGS84 coordinates (latitude/longitude)
//! - Pixel coordinates within tiles
//! - Tile coordinates (x, y, zoom)
//!
//! # Vector Tiles
//!
//! Vector tiles are square vector images, typically 256×256 pixels, that together
//! create a slippy map. The tile coordinate system:
//! - Origin (0,0) is at the top-left corner
//! - Tiles are addressed by x, y coordinates and zoom level
//! - At zoom level z, the map consists of 2^z × 2^z tiles
//!
//! # Coordinate Systems
//!
//! ## Tile Coordinates
//! - x: Column number from left (0 to 2^zoom - 1)
//! - y: Row number from top (0 to 2^zoom - 1)
//! - zoom: Detail level (typically 0-20)
//!
//! ## Pixel Coordinates
//! - Within each tile: 0 to TILE_SIZE-1 (typically 256)
//! - Global: 0 to 2^zoom * TILE_SIZE
//!
//! # Examples
//!
//! ```rust
//! use toolbox_rs::vector_tile::{degree_to_pixel_lon, degree_to_pixel_lat};
//! use toolbox_rs::wgs84::{FloatLatitude, FloatLongitude};
//!
//! // Convert Dresden coordinates to pixels at zoom level 12
//! let lat = FloatLatitude(51.0504);
//! let lon = FloatLongitude(13.7373);
//! let zoom = 12;
//!
//! let px_x = degree_to_pixel_lon(lon, zoom);
//! let px_y = degree_to_pixel_lat(lat, zoom);
//! ```
//!
//! # Implementation Notes
//!
//! - Pixel coordinates increase from west to east (x) and north to south (y)
//! - The equator is centered at y = 2^(zoom-1) * TILE_SIZE
//! - Greenwich meridian is centered at x = 2^(zoom-1) * TILE_SIZE

use crate::{
    mercator::{lat_to_y, y_to_lat},
    wgs84::{FloatLatitude, FloatLongitude},
};

/// Size of a map tile in pixels
const TILE_SIZE: usize = 4096;

/// Converts longitude in degrees to pixel x-coordinate
///
/// # Arguments
/// * `lon` - Longitude in degrees
/// * `zoom` - Zoom level
pub fn degree_to_pixel_lon(lon: FloatLongitude, zoom: u32) -> f64 {
    let shift = (1 << zoom) * TILE_SIZE;
    let b = shift as f64 / 2.0;
    b * (1.0 + lon.0 / 180.0)
}

/// Converts latitude in degrees to pixel y-coordinate
///
/// # Arguments
/// * `lat` - Latitude in degrees
/// * `zoom` - Zoom level
pub fn degree_to_pixel_lat(lat: FloatLatitude, zoom: u32) -> f64 {
    let shift = (1 << zoom) * TILE_SIZE;
    let b = shift as f64 / 2.0;
    b * (1.0 - lat_to_y(lat) / 180.0)
}

/// Converts pixel coordinates to degrees
///
/// # Arguments
/// * `shift` - Pixel shift based on zoom level (2^zoom * TILE_SIZE)
/// * `x` - x-coordinate in pixels (modified in place)
/// * `y` - y-coordinate in pixels (modified in place)
pub fn pixel_to_degree(shift: usize, x: &mut f64, y: &mut f64) {
    let b = shift as f64 / 2.0;
    *x = ((*x - b) / shift as f64) * 360.0;
    let normalized_y = *y / shift as f64;
    let lat_rad = std::f64::consts::PI * (1.0 - 2.0 * normalized_y);
    *y = y_to_lat(lat_rad.to_degrees()).0;
}

#[cfg(test)]
mod tests {
    use std::f64::EPSILON;

    use crate::wgs84::FloatCoordinate;

    use super::*;

    const TEST_COORDINATES: [(f64, f64); 4] = [
        (0.0, 0.0),     // equator
        (51.0, 13.0),   // Dresden
        (-33.9, 151.2), // Sydney
        (85.0, 180.0),  // near pole
    ];

    #[test]
    fn test_pixel_coordinates() {
        // Test for zoom level 0
        let center = FloatCoordinate {
            lat: FloatLatitude(0.0),
            lon: FloatLongitude(0.0),
        };

        let px_lat = degree_to_pixel_lat(center.lat, 0);
        let px_lon = degree_to_pixel_lon(center.lon, 0);

        assert!((px_lat - TILE_SIZE as f64 / 2.0).abs() < EPSILON);
        assert!((px_lon - TILE_SIZE as f64 / 2.0).abs() < EPSILON);
    }

    #[test]
    fn test_pixel_to_degree() {
        let test_cases = [
            // shift,    x_in,   y_in,    x_out,  y_out
            (256, 128.0, 128.0, 0.0, 0.0),     // center at zoom 0
            (512, 256.0, 256.0, 0.0, 0.0),     // center at zoom 1
            (256, 0.0, 0.0, -180.0, 85.0),     // northwest corner
            (256, 256.0, 256.0, 180.0, -85.0), // southeast corner
        ];

        for (shift, x_in, y_in, x_expected, y_expected) in test_cases {
            let mut x = x_in;
            let mut y = y_in;
            pixel_to_degree(shift, &mut x, &mut y);

            assert!(
                (x - x_expected).abs() < 1e-10,
                "x-coordinate wrong, shift={}: expected={}, result={}",
                shift,
                x_expected,
                x
            );

            assert!(
                (y - y_expected).abs() < 1.0,
                "y-coordinate wrong, shift={}: expected={}, result={}",
                shift,
                y_expected,
                y
            );
        }

        // test roundtrip with degree_to_pixel_*
        for &(lat, lon) in TEST_COORDINATES.iter() {
            let zoom = 1u32;
            let shift = (1 << zoom) * TILE_SIZE;

            let orig_lat = FloatLatitude(lat);
            let orig_lon = FloatLongitude(lon);

            let px_x = degree_to_pixel_lon(orig_lon, zoom);
            let px_y = degree_to_pixel_lat(orig_lat, zoom);

            let mut x = px_x;
            let mut y = px_y;
            pixel_to_degree(shift, &mut x, &mut y);

            assert!(
                (x - lon).abs() < 1e-10,
                "Longitude roundtrip failed: {} -> ({}, {}) -> {}",
                lon,
                px_x,
                px_y,
                x
            );

            assert!(
                (y - lat).abs() < 1.0,
                "Latitude roundtrip failed: {} -> ({}, {}) -> {}",
                lat,
                px_x,
                px_y,
                y
            );
        }
    }

    #[test]
    fn test_degree_to_pixel_lat_zoom_levels() {
        let test_coordinates = [
            FloatLatitude(0.0),   // equator
            FloatLatitude(51.0),  // Frankfurt
            FloatLatitude(-33.9), // Sydney
            FloatLatitude(85.0),  // near pole
        ];

        for zoom in 0..=18 {
            let shift = (1 << zoom) * TILE_SIZE;
            let center = shift as f64 / 2.0;

            for &lat in &test_coordinates {
                let px = degree_to_pixel_lat(lat, zoom);

                // equator should be centered
                if (lat.0 - 0.0).abs() < EPSILON {
                    assert!(
                        (px - center).abs() < EPSILON,
                        "equator not centered at zoom {zoom}: expected={center}, result={px}"
                    );
                }

                // Pixel-Koordinaten müssen im gültigen Bereich liegen
                assert!(
                    px >= 0.0 && px <= shift as f64,
                    "Pixel-Koordinate außerhalb des gültigen Bereichs bei Zoom {}: lat={}, px={}",
                    zoom,
                    lat.0,
                    px
                );

                // roundtrip test
                let mut x = 0.0;
                let mut y = px;
                pixel_to_degree(shift, &mut x, &mut y);
                assert!(
                    (y - lat.0).abs() < 1.0,
                    "Roundtrip failed at zoom {}: {} -> {} -> {}",
                    zoom,
                    lat.0,
                    px,
                    y
                );
            }
        }
    }
}
