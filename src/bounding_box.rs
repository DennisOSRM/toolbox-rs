use crate::geometry::FPCoordinate;

#[derive(Clone, Copy, Debug, Eq, PartialEq, rkyv::Archive, rkyv::Serialize, rkyv::Deserialize)]
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

    /// Whether two boxes share any ground, edges and corners included.
    ///
    /// Two boxes miss each other exactly when one lies wholly to one side of
    /// the other on either axis, which is four comparisons and no arithmetic.
    #[must_use]
    pub fn intersects(&self, other: &BoundingBox) -> bool {
        self.min.lat <= other.max.lat
            && other.min.lat <= self.max.lat
            && self.min.lon <= other.max.lon
            && other.min.lon <= self.max.lon
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
        self.nearest_point(coordinate).distance_to(coordinate)
    }

    /// The point of the box nearest the coordinate, taken axis by axis.
    ///
    /// It is a corner only when the coordinate lies past one. Beside an edge
    /// it is the point on that edge level with the coordinate, and asking the
    /// corners alone hands back half the length of the edge for a coordinate
    /// that sits right up against the middle of it. That is not merely a loose
    /// answer: a nearest first walk keys its nodes on this, and a key that
    /// overshoots what lies under the node hands out a far element before a
    /// near one.
    ///
    /// Clamping each axis in turn is the nearest point of a rectangle in the
    /// plane. It is not the nearest point of the patch of a sphere the same
    /// four numbers describe, so a measure taken over the sphere still reads a
    /// little long from here.
    #[must_use]
    pub fn nearest_point(&self, coordinate: &FPCoordinate) -> FPCoordinate {
        FPCoordinate::new(
            coordinate.lat.max(self.min.lat).min(self.max.lat),
            coordinate.lon.max(self.min.lon).min(self.max.lon),
        )
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
            FPCoordinate::new(15, 20).distance_to(&east_point),
            "the nearest point of the box is on its edge, level with the point"
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

    #[test]
    fn a_point_beside_a_long_edge_is_measured_to_that_edge() {
        // a box far wider than it is tall, and a point a little above the
        // middle of its top edge. The corners are half a width away; the edge
        // is right there.
        let wide = BoundingBox::from_coordinates(&[
            FPCoordinate::new(0, 0),
            FPCoordinate::new(1_000, 10_000_000),
        ]);
        let above = FPCoordinate::new(1_100, 5_000_000);

        let to_edge = FPCoordinate::new(1_000, 5_000_000).distance_to(&above);
        let to_nearest_corner = FPCoordinate::new(1_000, 0)
            .distance_to(&above)
            .min(FPCoordinate::new(1_000, 10_000_000).distance_to(&above));

        assert_eq!(wide.min_distance(&above), to_edge);
        assert!(
            to_nearest_corner > to_edge * 100.,
            "the corners are nowhere near, which is what made this worth fixing"
        );
    }

    #[test]
    fn a_point_past_a_corner_is_measured_to_that_corner() {
        let one = BoundingBox::from_coordinates(&[
            FPCoordinate::new(0, 0),
            FPCoordinate::new(1_000, 1_000),
        ]);
        let beyond = FPCoordinate::new(2_000, 2_000);
        assert_eq!(
            one.min_distance(&beyond),
            FPCoordinate::new(1_000, 1_000).distance_to(&beyond)
        );
    }

    #[test]
    fn a_box_is_never_further_off_than_anything_in_it() {
        // what a nearest first walk leans on: the distance to a box is at most
        // the distance to any point of it
        let one = BoundingBox::from_coordinates(&[
            FPCoordinate::new(-500, -700),
            FPCoordinate::new(900, 1_300),
        ]);
        for lat in (-2_000..2_000).step_by(311) {
            for lon in (-2_000..2_000).step_by(457) {
                let from = FPCoordinate::new(lat, lon);
                let to_box = one.min_distance(&from);
                for inside_lat in (-500..=900).step_by(233) {
                    for inside_lon in (-700..=1_300).step_by(347) {
                        let inside = FPCoordinate::new(inside_lat, inside_lon);
                        assert!(
                            to_box <= inside.distance_to(&from) + 1e-9,
                            "{to_box} to the box, {} to a point of it",
                            inside.distance_to(&from)
                        );
                    }
                }
            }
        }
    }

    fn box_of(min_lat: i32, min_lon: i32, max_lat: i32, max_lon: i32) -> BoundingBox {
        BoundingBox::from_coordinates(&[
            FPCoordinate::new(min_lat, min_lon),
            FPCoordinate::new(max_lat, max_lon),
        ])
    }

    #[test]
    fn boxes_that_lie_over_each_other_intersect() {
        let one = box_of(0, 0, 10, 10);
        assert!(
            one.intersects(&box_of(5, 5, 15, 15)),
            "a corner over a corner"
        );
        assert!(one.intersects(&box_of(2, 2, 3, 3)), "one inside the other");
        assert!(
            box_of(2, 2, 3, 3).intersects(&one),
            "and the other way round"
        );
        assert!(one.intersects(&one), "a box lies over itself");
    }

    #[test]
    fn boxes_that_only_touch_still_intersect() {
        let one = box_of(0, 0, 10, 10);
        assert!(one.intersects(&box_of(10, 10, 20, 20)), "corner to corner");
        assert!(one.intersects(&box_of(10, 0, 20, 10)), "edge to edge");
    }

    #[test]
    fn boxes_to_one_side_of_each_other_do_not_intersect() {
        let one = box_of(0, 0, 10, 10);
        assert!(!one.intersects(&box_of(11, 0, 20, 10)), "wholly north");
        assert!(!one.intersects(&box_of(0, 11, 10, 20)), "wholly east");
        assert!(
            !box_of(-20, -20, -11, -11).intersects(&one),
            "wholly southwest"
        );
        // near in one axis is not near at all if it misses in the other
        assert!(!one.intersects(&box_of(5, 11, 15, 20)));
    }
}
