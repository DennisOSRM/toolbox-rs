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

use std::f64::consts::PI;

use crate::{
    mercator::{lat_to_y, lat_to_y_approx, lon_to_x, y_to_lat},
    wgs84::{FloatCoordinate, FloatLatitude, FloatLongitude},
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

/// Converts WGS84 coordinates to tile coordinates at a given zoom level
///
/// This function implements the standard Web Mercator projection used in web mapping.
/// It converts geographical coordinates (latitude/longitude) to tile numbers that
/// identify which tile contains the coordinate at the specified zoom level.
///
/// # Arguments
/// * `coordinate` - WGS84 coordinate (latitude/longitude)
/// * `zoom` - Zoom level (0-20)
///
/// # Returns
/// A tuple (x, y) containing the tile coordinates:
/// * x: Tile column (0 to 2^zoom - 1)
/// * y: Tile row (0 to 2^zoom - 1)
///
/// # Examples
/// ```rust
/// use toolbox_rs::wgs84::{FloatCoordinate, FloatLatitude, FloatLongitude};
/// use toolbox_rs::vector_tile::coordinate_to_tile_number;
///
/// let coordinate = FloatCoordinate {
///     lat: FloatLatitude(50.20731),
///     lon: FloatLongitude(8.57747),
/// };
///
/// // Convert to tile coordinates at zoom level 14
/// let (tile_x, tile_y) = coordinate_to_tile_number(coordinate, 14);
///
/// // Verify we get the correct tile
/// assert_eq!(tile_x, 8582);
/// assert_eq!(tile_y, 5541);
/// ```
///
/// # Implementation Details
/// The conversion uses the following formulas:
/// * x_tile = floor((longitude + 180) / 360 * 2^zoom)
/// * y_tile = floor((1 - ln(tan(lat) + 1/cos(lat)) / π) * 2^(zoom-1))
pub fn coordinate_to_tile_number(coordinate: FloatCoordinate, zoom: u32) -> (u32, u32) {
    let n = (1 << zoom) as f64;

    let x_tile = (n * (coordinate.lon.0 + 180.0) / 360.0) as u32;

    let lat_rad = coordinate.lat.0.to_radians();
    let y_tile = (n * (1.0 - (lat_rad.tan() + (1.0 / lat_rad.cos())).ln() / PI) / 2.0) as u32;

    (x_tile, y_tile)
}

#[derive(Debug)]
pub struct TileBounds {
    pub min_lon: FloatLongitude,
    pub min_lat: FloatLatitude,
    pub max_lon: FloatLongitude,
    pub max_lat: FloatLatitude,
}

/// Berechnet die WGS84-Koordinaten der Grenzen einer Kachel (Tile)
///
/// Jede Kachel wird durch ihre Position (x, y) und Zoomstufe definiert.
/// Die zurückgegebenen Koordinaten beschreiben ein Rechteck in WGS84-Koordinaten,
/// das die Kachel vollständig umschließt.
///
/// # Arguments
/// * `zoom` - Zoomstufe (0-20)
/// * `x` - X-Koordinate der Kachel (0 bis 2^zoom - 1)
/// * `y` - Y-Koordinate der Kachel (0 bis 2^zoom - 1)
///
/// # Returns
/// Ein `TileBounds`-Objekt mit den Grenzkoordinaten:
/// - `min_lon`: Westliche Grenze
/// - `min_lat`: Südliche Grenze
/// - `max_lon`: Östliche Grenze
/// - `max_lat`: Nördliche Grenze
///
/// # Examples
/// ```rust
/// use toolbox_rs::vector_tile::get_tile_bounds;
///
/// // Berlin Mitte bei Zoom 14
/// let bounds = get_tile_bounds(14, 8802, 5373);
/// assert!(bounds.min_lon.0 >= 13.4033203125);
/// assert!(bounds.max_lon.0 <= 13.42529296875);
/// ```
///
/// # Mathematische Details
/// Die Berechnung basiert auf der Web-Mercator-Projektion:
/// - Longitude: linear von -180° bis +180°
/// - Latitude: nicht-linear aufgrund der Mercator-Projektion
/// - Die Y-Koordinaten werden über arctan(sinh(π * (1-2y/2^zoom))) berechnet
pub fn get_tile_bounds(zoom: u32, x: u32, y: u32) -> TileBounds {
    let n = (1u32 << zoom) as f64;

    let lon1 = x as f64 / n * 360.0 - 180.0;
    let lon2 = (x + 1) as f64 / n * 360.0 - 180.0;
    let lat1 = (PI * (1.0 - 2.0 * y as f64 / n)).sinh().atan().to_degrees();
    let lat2 = (PI * (1.0 - 2.0 * (y + 1) as f64 / n))
        .sinh()
        .atan()
        .to_degrees();

    TileBounds {
        min_lon: FloatLongitude(lon1),
        min_lat: FloatLatitude(lat1),
        max_lon: FloatLongitude(lon2),
        max_lat: FloatLatitude(lat2),
    }
}

/// Converts WGS84 coordinates to tile-local pixel coordinates
///
/// For a given sequence of WGS84 coordinates, this function computes the corresponding
/// pixel coordinates within a specific tile. The resulting coordinates are in the range
/// 0..TILE_SIZE-1 (typically 0..4095).
///
/// # Arguments
/// * `points` - Slice of WGS84 coordinates to convert
/// * `zoom` - Zoom level of the target tile
/// * `tile_x` - X coordinate of the target tile
/// * `tile_y` - Y coordinate of the target tile
///
/// # Returns
/// A vector of (x,y) tuples containing pixel coordinates within the tile
///
/// # Examples
/// ```rust
/// use toolbox_rs::wgs84::{FloatCoordinate, FloatLatitude, FloatLongitude};
/// use toolbox_rs::vector_tile::linestring_to_tile_coords;
///
/// // Create a line in central Berlin
/// let points = vec![
///     FloatCoordinate {
///         lat: FloatLatitude(52.52),
///         lon: FloatLongitude(13.405),
///     },
///     FloatCoordinate {
///         lat: FloatLatitude(52.53),
///         lon: FloatLongitude(13.410),
///     },
/// ];
///
/// // Convert to tile coordinates (tile 8802/5373 at zoom 14)
/// let tile_coords = linestring_to_tile_coords(&points, 14, 8802, 5373);
///
/// // Verify coordinates are within tile bounds (0..4096)
/// for (x, y) in tile_coords {
///     assert!(x < 4096);
///     assert!(y < 4096);
/// }
/// ```
pub fn linestring_to_tile_coords(
    points: &[FloatCoordinate],
    zoom: u32,
    tile_x: u32,
    tile_y: u32,
) -> Vec<(u32, u32)> {
    let tile_bounds = get_tile_bounds(zoom, tile_x, tile_y);

    // Tile bounds in Web Mercator
    let min_x = lon_to_x(tile_bounds.min_lon);
    let max_x = lon_to_x(tile_bounds.max_lon);
    let min_y = lat_to_y_approx(tile_bounds.min_lat);
    let max_y = lat_to_y_approx(tile_bounds.max_lat);

    let x_span = max_x - min_x;
    let y_span = max_y - min_y;

    points
        .iter()
        .map(|coordinate| {
            let x = lon_to_x(coordinate.lon);
            let y = lat_to_y_approx(coordinate.lat);

            // normalize to tile coordinates, i.e. 0..TILE_SIZE
            let tile_x = ((x - min_x) * (TILE_SIZE as f64 - 1.0) / x_span) as u32;
            let tile_y = ((y - min_y) * (TILE_SIZE as f64 - 1.0) / y_span) as u32;

            (
                tile_x.min(TILE_SIZE as u32 - 1),
                tile_y.min(TILE_SIZE as u32 - 1),
            )
        })
        .collect()
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

                // Pixel coordinates must be within valid range
                assert!(
                    px >= 0.0 && px <= shift as f64,
                    "Pixel coordinate outside valid range at zoom {}: lat={}, px={}",
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

    #[test]
    fn test_coordinate_to_tile_conversion() {
        let test_cases = [
            (
                // downtown Berlin
                FloatCoordinate {
                    lat: FloatLatitude(52.52),
                    lon: FloatLongitude(13.405),
                },
                14,   // zoom
                8802, // tile_x
                5373, // tile_y
            ),
            (
                FloatCoordinate {
                    lat: FloatLatitude(50.20731),
                    lon: FloatLongitude(8.57747),
                },
                14,   // zoom
                8582, // tile_x
                5541, // tile_y
            ),
            (
                // coordinate on tile boundary
                FloatCoordinate {
                    lat: FloatLatitude(52.5224609375),
                    lon: FloatLongitude(13.4033203125),
                },
                14,   // zoom
                8802, // tile_x
                5373, // tile_y
            ),
            (
                FloatCoordinate {
                    lat: FloatLatitude(52.5224609375),
                    lon: FloatLongitude(13.4033203125),
                },
                14,
                8802,
                5373,
            ),
            (
                // Hachiku statue in Tokyo
                FloatCoordinate {
                    lat: FloatLatitude(35.6590699),
                    lon: FloatLongitude(139.7006793),
                },
                18,
                232798,
                103246,
            ),
        ];

        for (coordinate, zoom, tile_x, tile_y) in test_cases {
            let tile_coords = coordinate_to_tile_number(coordinate, zoom);

            assert_eq!(
                tile_coords.0, tile_x,
                "x-coordinate mismatch: expected={}, result={}",
                tile_x, tile_coords.0
            );
            assert_eq!(
                tile_coords.1, tile_y,
                "y-coordinate mismatch: expected={}, result={}",
                tile_y, tile_coords.1
            );
        }
    }

    #[test]
    fn test_linestring_to_tile_coords() {
        let test_cases = [
            // case 1: Berlin
            (
                vec![
                    FloatCoordinate {
                        lat: FloatLatitude(52.52),
                        lon: FloatLongitude(13.405),
                    },
                    FloatCoordinate {
                        lat: FloatLatitude(52.53),
                        lon: FloatLongitude(13.410),
                    },
                ],
                14,                          // zoom
                8802,                        // tile_x
                5373,                        // tile_y,
                vec![(313, 890), (1244, 0)], // expected
            ),
            // case 2: tile boundary
            (
                vec![FloatCoordinate {
                    lat: FloatLatitude(52.5224609375),
                    lon: FloatLongitude(13.4033203125),
                }],
                14,             // zoom
                8802,           // tile_x
                5373,           // tile_y
                vec![(0, 136)], // expected
            ),
        ];

        for (points, zoom, tile_x, tile_y, expected) in test_cases {
            let tile_coords = linestring_to_tile_coords(&points, zoom, tile_x, tile_y);

            assert_eq!(tile_coords, expected, "Tile coordinates mismatch");

            assert_eq!(
                tile_coords.len(),
                points.len(),
                "number of points and tile coordinates differ"
            );

            // check tile coordinates are plausible
            if points.len() >= 2 {
                let (x1, y1) = tile_coords[0];
                let (x2, y2) = tile_coords[1];

                // compute Manhattan distance between points
                let manhattan_dist =
                    ((x2 as i32 - x1 as i32).abs() + (y2 as i32 - y1 as i32).abs()) as u32;

                assert!(
                    manhattan_dist < TILE_SIZE as u32,
                    "implausible tile coordinates: {:?} -> {:?}, distance={}",
                    tile_coords[0],
                    tile_coords[1],
                    manhattan_dist
                );
            }
        }
    }
}
