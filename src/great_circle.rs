use crate::wgs84::EARTH_RADIUS_KM;

/// Calculates the great-circle distance between two points using the Haversine formula.
///
/// This function computes the shortest distance over the earth's surface between two points
/// given their latitude and longitude coordinates. It uses the Haversine formula which
/// provides good accuracy for most distances while being relatively simple to compute.
///
/// # Arguments
/// * `latitude1` - Latitude of first point in decimal degrees
/// * `longitude1` - Longitude of first point in decimal degrees
/// * `latitude2` - Latitude of second point in decimal degrees
/// * `longitude2` - Longitude of second point in decimal degrees
///
/// # Returns
/// Distance between the points in kilometers
///
/// # Examples
///
/// ```
/// use toolbox_rs::great_circle::haversine;
///
/// // Distance between New York and San Francisco
/// let ny = (40.730610, -73.935242);
/// let sf = (37.773972, -122.431297);
/// let distance = haversine(ny.0, ny.1, sf.0, sf.1);
///
/// assert!((distance - 4140.175).abs() < 0.001);
///
/// // Distance to self is zero
/// let distance = haversine(ny.0, ny.1, ny.0, ny.1);
/// assert!(distance < 0.000001);
/// ```
pub fn haversine(latitude1: f64, longitude1: f64, latitude2: f64, longitude2: f64) -> f64 {
    let d1 = latitude1.to_radians();
    let d2 = latitude2.to_radians();

    let d_lat = (latitude2 - latitude1).to_radians();
    let d_lon = (longitude1 - longitude2).to_radians();

    let a = (d_lat / 2.).sin().powi(2) + d1.cos() * d2.cos() * (d_lon / 2.).sin().powi(2);

    let c = 2. * a.sqrt().atan2((1. - a).sqrt());

    EARTH_RADIUS_KM * c
}

/// Calculates the great-circle distance between two points using Vincenty's formula.
///
/// This function implements Vincenty's formula for calculating geodesic distances.
/// It's generally more accurate than the Haversine formula, especially for antipodal points
/// (points on opposite sides of the Earth).
///
/// # Arguments
/// * `latitude1` - Latitude of first point in decimal degrees
/// * `longitude1` - Longitude of first point in decimal degrees
/// * `latitude2` - Latitude of second point in decimal degrees
/// * `longitude2` - Longitude of second point in decimal degrees
///
/// # Returns
/// Distance between the points in kilometers
///
/// # Examples
///
/// ```
/// use toolbox_rs::great_circle::vincenty;
///
/// // Distance between New York and San Francisco
/// let ny = (40.730610, -73.935242);
/// let sf = (37.773972, -122.431297);
/// let distance = vincenty(ny.0, ny.1, sf.0, sf.1);
///
/// assert!((distance - 4140.175).abs() < 0.001);
///
/// // Antipodal points (approximately)
/// let dist = vincenty(0.0, 0.0, 0.0, 180.0);
/// assert!((dist - 20015.0).abs() < 25.0); // Half Earth's circumference
/// ```
pub fn vincenty(latitude1: f64, longitude1: f64, latitude2: f64, longitude2: f64) -> f64 {
    let lat1 = latitude1.to_radians();
    let lon1 = longitude1.to_radians();
    let lat2 = latitude2.to_radians();
    let lon2 = longitude2.to_radians();

    let d_lon = (lon1 - lon2).abs();

    // Numerator
    let a = (lat2.cos() * d_lon.sin()).powi(2);
    let b = lat1.cos() * lat2.sin();
    let c = lat1.sin() * lat2.cos() * d_lon.cos();
    let d = (b - c).powi(2);
    let e = (a + d).sqrt();

    // Denominator
    let f = lat1.sin() * lat2.sin();
    let g = lat1.cos() * lat2.cos() * d_lon.cos();

    let h = f + g;

    let d_sigma = e.atan2(h);

    EARTH_RADIUS_KM * d_sigma
}

#[cfg(test)]
mod tests {
    use super::{haversine, vincenty};

    #[test]
    fn haversine_sf_nyc() {
        let ny = (40.730610, -73.935242);
        let sf = (37.773972, -122.431297);
        assert_eq!((haversine(ny.0, ny.1, sf.0, sf.1) * 1000.) as u32, 4140175)
    }

    #[test]
    fn vincenty_sf_nyc() {
        let ny = (40.730610, -73.935242);
        let sf = (37.773972, -122.431297);
        assert_eq!((vincenty(ny.0, ny.1, sf.0, sf.1) * 1000.) as u32, 4140175)
    }
}
