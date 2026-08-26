//! The measure a spatial search works in.
//!
//! A search that hands out elements nearest first needs two numbers: how far
//! away a thing is, and how far away the nearest thing under a box could
//! possibly be. The walk keys every node on the second and opens the queue in
//! order, which is only right while the second is never larger than the first
//! for anything under that node. A key that overshoots holds a whole subtree
//! back behind elements that are further away.
//!
//! The two therefore have to agree, and keeping them in separate places is how
//! they stop agreeing: measuring in one and bounding in another is a pairing
//! nothing checks. A metric holds both, so the pairing is a property of the
//! implementation rather than a coincidence between two modules.

use crate::{
    bounding_box::BoundingBox,
    geometry::{FPCoordinate, Point2D, Segment, distance_to_segment_2d},
};

/// How far apart two points are, and how near a box could be.
///
/// # Contract
///
/// `min_distance(box, p)` must never exceed `distance(q, p)` for any point `q`
/// of the box. Being under is allowed and only costs a node opened too early;
/// being over is what puts the answers in the wrong order.
pub trait Metric {
    /// How far apart two points are.
    fn distance(&self, first: &FPCoordinate, second: &FPCoordinate) -> f64;

    /// How near the box could be, which is never further than anything in it.
    fn min_distance(&self, bbox: &BoundingBox, coordinate: &FPCoordinate) -> f64;

    /// How near the piece of road between two points is, and where on it.
    ///
    /// Written on [`distance`](Self::distance) by default, which answers with
    /// whichever end is nearer -- right for a measure that has no straight
    /// line in it, and loose for one that has. A measure with a plane under it
    /// should say where between the ends the near point falls, since that is
    /// the answer a caller snapping to a road wants.
    ///
    /// # Contract
    ///
    /// The same one: `min_distance` of a box holding both ends must never
    /// exceed what this returns.
    fn distance_to_segment(
        &self,
        at: &FPCoordinate,
        from: &FPCoordinate,
        to: &FPCoordinate,
    ) -> (f64, FPCoordinate) {
        let (one, other) = (self.distance(at, from), self.distance(at, to));
        if one <= other {
            (one, *from)
        } else {
            (other, *to)
        }
    }
}

/// Great-circle distance over a sphere, in kilometres.
///
/// The bound is the distance to the box's nearest point taken axis by axis,
/// which is the nearest point of a flat rectangle and not of a spherical one:
/// it can read too large beside a long meridian, over a pole, and across the
/// antimeridian, which breaks the contract above. See the tracking issue on
/// `min_distance`. It is the measure the tree has always used and the bound it
/// has always used, kept together here so the mismatch has somewhere to be
/// fixed.
#[derive(Clone, Copy, Debug, Default)]
pub struct Haversine;

impl Metric for Haversine {
    fn distance(&self, first: &FPCoordinate, second: &FPCoordinate) -> f64 {
        first.distance_to(second)
    }

    fn min_distance(&self, bbox: &BoundingBox, coordinate: &FPCoordinate) -> f64 {
        bbox.min_distance(coordinate)
    }
}

/// Straight-line distance treating latitude and longitude as a plane, in the
/// fixed-point units the coordinates are held in rather than in metres.
///
/// Nothing on a sphere is being approximated here, so a degree of longitude
/// counts the same at the equator and at the pole. That makes it the wrong
/// measure for ground distance and the right one for data already projected,
/// for an ordering that only has to be consistent, and for a test that wants
/// an answer it can work out by hand.
///
/// Its bound is exact: clamping each axis in turn does give the nearest point
/// of a rectangle in the plane.
#[derive(Clone, Copy, Debug, Default)]
pub struct Planar;

impl Metric for Planar {
    fn distance(&self, first: &FPCoordinate, second: &FPCoordinate) -> f64 {
        Scaled { lon_scale: 1.0 }.distance(first, second)
    }

    fn min_distance(&self, bbox: &BoundingBox, coordinate: &FPCoordinate) -> f64 {
        self.distance(&bbox.nearest_point(coordinate), coordinate)
    }

    fn distance_to_segment(
        &self,
        at: &FPCoordinate,
        from: &FPCoordinate,
        to: &FPCoordinate,
    ) -> (f64, FPCoordinate) {
        Scaled { lon_scale: 1.0 }.distance_to_segment(at, from, to)
    }
}

/// The same plane with longitude scaled, which is the one a search over a road
/// network wants.
///
/// # Why the scale
///
/// [`Planar`] counts a degree of longitude the same at the equator and at the
/// pole, so it ranks a thing due east nearer than it is: over a continent that
/// is wrong by a third. [`Haversine`] ranks correctly and its bound on a box
/// does not, which is the mismatch this module was written to have somewhere
/// to fix.
///
/// Scaling longitude by the cosine of the latitude the data sits at fixes
/// both. Within a continent it is within a percent of the great circle, so it
/// ranks as the great circle does; and it is a plane, so clamping each axis in
/// turn really is the nearest point of the box and the bound is exact. It is
/// the measure that satisfies the contract above rather than nearly satisfying
/// it.
///
/// It measures in the fixed-point units the coordinates are held in.
/// [`metres`](Self::metres) turns an answer into metres at the end, where it
/// is a distance and not a key.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Scaled {
    /// longitude times this is the plane
    pub lon_scale: f64,
}

