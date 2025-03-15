//! Web Mercator projection utilities (EPSG:3857)
//!
//! This module provides functions for converting between WGS84 coordinates (latitude/longitude)
//! and Web Mercator projection coordinates (meters). It includes both exact and fast approximate
//! implementations.
//!
//! The Web Mercator projection is used by many web mapping services. It has the following properties:
//! - Conformal (preserves angles)
//! - Not equal-area (significant distortion towards the poles)
//! - Valid latitude range: approximately ±85.051129° (EPSG3857_MAX_LATITUDE)
//!
//! # Examples
//!
//! ```
//! use toolbox_rs::mercator::{lon_to_x, lat_to_y};
//! use toolbox_rs::wgs84::{FloatLatitude, FloatLongitude};
//!
//! // Convert WGS84 coordinates to Web Mercator
//! let lon = FloatLongitude(10.0);  // 10° East
//! let lat = FloatLatitude(52.0);   // 52° North
//!
//! let x = lon_to_x(lon);  // meters from Greenwich meridian
//! let y = lat_to_y(lat);  // meters from equator
//! ```

/// Maximum latitude for Web Mercator projection (EPSG:3857)
pub const EPSG3857_MAX_LATITUDE: f64 = 85.051_128_779_806_59;

use std::f64::consts::PI;

use crate::{
    math::horner,
    wgs84::{EARTH_RADIUS_KM, FloatLatitude, FloatLongitude},
};

/// Converts a y-coordinate in Web Mercator projection back to latitude in degrees
///
/// # Arguments
/// * `y` - The y-coordinate in Web Mercator projection
pub fn y_to_lat(y: f64) -> FloatLatitude {
    let clamped_y = y.clamp(-180.0, 180.0);
    let normalized_lat = 2.0_f64.to_degrees() * (clamped_y.to_radians()).exp().atan();
    FloatLatitude(normalized_lat - 90.0)
}

/// Converts longitude in degrees to x-coordinate in meters (Web Mercator)
///
/// # Arguments
/// * `lon` - Longitude in degrees
pub fn lon_to_x(lon: FloatLongitude) -> f64 {
    lon.0 * EARTH_RADIUS_KM * 1000. * PI / 180.0
}

/// Converts x-coordinate in meters (Web Mercator) to longitude in degrees
///
/// # Arguments
/// * `x` - x-coordinate in meters
pub fn x_to_lon(x: f64) -> f64 {
    x * 180.0 / (EARTH_RADIUS_KM * 1000.0 * PI)
}

/// Converts latitude in degrees to y-coordinate in Web Mercator projection
///
/// # Arguments
/// * `latitude` - Latitude in degrees
pub fn lat_to_y(latitude: FloatLatitude) -> f64 {
    let clamped_latitude = latitude.clamp();
    let f = (clamped_latitude.0.to_radians()).sin();
    0.5_f64.to_degrees() * ((1.0 + f) / (1.0 - f)).ln()
}

/// Fast approximation of latitude to y-coordinate conversion
///
/// Uses Padé approximation for latitudes between -70° and +70°.
/// Falls back to exact calculation for higher latitudes.
///
/// # Arguments
/// * `latitude` - Latitude in degrees
pub fn lat_to_y_approx(latitude: FloatLatitude) -> f64 {
    if latitude.0 < -70.0 || latitude.0 > 70.0 {
        return lat_to_y(latitude);
    }

    // Approximate the inverse Gudermannian function with the Padé approximant [11/11]: deg → deg
    // Coefficients are computed for the argument range [-70°,70°] by Remez algorithm
    // |err|_∞=3.387e-12
    let num_coeffs = [
        -9.829_380_759_917_322e-23,
        2.090_142_250_253_142e-23,
        3.135_247_548_180_731e-17,
        -2.245_638_108_317_767_7e-18,
        -1.772_744_532_357_163e-12,
        6.311_927_023_204_925e-14,
        3.681_880_554_703_047_5e-8,
        -6.627_785_084_960_899e-10,
        -3.212_917_016_733_647e-4,
        2.344_394_103_869_972e-6,
        1.000_000_000_000_891,
        0.00000000000000000000000000e+00,
    ];

    let den_coeffs = [
        -3.230_832_248_359_674e-28,
        -8.721_307_289_820_124e-22,
        9.176_951_419_542_66e-23,
        9.329_992_291_691_568e-17,
        -4.784_462_798_887_749e-18,
        -3.308_332_886_079_218e-12,
        9.374_685_611_980_987e-14,
        5.184_187_241_865_764e-8,
        -7.818_023_896_854_292e-10,
        -3.720_612_716_272_519_7e-4,
        2.344_394_103_989_707e-6,
        1.0,
    ];

    horner(latitude.0, &num_coeffs) / horner(latitude.0, &den_coeffs)
}

