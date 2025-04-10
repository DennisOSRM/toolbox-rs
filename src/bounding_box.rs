use crate::geometry::FPCoordinate;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BoundingBox {
    min: FPCoordinate,
    max: FPCoordinate,
}

impl BoundingBox {
    pub fn from_coordinates(coordinates: &[FPCoordinate]) -> BoundingBox {
        debug_assert!(!coordinates.is_empty());
        let mut min_coordinate = FPCoordinate::max();
        let mut max_coordinate = FPCoordinate::min();

        coordinates.iter().for_each(|coordinate| {
            min_coordinate.lat = min_coordinate.lat.min(coordinate.lat);
            min_coordinate.lon = min_coordinate.lon.min(coordinate.lon);
            max_coordinate.lat = max_coordinate.lat.max(coordinate.lat);
            max_coordinate.lon = max_coordinate.lon.max(coordinate.lon);
        });

        BoundingBox {
            min: min_coordinate,
            max: max_coordinate,
        }
    }

    pub fn invalid() -> BoundingBox {
        BoundingBox {
            min: FPCoordinate::max(),
            max: FPCoordinate::min(),
        }
    }

    pub fn extend_with(&mut self, other: &BoundingBox) {
        self.min.lat = self.min.lat.min(other.min.lat);
        self.min.lon = self.min.lon.min(other.min.lon);

        self.max.lat = self.max.lat.max(other.max.lat);
        self.max.lon = self.max.lon.max(other.max.lon);
    }

    pub fn center(&self) -> FPCoordinate {
        debug_assert!(self.min.lat <= self.max.lat);
        debug_assert!(self.min.lon <= self.max.lon);

        let lat_diff = self.max.lat - self.min.lat;
        let lon_diff = self.max.lon - self.min.lon;

        FPCoordinate {
            lat: self.min.lat + lat_diff / 2,
            lon: self.min.lon + lon_diff / 2,
        }
    }

    /// Tests if a coordinate lies within the bounding box
    ///
    /// A coordinate is considered inside if it lies within or on the boundaries
    /// of the bounding box.
    ///
    /// # Arguments
    /// * `coordinate` - The coordinate to test
    ///
    /// # Returns
    /// `true` if the coordinate is inside or on the boundary, `false` otherwise
    ///
    /// # Examples
    /// ```rust
    /// use toolbox_rs::geometry::FPCoordinate;
    /// use toolbox_rs::bounding_box::BoundingBox;
    ///
    /// let bbox = BoundingBox::from_coordinates(&[
    ///     FPCoordinate::new(10, 10),
    ///     FPCoordinate::new(20, 20),
    /// ]);
    ///
    /// assert!(bbox.contains(&FPCoordinate::new(15, 15))); // inside
    /// assert!(bbox.contains(&FPCoordinate::new(10, 10))); // on boundary
    /// assert!(!bbox.contains(&FPCoordinate::new(5, 15))); // outside
    /// ```
    pub fn contains(&self, coordinate: &FPCoordinate) -> bool {
        coordinate.lat >= self.min.lat
            && coordinate.lat <= self.max.lat
            && coordinate.lon >= self.min.lon
            && coordinate.lon <= self.max.lon
    }

    /// Calculates the minimum distance from a coordinate to the bounding box
    ///
    /// If the coordinate lies inside the bounding box, the distance is 0.
    /// Otherwise, it returns the shortest distance to any part of the box's boundary.
    ///
    /// # Arguments
    /// * `coordinate` - The coordinate to measure distance to
    ///
    /// # Returns
    /// The minimum distance in kilometers
    ///
    /// # Examples
    /// ```rust
    /// use toolbox_rs::geometry::FPCoordinate;
    /// use toolbox_rs::bounding_box::BoundingBox;
    ///
    /// let bbox = BoundingBox::from_coordinates(&[
    ///     FPCoordinate::new_from_lat_lon(50.0, 10.0),
    ///     FPCoordinate::new_from_lat_lon(51.0, 11.0),
    /// ]);
    ///
    /// // Point inside -> distance is 0
    /// let inside = FPCoordinate::new_from_lat_lon(50.5, 10.5);
    /// assert_eq!(bbox.min_distance(&inside), 0.0);
    ///
    /// // Point outside -> positive distance
    /// let outside = FPCoordinate::new_from_lat_lon(49.0, 10.5);
    /// assert!(bbox.min_distance(&outside) > 0.0);
    /// ```
    pub fn min_distance(&self, coordinate: &FPCoordinate) -> f64 {
        if self.contains(coordinate) {
            return 0.;
        }

        let c1 = self.max;
        let c2 = self.min;
        let c3 = FPCoordinate::new(c1.lat, c2.lon);
        let c4 = FPCoordinate::new(c2.lat, c1.lon);

        c1.distance_to(coordinate)
            .min(c2.distance_to(coordinate))
            .min(c3.distance_to(coordinate))
            .min(c4.distance_to(coordinate))
    }

