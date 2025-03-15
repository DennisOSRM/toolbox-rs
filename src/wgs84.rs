//! WGS84 coordinate handling and conversions.
//!
//! This module provides functionality for working with WGS84 coordinates and converting between
//! different coordinate systems (WGS84, Web Mercator, pixel coordinates).

use crate::mercator::{EPSG3857_MAX_LATITUDE, lat_to_y_approx, y_to_lat};

/// Earth's radius at the equator in kilometers (semi-major axis of WGS84 ellipsoid)
pub const EARTH_RADIUS_KM: f64 = 6_378.137;
/// Maximum longitude in degrees
const MAX_LONGITUDE: f64 = 180.0;

/// Represents a latitude value in degrees
#[derive(Debug, Clone, Copy)]
pub struct FloatLatitude(pub f64);

/// Represents a longitude value in degrees
#[derive(Debug, Clone, Copy)]
pub struct FloatLongitude(pub f64);

/// Represents a coordinate pair in degrees (longitude, latitude)
#[derive(Debug, Clone, Copy)]
pub struct FloatCoordinate {
    pub lon: FloatLongitude,
    pub lat: FloatLatitude,
}

impl FloatLatitude {
    /// Clamps the latitude value to valid Web Mercator range (-85.051129° to +85.051129°)
    pub fn clamp(self) -> Self {
        FloatLatitude(self.0.clamp(-EPSG3857_MAX_LATITUDE, EPSG3857_MAX_LATITUDE))
    }
}

impl FloatLongitude {
    /// Clamps the longitude value to valid range (-180° to +180°)
    pub fn clamp(self) -> Self {
        FloatLongitude(self.0.clamp(-MAX_LONGITUDE, MAX_LONGITUDE))
    }
}

/// Converts WGS84 coordinates to Web Mercator projection
///
/// # Arguments
/// * `wgs84_coordinate` - Coordinate pair in WGS84 (latitude, longitude)
pub fn from_wgs84(wgs84_coordinate: FloatCoordinate) -> (f64, f64) {
    (
        wgs84_coordinate.lon.0,
        lat_to_y_approx(wgs84_coordinate.lat),
    )
}

/// Converts Web Mercator coordinates back to WGS84
///
/// # Arguments
/// * `mercator_coordinate` - Coordinate pair in Web Mercator projection
pub fn to_wgs84(mercator_coordinate: (f64, f64)) -> FloatCoordinate {
    FloatCoordinate {
        lon: FloatLongitude(mercator_coordinate.0),
        lat: y_to_lat(mercator_coordinate.1),
    }
}

#[cfg(test)]
mod tests {
    use crate::mercator::lat_to_y;

    use super::*;
    use std::f64::EPSILON;

    const TEST_COORDINATES: [(f64, f64); 4] = [
        (0.0, 0.0),     // equator
        (51.0, 13.0),   // Dresden
        (-33.9, 151.2), // Sydney
        (85.0, 180.0),  // near pole
    ];

    #[test]
    fn test_wgs84_roundtrip() {
        for &(lat, lon) in TEST_COORDINATES.iter() {
            let wgs84 = FloatCoordinate {
                lat: FloatLatitude(lat),
                lon: FloatLongitude(lon),
            };

            let mercator = from_wgs84(wgs84);
            let roundtrip = to_wgs84(mercator);

            assert!(
                (roundtrip.lat.0 - wgs84.lat.0).abs() < 1e-10,
                "Latitude roundtrip failed: {} -> {} -> {}",
                wgs84.lat.0,
                mercator.0,
                roundtrip.lat.0
            );

            assert!(
                (roundtrip.lon.0 - wgs84.lon.0).abs() < EPSILON,
                "Longitude roundtrip failed: {} -> {} -> {}",
                wgs84.lon.0,
                mercator.0,
                roundtrip.lon.0
            );
        }
    }

    #[test]
    fn test_latitude_bounds() {
        // Test latitude clamping
        let max_lat = FloatLatitude(90.0);
        let min_lat = FloatLatitude(-90.0);

        assert!((max_lat.clamp().0 - EPSG3857_MAX_LATITUDE).abs() < EPSILON);
        assert!((min_lat.clamp().0 + EPSG3857_MAX_LATITUDE).abs() < EPSILON);
    }

    #[test]
    fn test_y_lat_conversion() {
        for &(lat, _) in TEST_COORDINATES.iter() {
            let latitude = FloatLatitude(lat);
            let y = lat_to_y(latitude);
            let roundtrip = y_to_lat(y);

            assert!(
                (roundtrip.0 - latitude.0).abs() < 1e-10,
                "y/lat conversion failed: {} -> {} -> {}",
                latitude.0,
                y,
                roundtrip.0
            );
        }
    }

    #[test]
    fn test_approximation_accuracy() {
        for &(lat, _) in TEST_COORDINATES.iter() {
            let latitude = FloatLatitude(lat);
            let exact = lat_to_y(latitude);
            let approx = lat_to_y_approx(latitude);

            assert!(
                (exact - approx).abs() < 1e-10,
                "Approximation too inaccurate at {}: exact={}, approx={}",
                lat,
                exact,
                approx
            );
        }
    }

    #[test]
    fn test_coordinate_clamping() {
        // Test Latitude Clamping
        let test_cases_lat = [
            (-90.0, -EPSG3857_MAX_LATITUDE),
            (-86.0, -85.051_128_779_806_59),
            (0.0, 0.0),
            (85.0, 85.0),
            (90.0, EPSG3857_MAX_LATITUDE),
        ];

        for (input, expected) in test_cases_lat {
            let lat = FloatLatitude(input);
            let clamped = lat.clamp();
            assert!(
                (clamped.0 - expected).abs() < EPSILON,
                "Latitude clamping failed for {}: expected {}, got {}",
                input,
                expected,
                clamped.0
            );
        }

        // Test Longitude Clamping
        let test_cases_lon = [
            (-200.0, -180.0),
            (-180.0, -180.0),
            (0.0, 0.0),
            (180.0, 180.0),
            (200.0, 180.0),
        ];

        for (input, expected) in test_cases_lon {
            let lon = FloatLongitude(input);
            let clamped = lon.clamp();
            assert!(
                (clamped.0 - expected).abs() < EPSILON,
                "Longitude clamping failed for {}: expected {}, got {}",
                input,
                expected,
                clamped.0
            );
        }
    }
}
