use crate::geometry::primitives::FPCoordinate;

/// Provides a total order on fixed-point coordinates that corresponds to the
/// well-known Z-order space-filling curve. The compiler will emit about twenty
/// assembly instructions for this.
pub fn zorder_cmp(lhs: FPCoordinate, rhs: FPCoordinate) -> std::cmp::Ordering {
    if less_msb(lhs.lat ^ rhs.lat, lhs.lon ^ rhs.lon) {
        return lhs.lon.cmp(&rhs.lon);
    }
    lhs.lat.cmp(&rhs.lat)
}

fn less_msb(x: i32, y: i32) -> bool {
    x < y && x < (x ^ y)
}

#[cfg(test)]
mod tests {
    use crate::{geometry::primitives::FPCoordinate, space_filling_curve::zorder_cmp};

    #[test]
    fn compare_greater() {
        let ny = FPCoordinate::new_from_lat_lon(40.730610, -73.935242);
        let sf = FPCoordinate::new_from_lat_lon(37.773972, -122.431297);

        assert_eq!(std::cmp::Ordering::Greater, zorder_cmp(ny, sf));
    }

    #[test]
    fn compare_less() {
        let ny = FPCoordinate::new_from_lat_lon(40.730610, -73.935242);
        let sf = FPCoordinate::new_from_lat_lon(37.773972, -122.431297);

        assert_eq!(std::cmp::Ordering::Less, zorder_cmp(sf, ny));
    }

    #[test]
    fn compare_equal() {
        let ny = FPCoordinate::new_from_lat_lon(40.730610, -73.935242);
        assert_eq!(std::cmp::Ordering::Equal, zorder_cmp(ny, ny));
    }
}
