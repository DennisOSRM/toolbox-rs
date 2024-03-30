pub mod distance {
    use crate::wgs84::EARTH_RADIUS_KM;

    pub fn haversine(latitude1: f64, longitude1: f64, latitude2: f64, longitude2: f64) -> f64 {
        let d1 = latitude1.to_radians();
        // let lon1 = longitude1.to_radians();
        let d2 = latitude2.to_radians();
        // let lon2 = longitude2.to_radians();

        let d_lat = (latitude2 - latitude1).to_radians();
        let d_lon = (longitude1 - longitude2).to_radians();

        let a = (d_lat / 2.).sin().powi(2) + d1.cos() * d2.cos() * (d_lon / 2.).sin().powi(2);

        // let d_sigma = 2. * a.sqrt().atan2(1. - a);
        // let d_sigma = 2. * a.sqrt().asin();
        let c = 2. * a.sqrt().atan2((1. - a).sqrt());

        EARTH_RADIUS_KM * c

        /*

        const R = 6371e3; // metres
        const φ1 = lat1 * Math.PI/180; // φ, λ in radians
        const φ2 = lat2 * Math.PI/180;
        const Δφ = (lat2-lat1) * Math.PI/180;
        const Δλ = (lon2-lon1) * Math.PI/180;

        const a = Math.sin(Δφ/2) * Math.sin(Δφ/2) +
                  Math.cos(φ1) * Math.cos(φ2) *
                  Math.sin(Δλ/2) * Math.sin(Δλ/2);
        const c = 2 * Math.atan2(Math.sqrt(a), Math.sqrt(1-a));

        const d = R * c; // in metres

                 */
    }

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
}

#[cfg(test)]
mod tests {
    use super::distance::{haversine, vincenty};

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
