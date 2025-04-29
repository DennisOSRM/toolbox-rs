use std::fmt::Display;

/// A fixed-point coordinate representation for geographical locations.
///
/// This struct stores latitude and longitude as fixed-point numbers
/// (integers multiplied by 1,000,000) to avoid floating-point precision issues
/// while maintaining 6 decimal places of precision.
///
/// # Representation
/// - `lat`: Latitude in millionths of a degree (-90.000000 to +90.000000)
/// - `lon`: Longitude in millionths of a degree (-180.000000 to +180.000000)
///
/// # Examples
///
/// ```
/// use toolbox_rs::geometry::FPCoordinate;
///
/// // Create from raw fixed-point values
/// let coord = FPCoordinate::new(40730610, -73935242);
///
/// // Create from floating-point degrees
/// let ny = FPCoordinate::new_from_lat_lon(40.730610, -73.935242);
///
/// // Convert back to floating-point
/// let (lon, lat) = ny.to_lon_lat_pair();
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq, bincode::Decode, bincode::Encode)]
pub struct FPCoordinate {
    pub lat: i32,
    pub lon: i32,
}

impl FPCoordinate {
    pub const fn new(lat: i32, lon: i32) -> Self {
        Self { lat, lon }
    }

    pub const fn min() -> Self {
        Self {
            lat: i32::MIN,
            lon: i32::MIN,
        }
    }

    pub const fn max() -> Self {
        Self {
            lat: i32::MAX,
            lon: i32::MAX,
        }
    }

    pub fn new_from_lat_lon(lat: f64, lon: f64) -> Self {
        let lat = (lat * 1000000.) as i32;
        let lon = (lon * 1000000.) as i32;

        Self { lat, lon }
    }

    pub fn to_lon_lat_pair(&self) -> (f64, f64) {
        (self.lon as f64 / 1000000., self.lat as f64 / 1000000.)
    }

    pub fn distance(first: &FPCoordinate, second: &FPCoordinate) -> f64 {
        let (lona, lata) = first.to_lon_lat_pair();
        let (lonb, latb) = second.to_lon_lat_pair();
        crate::great_circle::haversine(lata, lona, latb, lonb)
    }
    pub fn to_lon_lat_vec(&self) -> Vec<f64> {
        let (lon, lat) = self.to_lon_lat_pair();
        vec![lon, lat]
    }

    /// Calculates the great circle distance between two coordinates using the Haversine formula
    ///
    /// The distance is returned in kilometers.
    ///
    /// # Examples
    ///
    /// ```
    /// use toolbox_rs::geometry::FPCoordinate;
    ///
    /// // Distance between New York and San Francisco
    /// let ny = FPCoordinate::new_from_lat_lon(40.730610, -73.935242);
    /// let sf = FPCoordinate::new_from_lat_lon(37.773972, -122.431297);
    ///
    /// let distance = ny.distance_to(&sf);
    /// assert!((distance - 4140.175).abs() < 0.001); // ~4140 km
    ///
    /// // Distance to self is always 0
    /// assert_eq!(ny.distance_to(&ny), 0.0);
    /// ```
    pub fn distance_to(&self, other: &FPCoordinate) -> f64 {
        distance(self, other)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point2D {
    pub x: f64,
    pub y: f64,
}

impl Point2D {
    pub fn new() -> Self {
        Point2D { x: 0., y: 0. }
    }
}

impl Default for Point2D {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Segment(pub Point2D, pub Point2D);

pub fn distance_to_segment_2d(point: &Point2D, segment: &Segment) -> (f64, Point2D) {
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
        closest = Point2D {
            x: segment.1.x,
            y: segment.1.y,
        };
        dx = point.x - segment.1.x;
        dy = point.y - segment.1.y;
    } else {
        closest = Point2D {
            x: segment.0.x + t * dx,
            y: segment.0.y + t * dy,
        };
        dx = point.x - closest.x;
        dy = point.y - closest.y;
    }

    ((dx * dx + dy * dy).sqrt(), closest)
}

impl Display for FPCoordinate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{:.6}, {:.6}",
            self.lat as f64 / 1000000.,
            self.lon as f64 / 1000000.
        )
    }
}

pub const fn cross_product(o: &FPCoordinate, a: &FPCoordinate, b: &FPCoordinate) -> i64 {
    // upcasting to i64 to avoid integer overflow
    let first = (a.lon as i64 - o.lon as i64) * (b.lat as i64 - o.lat as i64);
    let second = (a.lat as i64 - o.lat as i64) * (b.lon as i64 - o.lon as i64);
    first - second
}

