use crate::geometry::primitives::FPCoordinate;

#[derive(Debug, PartialEq)]
pub struct BoundingBox {
    pub min: FPCoordinate,
    pub max: FPCoordinate,
}

pub fn bounding_box(input_coordinates: &[FPCoordinate]) -> BoundingBox {
    debug_assert!(!input_coordinates.is_empty());

    let mut min_lat = i32::MAX;
    let mut min_lon = i32::MAX;
    let mut max_lat = i32::MIN;
    let mut max_lon = i32::MIN;

    for c in input_coordinates {
        min_lat = min_lat.min(c.lat);
        min_lon = min_lon.min(c.lon);

        max_lat = max_lat.max(c.lat);
        max_lon = max_lon.max(c.lon);
    }

    let min = FPCoordinate::new(min_lat, min_lon);
    let max = FPCoordinate::new(max_lat, max_lon);

    BoundingBox { min, max }
}

#[cfg(test)]
pub mod tests {
    use crate::{
        bounding_box::{bounding_box, BoundingBox},
        geometry::primitives::FPCoordinate,
    };

    #[test]
    pub fn grid() {
        let mut coordinates: Vec<FPCoordinate> = Vec::new();
        for i in 0..100 {
            coordinates.push(FPCoordinate::new(i / 10, i % 10));
        }

        let expected = BoundingBox {
            min: FPCoordinate::new(0, 0),
            max: FPCoordinate::new(9, 9),
        };
        let result = bounding_box(&coordinates);
        assert_eq!(expected, result);
    }
}
