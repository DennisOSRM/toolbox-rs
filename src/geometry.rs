pub mod primitives {

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub struct FPCoordinate {
        pub lat: i32,
        pub lon: i32,
    }

    impl FPCoordinate {
        pub fn new(lat: i32, lon: i32) -> Self {
            Self { lat, lon }
        }

        pub fn to_lon_lat_vec(&self) -> Vec<f64> {
            vec![self.lon as f64 / 1000000., self.lat as f64 / 1000000.]
        }
    }

    pub fn cross_product(o: &FPCoordinate, a: &FPCoordinate, b: &FPCoordinate) -> i32 {
        (a.lon - o.lon) * (b.lat - o.lat) - (a.lat - o.lat) * (b.lon - o.lon)
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    pub struct Point {
        pub x: f64,
        pub y: f64,
    }

    impl Point {
        pub fn new() -> Self {
            Point { x: 0., y: 0. }
        }
    }

    impl Default for Point {
        fn default() -> Self {
            Self::new()
        }
    }

    pub struct Segment(pub Point, pub Point);

    pub fn distance_to_segment(point: Point, segment: Segment) -> (f64, Point) {
        let mut dx = segment.1.x - segment.0.x;
        let mut dy = segment.1.y - segment.0.y;

        if (dx == 0.) && (dy == 0.) {
            // It's a point not a line segment.
            dx = point.x - segment.0.x;
            dy = point.y - segment.0.y;
            return ((dx * dx + dy * dy).sqrt(), segment.0);
        }

        // Calculate the t that minimizes the distance.
        let t = ((point.x - segment.0.x) * dx + (point.y - segment.0.y) * dy) / (dx * dx + dy * dy);

        // See if this represents one of the segment's
        // end points or a point in the middle.
        let closest;
        if t < 0. {
            closest = segment.0;
            dx = point.x - segment.0.x;
            dy = point.y - segment.0.y;
        } else if t > 1. {
            closest = Point {
                x: segment.1.x,
                y: segment.1.y,
            };
            dx = point.x - segment.1.x;
            dy = point.y - segment.1.y;
        } else {
            closest = Point {
                x: segment.0.x + t * dx,
                y: segment.0.y + t * dy,
            };
            dx = point.x - closest.x;
            dy = point.y - closest.y;
        }

        ((dx * dx + dy * dy).sqrt(), closest)
    }
}

#[cfg(test)]
mod tests {
    use crate::geometry::primitives::{distance_to_segment, Point, Segment};

    use super::primitives::{cross_product, FPCoordinate};

    #[test]
    pub fn distance_one() {
        let p = Point { x: 1., y: 2. };
        let s = Segment(Point { x: 0., y: 0. }, Point { x: 0., y: 10. });
        let (distance, location) = distance_to_segment(p, s);
        assert_eq!(distance, 1.);
        assert_eq!(location, Point { x: 0., y: 2. });
    }

    #[test]
    pub fn cross_product_ex1() {
        let o = FPCoordinate::new(1, 1);
        let a = FPCoordinate::new(4, 5);
        let b = FPCoordinate::new(5, 4);
        assert_eq!(7, cross_product(&o, &a, &b))
    }

    #[test]
    pub fn cross_product_ex2() {
        let o = FPCoordinate::new(0, 0);
        let a = FPCoordinate::new(7, 5);
        let b = FPCoordinate::new(17, 13);
        assert_eq!(-6, cross_product(&o, &a, &b))
    }

    #[test]
    pub fn cross_product_ex3() {
        let o = FPCoordinate::new(0, 0);
        let a = FPCoordinate::new(2, 2);
        let b = FPCoordinate::new(0, -3);
        assert_eq!(6, cross_product(&o, &a, &b))
    }
}
