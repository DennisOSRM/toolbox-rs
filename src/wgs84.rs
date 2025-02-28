// length of semi-major axis of the WGS84 ellipsoid, i.e. radius at equator

use std::f64::consts::PI;

use crate::math::horner;

pub const EARTH_RADIUS_KM: f64 = 6_378.137;
const DEGREE_TO_RAD: f64 = 0.017_453_292_519_943_295;
const EPSG3857_MAX_LATITUDE: f64 = 85.051_128_779_806_59;
const RAD_TO_DEGREE: f64 = 1.0 / DEGREE_TO_RAD;
const MAX_LONGITUDE: f64 = 180.0;
const TILE_SIZE: f64 = 256.0;

#[derive(Debug, Clone, Copy)]
pub struct FloatLatitude(f64);

#[derive(Debug, Clone, Copy)]
pub struct FloatLongitude(f64);

#[derive(Debug, Clone, Copy)]
pub struct FloatCoordinate {
    lon: FloatLongitude,
    lat: FloatLatitude,
}

impl FloatLatitude {
    pub fn clamp(self) -> Self {
        FloatLatitude(self.0.clamp(-EPSG3857_MAX_LATITUDE, EPSG3857_MAX_LATITUDE))
    }
}

impl FloatLongitude {
    pub fn clamp(self) -> Self {
        FloatLongitude(self.0.clamp(-MAX_LONGITUDE, MAX_LONGITUDE))
    }
}

pub fn y_to_lat(y: f64) -> FloatLatitude {
    let clamped_y = y.clamp(-180.0, 180.0);
    let normalized_lat = RAD_TO_DEGREE * 2.0 * (clamped_y * DEGREE_TO_RAD).exp().atan();

    FloatLatitude(normalized_lat - 90.0)
}

pub fn lat_to_y(latitude: FloatLatitude) -> f64 {
    let clamped_latitude = latitude.clamp();
    let f = (DEGREE_TO_RAD * clamped_latitude.0).sin();
    RAD_TO_DEGREE * 0.5 * ((1.0 + f) / (1.0 - f)).ln()
}

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

pub fn pixel_to_degree(shift: f64, x: &mut f64, y: &mut f64) {
    let b = shift / 2.0;
    *x = ((*x - b) / shift) * 360.0;
    let g = (*y - b) / -(shift * 0.5 / PI) / DEGREE_TO_RAD;
    *y = y_to_lat(g).0;
}

pub fn degree_to_pixel_lon(lon: FloatLongitude, zoom: u32) -> f64 {
    let shift = (1 << zoom) as f64 * TILE_SIZE;
    let b = shift / 2.0;
    b * (1.0 + lon.0 / 180.0)
}

pub fn degree_to_pixel_lat(lat: FloatLatitude, zoom: u32) -> f64 {
    let shift = (1 << zoom) as f64 * TILE_SIZE;
    let b = shift / 2.0;
    b * (1.0 - lat_to_y(lat) / 180.0)
}

pub fn from_wgs84(wgs84_coordinate: FloatCoordinate) -> FloatCoordinate {
    FloatCoordinate {
        lon: wgs84_coordinate.lon,
        lat: FloatLatitude(lat_to_y_approx(wgs84_coordinate.lat)),
    }
}

pub fn to_wgs84(mercator_coordinate: FloatCoordinate) -> FloatCoordinate {
    FloatCoordinate {
        lon: mercator_coordinate.lon,
        lat: y_to_lat(mercator_coordinate.lat.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::EPSILON;

    const TEST_COORDINATES: [(f64, f64); 4] = [
        (0.0, 0.0),     // Äquator/Nullmeridian
        (51.0, 13.0),   // Dresden
        (-33.9, 151.2), // Sydney
        (85.0, 180.0),  // Polnähe
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
                mercator.lat.0,
                roundtrip.lat.0
            );

            assert!(
                (roundtrip.lon.0 - wgs84.lon.0).abs() < EPSILON,
                "Longitude roundtrip failed: {} -> {} -> {}",
                wgs84.lon.0,
                mercator.lon.0,
                roundtrip.lon.0
            );
        }
    }

    #[test]
    fn test_latitude_bounds() {
        // Test Latitude-Begrenzung
        let max_lat = FloatLatitude(90.0);
        let min_lat = FloatLatitude(-90.0);

        assert!((max_lat.clamp().0 - EPSG3857_MAX_LATITUDE).abs() < EPSILON);
        assert!((min_lat.clamp().0 + EPSG3857_MAX_LATITUDE).abs() < EPSILON);
    }

    #[test]
    fn test_pixel_coordinates() {
        // Test für Zoom Level 0
        let center = FloatCoordinate {
            lat: FloatLatitude(0.0),
            lon: FloatLongitude(0.0),
        };

        let px_lat = degree_to_pixel_lat(center.lat, 0);
        let px_lon = degree_to_pixel_lon(center.lon, 0);

        assert!((px_lat - TILE_SIZE / 2.0).abs() < EPSILON);
        assert!((px_lon - TILE_SIZE / 2.0).abs() < EPSILON);
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
}
