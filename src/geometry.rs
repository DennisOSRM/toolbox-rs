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

    #[test]
    pub fn distance_one() {
        let p = Point { x: 1., y: 2. };
        let s = Segment(Point { x: 0., y: 0. }, Point { x: 0., y: 10. });
        let (distance, location) = distance_to_segment(p, s);
        assert_eq!(distance, 1.);
        assert_eq!(location, Point { x: 0., y: 2. });
    }
}