pub const fn is_clock_wise_turn(o: &FPCoordinate, a: &FPCoordinate, b: &FPCoordinate) -> bool {
    // upcasting to i64 to avoid integer overflow
    let first = (a.lon as i64 - o.lon as i64) * (b.lat as i64 - o.lat as i64);
    let second = (a.lat as i64 - o.lat as i64) * (b.lon as i64 - o.lon as i64);
    first > second
}

pub fn distance(first: &FPCoordinate, b: &FPCoordinate) -> f64 {
    let (lona, lata) = first.to_lon_lat_pair();
    let (lonb, latb) = b.to_lon_lat_pair();
    crate::great_circle::haversine(lata, lona, latb, lonb)
}

/// A 2D point with integer coordinates, primarily used for TSP problems
/// where distances need to be rounded to the nearest integer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IPoint2D {
    pub x: i32,
    pub y: i32,
}

impl IPoint2D {
    pub fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    /// Computes the Euclidean distance to another point, rounded to the nearest integer.
    ///
    /// This follows the TSPLIB standard for EUC_2D edge weight type:
    /// dij = nint(sqrt(xd*xd + yd*yd)) where xd = xi-xj and yd = yi-yj
    pub fn distance_to(&self, other: &IPoint2D) -> i32 {
        let xd = self.x - other.x;
        let yd = self.y - other.y;
        let d = ((xd * xd + yd * yd) as f64).sqrt();
        d.round() as i32
    }
}

#[cfg(test)]
mod tests {
    macro_rules! assert_delta {
        ($x:expr, $y:expr, $d:expr) => {
            if !($x - $y < $d || $y - $x < $d) {
                panic!();
            }
        };
    }

    use super::{FPCoordinate, cross_product, distance};
    use crate::geometry::{IPoint2D, Point2D, Segment, distance_to_segment_2d, is_clock_wise_turn};

    #[test]
    fn distance_one() {
        let p = Point2D { x: 1., y: 2. };
        let s = Segment(Point2D { x: 0., y: 0. }, Point2D { x: 0., y: 10. });
        let (distance, location) = distance_to_segment_2d(&p, &s);
        assert_eq!(distance, 1.);
        assert_eq!(location, Point2D { x: 0., y: 2. });
    }

    #[test]
    fn distance_on_line() {
        let p = Point2D { x: 1., y: 2. };
        let s = Segment(Point2D { x: 0., y: 0. }, Point2D { x: 0., y: 0. });
        let (distance, location) = distance_to_segment_2d(&p, &s);
        assert_eq!(distance, 2.23606797749979);
        assert_eq!(location, Point2D { x: 0., y: 0. });
    }

    #[test]
    fn distance_to_zero_length_segment() {
        let point = Point2D { x: 3.0, y: 4.0 };
        let segment_point = Point2D { x: 0.0, y: 0.0 };
        let segment = Segment(segment_point, segment_point); // Zero-length segment (point)

        let (distance, closest) = distance_to_segment_2d(&point, &segment);

        // Expected distance is 5.0 (using Pythagorean theorem: sqrt(3² + 4²))
        assert_eq!(distance, 5.0);
        // Closest point should be the segment point
        assert_eq!(closest, segment_point);
    }

    #[test]
    fn distance_beyond_segment_end() {
        let point = Point2D { x: 0.0, y: 3.0 };
        let segment = Segment(Point2D { x: 0.0, y: 0.0 }, Point2D { x: 0.0, y: 2.0 });

        let (distance, closest) = distance_to_segment_2d(&point, &segment);

        // Point is 1 unit beyond the end of the segment
        assert_eq!(distance, 1.0);
        // Closest point should be the segment endpoint
        assert_eq!(closest, segment.1);
    }

    #[test]
    fn distance_beyond_diagonal_segment_end() {
        let point = Point2D { x: 3.0, y: 3.0 };
        let segment = Segment(Point2D { x: 0.0, y: 0.0 }, Point2D { x: 2.0, y: 2.0 });

        let (distance, closest) = distance_to_segment_2d(&point, &segment);

        // Point is (1,1) away from segment end at (2,2)
        assert_delta!(distance, 2_f64.sqrt(), 0.000001);
        // Closest point should be the segment endpoint
        assert_eq!(closest, segment.1);
    }

    #[test]
    fn distance_before_segment_start() {
        let point = Point2D { x: -1.0, y: -1.0 };
        let segment = Segment(Point2D { x: 0.0, y: 0.0 }, Point2D { x: 2.0, y: 2.0 });

        let (distance, closest) = distance_to_segment_2d(&point, &segment);

        // Point is (-1,-1) away from segment start at (0,0)
        assert_delta!(distance, 2_f64.sqrt(), 0.000001);
        // Closest point should be the segment start point
        assert_eq!(closest, segment.0);
    }

    #[test]
    fn cross_product_ex1() {
        let o = FPCoordinate::new(1, 1);
        let a = FPCoordinate::new(4, 5);
        let b = FPCoordinate::new(5, 4);
        assert_eq!(7, cross_product(&o, &a, &b))
    }