    pub fn is_valid(&self) -> bool {
        self.min.lat <= self.max.lat && self.min.lon <= self.max.lon
    }

    pub fn from_coordinate(coordinate: &FPCoordinate) -> BoundingBox {
        BoundingBox {
            min: *coordinate,
            max: *coordinate,
        }
    }
}

impl From<&BoundingBox> for geojson::Bbox {
    fn from(bbox: &BoundingBox) -> geojson::Bbox {
        let result = vec![
            bbox.min.lon as f64 / 1000000.,
            bbox.min.lat as f64 / 1000000.,
            bbox.max.lon as f64 / 1000000.,
            bbox.max.lat as f64 / 1000000.,
        ];
        result
    }
}

#[cfg(test)]
mod tests {
    use crate::{bounding_box::BoundingBox, geometry::FPCoordinate};

    #[test]
    fn grid() {
        let mut coordinates: Vec<FPCoordinate> = Vec::new();
        for i in 0..100 {
            coordinates.push(FPCoordinate::new(i / 10, i % 10));
        }

        let expected = BoundingBox {
            min: FPCoordinate::new(0, 0),
            max: FPCoordinate::new(9, 9),
        };
        assert!(expected.is_valid());
        let result = BoundingBox::from_coordinates(&coordinates);
        assert_eq!(expected, result);
    }

    #[test]
    fn center() {
        let bbox = BoundingBox {
            min: FPCoordinate::new_from_lat_lon(33.406637, -115.000801),
            max: FPCoordinate::new_from_lat_lon(33.424732, -114.905286),
        };
        assert!(bbox.is_valid());
        let center = bbox.center();
        assert_eq!(center, FPCoordinate::new(33415684, -114953044));
    }

    #[test]
    fn center_with_rounding() {
        let bbox = BoundingBox {
            min: FPCoordinate::new(0, 0),
            max: FPCoordinate::new(9, 9),
        };
        assert!(bbox.is_valid());
        let center = bbox.center();
        assert_eq!(center, FPCoordinate::new(4, 4));
    }

    #[test]
    fn center_without_rounding() {
        let bbox = BoundingBox {
            min: FPCoordinate::new(0, 0),
            max: FPCoordinate::new(100, 100),
        };
        assert!(bbox.is_valid());
        let center = bbox.center();
        assert_eq!(center, FPCoordinate::new(50, 50));
    }

    #[test]
    fn invalid() {
        let bbox = BoundingBox::invalid();
        assert!(bbox.min.lat > bbox.max.lat);
        assert!(bbox.min.lon > bbox.max.lon);
    }

    #[test]
    fn extend_with_extend_invalid() {
        let mut c1 = BoundingBox::invalid();
        let c2 =
            BoundingBox::from_coordinates(&[FPCoordinate::new(11, 50), FPCoordinate::new(50, 37)]);
        c1.extend_with(&c2);
        assert!(c1.is_valid());

        assert_eq!(c2.min, FPCoordinate::new(11, 37));
        assert_eq!(c2.max, FPCoordinate::new(50, 50));
    }