#[cfg(test)]
mod tests {
    use super::*;
    const ALLOWED_ERROR: f64 = 0.0000000000001;
    // Allowed error in IEEE-754-based projection math.
    // Note that this is way below a centimeter of error

    #[test]
    fn lon_conversion_roundtrip() {
        // Roundtrip calculation of the projection with expected tiny errors

        // longitude in [180. to -180.]
        for i in -18_000..18_001 {
            // off-by-one to be inclusive of 180.
            let lon = f64::from(i) * 0.01;
            let result = x_to_lon(lon_to_x(FloatLongitude(lon)));
            assert!((lon - result).abs() < ALLOWED_ERROR);
        }
    }

    #[test]
    fn lat_conversion_roundtrip() {
        // Roundtrip calculation of the projection with expected tiny errors

        // latitude in [90. to -90.]
        for i in -85..85 {
            // off-by-one to be inclusive of 90.
            let lat = f64::from(i) * 0.01;
            let result = y_to_lat(lat_to_y(FloatLatitude(lat)));
            assert!(
                (lat - result.0).abs() < ALLOWED_ERROR,
                "lat={} result={} diff={}",
                lat,
                result.0,
                (lat - result.0).abs()
            );
        }
    }

    #[test]
    fn test_lon_to_x() {
        let test_cases = [
            (-180.0, -20_037_508.342789244), // most western point
            (-90.0, -10_018_754.171394622),  // 90° west
            (0.0, 0.0),                      // zero meridian
            (90.0, 10_018_754.171394622),    // 90° east
            (180.0, 20_037_508.342789244),   // most eastern point
        ];

        for (lon, expected_x) in test_cases {
            let x = lon_to_x(FloatLongitude(lon));
            assert!(
                (x - expected_x).abs() < 1e-6,
                "lon_to_x failed for {}: expected={}, but got={}",
                lon,
                expected_x,
                x
            );
        }

        // test earth circumference
        let earth_circumference = 40_075_016.686; // Meter am Äquator
        let calculated_circumference =
            lon_to_x(FloatLongitude(180.)) - lon_to_x(FloatLongitude(-180.));
        assert!(
            (earth_circumference - calculated_circumference).abs() < 1.0,
            "Earth radius wrong: expected={}, but got={}",
            earth_circumference,
            calculated_circumference
        );
    }

    #[test]
    fn test_x_to_lon() {
        let test_cases = [
            (-20_037_508.342789244, -180.0), // most western point
            (-10_018_754.171394622, -90.0),  // 90° west
            (0.0, 0.0),                      // zero meridian
            (10_018_754.171394622, 90.0),    // 90° east
            (20_037_508.342789244, 180.0),   // most eastern point
        ];

        for (x, expected_lon) in test_cases {
            let lon = x_to_lon(x);
            assert!(
                (lon - expected_lon).abs() < 1e-10,
                "x_to_lon failed for {}: expected={}, result={}",
                x,
                expected_lon,
                lon
            );
        }
    }

    #[test]
    fn longitude_x_roundtrip_accuracy() {
        for lon in (-180..=180).step_by(1) {
            let lon = lon as f64;
            let x = lon_to_x(FloatLongitude(lon));
            let lon_result = x_to_lon(x);
            assert!(
                (lon - lon_result).abs() < ALLOWED_ERROR,
                "lon to x roundtrip error {}: start={}, end={}",
                lon,
                lon,
                lon_result
            );
        }
    }
}