    #[test]
    fn cross_product_ex2() {
        let o = FPCoordinate::new(0, 0);
        let a = FPCoordinate::new(7, 5);
        let b = FPCoordinate::new(17, 13);
        assert_eq!(-6, cross_product(&o, &a, &b))
    }

    #[test]
    fn cross_product_ex3() {
        let o = FPCoordinate::new(0, 0);
        let a = FPCoordinate::new(2, 2);
        let b = FPCoordinate::new(0, -3);
        assert_eq!(6, cross_product(&o, &a, &b))
    }

    #[test]
    fn clock_wise_turn() {
        let o = FPCoordinate::new_from_lat_lon(33.376756, -114.990162);
        let a = FPCoordinate::new_from_lat_lon(33.359699, -114.945064);
        let b = FPCoordinate::new_from_lat_lon(33.412820, -114.943641);

        let cp = cross_product(&o, &a, &b);
        assert!(cp > 0);
        assert!(is_clock_wise_turn(&o, &a, &b));
    }

    #[test]
    fn println() {
        let input = FPCoordinate::new_from_lat_lon(33.359699, -114.945064);
        let output = format!("{input}");
        assert_eq!(output, "33.359699, -114.945064");
    }

    #[test]
    fn println_truncated() {
        let input = FPCoordinate::new_from_lat_lon(33.359_699_123_456_79, -114.945064127454);
        let output = format!("{input}");
        assert_eq!(output, "33.359699, -114.945064");
    }

    #[test]
    fn to_lon_lat_pair_vec_equivalent() {
        let input = FPCoordinate::new_from_lat_lon(33.359_699_123_456_79, -114.945064127454);
        let output1 = input.to_lon_lat_pair();
        let output2 = input.to_lon_lat_vec();

        assert_eq!(output1.0, output2[0]);
        assert_eq!(output1.1, output2[1]);
    }

    #[test]
    fn trivial_distance_equivalent() {
        let ny = FPCoordinate::new_from_lat_lon(40.730610, -73.935242);
        let sf = FPCoordinate::new_from_lat_lon(37.773972, -122.431297);
        let distance = distance(&ny, &sf);

        assert_delta!(distance, 4140.175105689902, 0.0000001);
    }

    #[test]
    fn point_self_new_equivalent() {
        let p1 = Point2D::default();
        let p2 = Point2D::new();
        assert_eq!(p1, p2);
    }

    #[test]
    fn test_distance_to() {
        let ny = FPCoordinate::new_from_lat_lon(40.730610, -73.935242); // New York
        let sf = FPCoordinate::new_from_lat_lon(37.773972, -122.431297); // San Francisco

        // Test symmetry
        let d1 = ny.distance_to(&sf);
        let d2 = sf.distance_to(&ny);
        assert_eq!(d1, d2);

        // Test known distance (approximately 4140km)
        assert_delta!(d1, 4140.175105689902, 0.0000001);

        // Test zero distance to self
        assert_eq!(ny.distance_to(&ny), 0.0);
    }

    #[test]
    fn test_coordinate_encoding() {
        let original = FPCoordinate::new_from_lat_lon(40.730610, -73.935242);

        // Encode to bytes
        let encoded: Vec<u8> =
            bincode::encode_to_vec(original, bincode::config::standard()).unwrap();

        // Decode from bytes
        let decoded: FPCoordinate =
            bincode::decode_from_slice(&encoded, bincode::config::standard())
                .unwrap()
                .0;

        // Verify the roundtrip
        assert_eq!(original, decoded);
        assert_eq!(original.lat, decoded.lat);
        assert_eq!(original.lon, decoded.lon);

        // Verify actual values preserved
        let (lon, lat) = decoded.to_lon_lat_pair();
        assert_delta!(lat, 40.730610, 0.000001);
        assert_delta!(lon, -73.935242, 0.000001);
    }

    #[test]
    fn test_ipoint2d_distance() {
        // Test cases from TSPLIB documentation examples
        let cases = [
            ((0, 0), (3, 0), 3), // Horizontal line
            ((0, 0), (0, 4), 4), // Vertical line
            ((0, 0), (3, 4), 5), // 3-4-5 triangle
            ((1, 1), (4, 5), 5), // Diagonal with rounding
            ((2, 2), (5, 6), 5), // Another diagonal
        ];

        for ((x1, y1), (x2, y2), expected) in cases {
            let p1 = IPoint2D::new(x1, y1);
            let p2 = IPoint2D::new(x2, y2);
            assert_eq!(p1.distance_to(&p2), expected);
            // Test symmetry
            assert_eq!(p2.distance_to(&p1), expected);
        }
    }
}