    #[test]
    fn extend_with_merge_two_valid() {
        let mut b1 =
            BoundingBox::from_coordinates(&[FPCoordinate::new(10, 10), FPCoordinate::new(20, 20)]);

        let b2 =
            BoundingBox::from_coordinates(&[FPCoordinate::new(15, 15), FPCoordinate::new(25, 25)]);

        b1.extend_with(&b2);

        assert_eq!(b1.min, FPCoordinate::new(10, 10));
        assert_eq!(b1.max, FPCoordinate::new(25, 25));

        println!("{:?}", b1);

        assert!(b1.is_valid());
    }

    #[test]
    fn geojson_conversion() {
        let b1 =
            BoundingBox::from_coordinates(&[FPCoordinate::new(11, 50), FPCoordinate::new(50, 37)]);
        let g1 = geojson::Bbox::from(&b1);
        assert_eq!(4, g1.len());

        assert_eq!(b1.min.lon as f64 / 1000000., g1[0]);
        assert_eq!(b1.min.lat as f64 / 1000000., g1[1]);
        assert_eq!(b1.max.lon as f64 / 1000000., g1[2]);
        assert_eq!(b1.max.lat as f64 / 1000000., g1[3]);
    }

    #[test]
    fn extend_with_longitude_extension() {
        let mut b1 = BoundingBox::from_coordinates(&[
            FPCoordinate::new(10, -20), // lat=10, lon=-20
            FPCoordinate::new(15, -10), // lat=15, lon=-10
        ]);

        let b2 = BoundingBox::from_coordinates(&[
            FPCoordinate::new(12, 0),  // lat=12, lon=0
            FPCoordinate::new(14, 10), // lat=14, lon=10
        ]);

        // Initial checks
        assert_eq!(b1.max.lon, -10);

        // Extend b1 with b2
        b1.extend_with(&b2);

        // Verify longitude extension
        assert_eq!(b1.min.lon, -20); // Should keep original western boundary
        assert_eq!(b1.max.lon, 10); // Should extend eastern boundary

        assert!(b1.is_valid());
    }

    #[test]
    fn test_contains() {
        let bbox =
            BoundingBox::from_coordinates(&[FPCoordinate::new(10, 10), FPCoordinate::new(20, 20)]);

        // Test points inside the bounding box
        assert!(bbox.contains(&FPCoordinate::new(15, 15)));
        assert!(bbox.contains(&FPCoordinate::new(10, 10))); // boundary
        assert!(bbox.contains(&FPCoordinate::new(20, 20))); // boundary

        // Test points outside the bounding box
        assert!(!bbox.contains(&FPCoordinate::new(9, 15))); // west
        assert!(!bbox.contains(&FPCoordinate::new(21, 15))); // east
        assert!(!bbox.contains(&FPCoordinate::new(15, 9))); // south
        assert!(!bbox.contains(&FPCoordinate::new(15, 21))); // north
    }

    #[test]
    fn test_min_distance() {
        let bbox =
            BoundingBox::from_coordinates(&[FPCoordinate::new(10, 10), FPCoordinate::new(20, 20)]);

        // Test point inside -> distance should be 0
        assert_eq!(bbox.min_distance(&FPCoordinate::new(15, 15)), 0.0);

        // Test points outside
        let corner_point = FPCoordinate::new(10, 10);
        let distance_to_corner = bbox.min_distance(&FPCoordinate::new(5, 5));
        assert!(distance_to_corner > 0.0);
        assert_eq!(
            distance_to_corner,
            corner_point.distance_to(&FPCoordinate::new(5, 5))
        );

        // Test point directly east of box
        let east_point = FPCoordinate::new(15, 25);
        let distance_east = bbox.min_distance(&east_point);
        assert!(distance_east > 0.0);
        assert_eq!(
            distance_east,
            FPCoordinate::new(20, 20).distance_to(&east_point)
        );
    }

    #[test]
    fn test_from_coordinate() {
        let coord = FPCoordinate::new(15, 25);
        let bbox = BoundingBox::from_coordinate(&coord);

        assert_eq!(bbox.min, coord);
        assert_eq!(bbox.max, coord);
        assert!(bbox.is_valid());
        assert!(bbox.contains(&coord));
        assert_eq!(bbox.min_distance(&coord), 0.0);
    }
}
