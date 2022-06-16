use crate::geometry::primitives::FPCoordinate;

fn less_msb(x: i32, y: i32) -> bool {
    x < y && x < (x ^ y)
}

pub fn zorder_cmp(lhs: FPCoordinate, rhs: FPCoordinate) -> std::cmp::Ordering {
    if less_msb(lhs.lat ^ rhs.lat, lhs.lon ^ rhs.lon) {
        return lhs.lon.cmp(&rhs.lon);
    }
    lhs.lat.cmp(&rhs.lat)
}
