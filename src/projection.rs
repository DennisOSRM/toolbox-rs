pub mod mercator {
    // length of semi-major axis of the WGS84 ellipsoid, i.e. radius at equator
    const EARTH_RADIUS_KM: f64 = 6_378.137;

    pub fn lon2x(lon: f64) -> f64 {
        EARTH_RADIUS_KM * 1000. * lon.to_radians()
    }

    pub fn x2lon(x: f64) -> f64 {
        (x / (EARTH_RADIUS_KM * 1000.)).to_degrees()
    }

    pub fn lat2y(lat: f64) -> f64 {
        ((lat.to_radians() / 2. + std::f64::consts::PI / 4.).tan()).log(std::f64::consts::E)
            * EARTH_RADIUS_KM
            * 1000.
    }

    pub fn y2lat(y: f64) -> f64 {
        (2. * ((y / (EARTH_RADIUS_KM * 1000.)).exp()).atan() - std::f64::consts::PI / 2.)
            .to_degrees()
    }
}

#[cfg(test)]
mod tests {
    use crate::projection::mercator::{lat2y, lon2x, x2lon, y2lat};

    const ALLOWED_ERROR: f64 = 0.0000000000001;
    // Allowed error in IEEE-754-based projection math.
    // Note that this is way below a centimeter of error

    #[test]
    pub fn lon_conversion_roundtrip() {
        // Roundtrip calculation of the projection with expected tiny errors

        // longitude in [180. to -180.]
        for i in -180_00..180_01 {
            // off-by-one to be inclusive of 180.
            let lon = f64::from(i) * 0.01;
            let result = x2lon(lon2x(lon));
            assert!((lon - result).abs() < ALLOWED_ERROR);
        }
    }

    #[test]
    pub fn lat_conversion_roundtrip() {
        // Roundtrip calculation of the projection with expected tiny errors

        // latitude in [90. to -90.]
        for i in -90_00..90_01 {
            // off-by-one to be inclusive of 90.
            let lat = f64::from(i) * 0.01;
            let result = y2lat(lat2y(lat));
            assert!((lat - result).abs() < ALLOWED_ERROR);
        }
    }
}