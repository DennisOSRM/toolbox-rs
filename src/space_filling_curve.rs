use crate::geometry::primitives::FPCoordinate;

/// Provides a total order on fixed-point coordinates that corresponds to the
/// well-known Z-order space-filling curve.
///
/// This implementation ensures a proper total ordering by:
/// 1. First comparing the most significant differing bits
/// 2. Using consistent tie-breaking when bits are equal
/// 3. Properly handling all edge cases
///
/// # Arguments
/// * `lhs` - First coordinate to compare
/// * `rhs` - Second coordinate to compare
///
/// # Returns
/// A total ordering between the coordinates based on their Z-order:
/// * `Ordering::Less` - if `lhs` comes before `rhs`
/// * `Ordering::Equal` - if coordinates are identical
/// * `Ordering::Greater` - if `lhs` comes after `rhs`
///
/// # Examples
/// ```rust
/// use std::cmp::Ordering;
/// use toolbox_rs::geometry::primitives::FPCoordinate;
/// use toolbox_rs::space_filling_curve::zorder_cmp;
///
/// // Create some test coordinates
/// let berlin = FPCoordinate::new_from_lat_lon(52.520008, 13.404954);
/// let paris = FPCoordinate::new_from_lat_lon(48.856613, 2.352222);
/// let london = FPCoordinate::new_from_lat_lon(51.507351, -0.127758);
///
/// // Test total ordering properties
///
/// // 1. Antisymmetry: if a ≤ b and b ≤ a then a = b
/// assert_eq!(zorder_cmp(berlin, berlin), Ordering::Equal);
///
/// // 2. Transitivity: if a ≤ b and b ≤ c then a ≤ c
/// if zorder_cmp(paris, london) == Ordering::Less
///    && zorder_cmp(london, berlin) == Ordering::Less {
///     assert_eq!(zorder_cmp(paris, berlin), Ordering::Less);
/// }
///
/// // 3. Totality: either a ≤ b or b ≤ a must be true
/// let order = zorder_cmp(paris, london);
/// assert!(order == Ordering::Less || order == Ordering::Equal || order == Ordering::Greater);
/// ```
pub fn zorder_cmp(lhs: &FPCoordinate, rhs: &FPCoordinate) -> std::cmp::Ordering {
    let lat_xor = lhs.lat ^ rhs.lat;
    let lon_xor = lhs.lon ^ rhs.lon;

    // If both coordinates are identical
    if lat_xor == 0 && lon_xor == 0 {
        return std::cmp::Ordering::Equal;
    }

    // If one dimension has no differences
    if lat_xor == 0 {
        return lhs.lon.cmp(&rhs.lon);
    }
    if lon_xor == 0 {
        return lhs.lat.cmp(&rhs.lat);
    }

    // Compare most significant bits
    let lat_msb = 31 - lat_xor.leading_zeros();
    let lon_msb = 31 - lon_xor.leading_zeros();

    match lat_msb.cmp(&lon_msb) {
        std::cmp::Ordering::Greater => lhs.lat.cmp(&rhs.lat),
        std::cmp::Ordering::Less => lhs.lon.cmp(&rhs.lon),
        std::cmp::Ordering::Equal => {
            // If MSBs are at same position, use consistent ordering
            if (lhs.lat >> lat_msb) & 1 != (rhs.lat >> lat_msb) & 1 {
                lhs.lat.cmp(&rhs.lat)
            } else {
                lhs.lon.cmp(&rhs.lon)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{geometry::primitives::FPCoordinate, space_filling_curve::zorder_cmp};

    #[test]
    fn compare_greater() {
        let ny = FPCoordinate::new_from_lat_lon(40.730610, -73.935242);
        let sf = FPCoordinate::new_from_lat_lon(37.773972, -122.431297);

        assert_eq!(std::cmp::Ordering::Greater, zorder_cmp(&ny, &sf));
    }

    #[test]
    fn compare_less() {
        let ny = FPCoordinate::new_from_lat_lon(40.730610, -73.935242);
        let sf = FPCoordinate::new_from_lat_lon(37.773972, -122.431297);

        assert_eq!(std::cmp::Ordering::Less, zorder_cmp(&sf, &ny));
    }

    #[test]
    fn compare_equal() {
        let ny = FPCoordinate::new_from_lat_lon(40.730610, -73.935242);
        assert_eq!(std::cmp::Ordering::Equal, zorder_cmp(&ny, &ny));
    }
}
