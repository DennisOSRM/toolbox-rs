//! WGS84 coordinate handling and conversions.
//!
//! This module provides functionality for working with WGS84 coordinates and converting between
//! different coordinate systems (WGS84, Web Mercator, pixel coordinates).

use crate::mercator::EPSG3857_MAX_LATITUDE;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_latitude_bounds() {
        // Test latitude clamping
        let max_lat = FloatLatitude(90.0);
        let min_lat = FloatLatitude(-90.0);

        assert!((max_lat.clamp().0 - EPSG3857_MAX_LATITUDE).abs() < f64::EPSILON);
        assert!((min_lat.clamp().0 + EPSG3857_MAX_LATITUDE).abs() < f64::EPSILON);
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
                (clamped.0 - expected).abs() < f64::EPSILON,
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
                (clamped.0 - expected).abs() < f64::EPSILON,
                "Longitude clamping failed for {}: expected {}, got {}",
                input,
                expected,
                clamped.0
            );
        }
    }
}