/// How many fixed-point degrees make a degree.
const FIXED: f64 = 1_000_000.0;

/// Roughly how many metres a degree of latitude is.
const METRES_A_DEGREE: f64 = 111_320.0;

impl Scaled {
    /// The plane the middle of this box sits in.
    #[must_use]
    pub fn about(whole: &BoundingBox) -> Self {
        Self::about_latitude(whole.center().lat)
    }

    /// The plane a latitude sits in.
    #[must_use]
    pub fn about_latitude(lat: i32) -> Self {
        Self {
            lon_scale: (f64::from(lat) / FIXED).to_radians().cos(),
        }
    }

    /// A distance in this plane, in metres.
    #[must_use]
    pub fn metres(&self, away: f64) -> f64 {
        away * METRES_A_DEGREE / FIXED
    }

    /// A place in this plane.
    fn flat(&self, place: &FPCoordinate) -> Point2D {
        Point2D {
            x: f64::from(place.lon) * self.lon_scale,
            y: f64::from(place.lat),
        }
    }

    /// And back out of it.
    fn unflat(&self, place: Point2D) -> FPCoordinate {
        FPCoordinate::new(place.y as i32, (place.x / self.lon_scale) as i32)
    }
}

impl Metric for Scaled {
    fn distance(&self, first: &FPCoordinate, second: &FPCoordinate) -> f64 {
        let (a, b) = (self.flat(first), self.flat(second));
        (a.x - b.x).hypot(a.y - b.y)
    }

    fn min_distance(&self, bbox: &BoundingBox, coordinate: &FPCoordinate) -> f64 {
        self.distance(&bbox.nearest_point(coordinate), coordinate)
    }

    fn distance_to_segment(
        &self,
        at: &FPCoordinate,
        from: &FPCoordinate,
        to: &FPCoordinate,
    ) -> (f64, FPCoordinate) {
        let (away, near) =
            distance_to_segment_2d(&self.flat(at), &Segment(self.flat(from), self.flat(to)));
        (away, self.unflat(near))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_of(min_lat: i32, min_lon: i32, max_lat: i32, max_lon: i32) -> BoundingBox {
        BoundingBox::from_coordinates(&[
            FPCoordinate::new(min_lat, min_lon),
            FPCoordinate::new(max_lat, max_lon),
        ])
    }

    #[test]
    fn a_point_of_the_box_is_no_nearer_than_the_bound_says() {
        let bbox = box_of(0, 0, 100, 200);
        for lat in [-50, 0, 50, 100, 150] {
            for lon in [-50, 0, 100, 200, 300] {
                let coordinate = FPCoordinate::new(lat, lon);
                let bound = Planar.min_distance(&bbox, &coordinate);
                for corner_lat in [0, 50, 100] {
                    for corner_lon in [0, 100, 200] {
                        let inside = FPCoordinate::new(corner_lat, corner_lon);
                        assert!(
                            bound <= Planar.distance(&inside, &coordinate) + 1e-9,
                            "bound {bound} beats {inside:?} from {coordinate:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_planar_bound_is_the_nearest_point_and_not_a_corner() {
        // beside the middle of a long edge, where the nearest corner is far
        // along it and the nearest point is straight across
        let bbox = box_of(0, 0, 1000, 10);
        let beside = FPCoordinate::new(500, -10);
        assert!((Planar.min_distance(&bbox, &beside) - 10.).abs() < 1e-9);
    }

    #[test]
    fn a_point_inside_the_box_is_no_distance_from_it() {
        let bbox = box_of(0, 0, 100, 100);
        let inside = FPCoordinate::new(50, 50);
        assert_eq!(Planar.min_distance(&bbox, &inside), 0.);
        assert_eq!(Haversine.min_distance(&bbox, &inside), 0.);
    }

    #[test]
    fn the_planar_measure_counts_a_degree_the_same_everywhere() {
        // what the haversine does not do, and the reason to have both
        let equator = Planar.distance(&FPCoordinate::new(0, 0), &FPCoordinate::new(0, 1_000_000));
        let far_north = Planar.distance(
            &FPCoordinate::new(80_000_000, 0),
            &FPCoordinate::new(80_000_000, 1_000_000),
        );
        assert_eq!(equator, far_north);
        assert!(
            Haversine.distance(&FPCoordinate::new(0, 0), &FPCoordinate::new(0, 1_000_000))
                > 4. * Haversine.distance(
                    &FPCoordinate::new(80_000_000, 0),
                    &FPCoordinate::new(80_000_000, 1_000_000)
                )
        );
    }

    #[test]
    fn the_haversine_metric_is_what_the_coordinates_already_answer() {
        let first = FPCoordinate::new_from_lat_lon(50.1, 8.6);
        let second = FPCoordinate::new_from_lat_lon(48.1, 11.5);
        assert_eq!(
            Haversine.distance(&first, &second),
            first.distance_to(&second)
        );
    }
}
