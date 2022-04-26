pub mod distance {
    use crate::wgs84::EARTH_RADIUS_KM;

    pub fn haversine(
        latitude1: f64,
        longitude1: f64,
        latitude2: f64,
        longitude2: f64,
    ) -> f64 {
        let lat1 = latitude1.to_radians();
        let lon1 = longitude1.to_radians();
        let lat2 = latitude2.to_radians();
        let lon2 = longitude2.to_radians();

        let d_lat = (lat1 - lat2).abs();
        let d_lon = (lon1 - lon2).abs();

        let a = (d_lat / 2.).sin().powi(2) + lat1.cos() * lat2.cos() * (d_lon / 2.).sin().powi(2);

        // let d_sigma = 2. * a.sqrt().atan2(1. - a);
        let d_sigma = 2. * a.sqrt().asin();

        EARTH_RADIUS_KM * d_sigma
    }

    pub fn vincenty(
        latitude1: f64,
        longitude1: f64,
        latitude2: f64,
        longitude2: f64,
    ) -> f64 {
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
    pub fn haversine_sf_nyc() {
        assert_eq!((haversine(-122.416389, 37.7775, -74.006111, 40.712778) * 1000.) as u32, 5387354)
    }

    #[test]
    pub fn vincenty_sf_nyc() {
        assert_eq!((vincenty(-122.416389, 37.7775, -74.006111, 40.712778) * 1000.) as u32, 5387354)
    }
}