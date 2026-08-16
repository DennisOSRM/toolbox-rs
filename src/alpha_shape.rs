//! The alpha shape of a set of points: a hull that is allowed to be concave.
//!
//! # What it is
//!
//! A convex hull is the shape a rubber band takes around a set of points. An
//! alpha shape is what a disc of radius alpha carves out of it: roll the disc
//! around the outside of the point set, and wherever it fits into a bay it
//! takes a bite out of the hull. A large alpha rolls over every bay and gives
//! the convex hull back. A small one eats into every gap until nothing but the
//! points themselves is left.
//!
//! What comes out is therefore a family of shapes with one dial, and the dial
//! is a length: alpha is the radius of the largest circle that may sit in a
//! hole without the hole being opened up.
//!
//! # How it is worked out
//!
//! Every edge of an alpha shape is an edge of the Delaunay triangulation, which
//! is what makes this cheap: triangulate once, then keep the triangles whose
//! circumcircle is no larger than alpha and drop the rest. The boundary of what
//! is kept is the alpha shape, and an edge is on that boundary when exactly one
//! kept triangle owns it.
//!
//! This is the alpha complex of the triangulation rather than the alpha shape
//! of the original definition. They differ only in that the original also keeps
//! an edge whose own alpha disc happens to be empty when both of its triangles
//! are dropped, which leaves a dangling edge rather than a region. A shape made
//! of regions is what a caller wants to draw, so this leaves those out.
//!
//! # Examples
//!
//! ```rust
//! use toolbox_rs::alpha_shape::{alpha_shape, triangulate};
//! use toolbox_rs::geometry::Point2D;
//!
//! // a square of four points, which one triangle of the pair cannot cover
//! let square = [
//!     Point2D { x: 0., y: 0. },
//!     Point2D { x: 4., y: 0. },
//!     Point2D { x: 4., y: 4. },
//!     Point2D { x: 0., y: 4. },
//! ];
//! assert_eq!(triangulate(&square).len(), 2);
//!
//! // a disc that fits around either triangle keeps both, so the shape is the
//! // square itself: one ring of four corners
//! let rings = alpha_shape(&square, 3.0);
//! assert_eq!(rings.len(), 1);
//! assert_eq!(rings[0].len(), 4);
//!
//! // and one that fits in neither leaves nothing at all
//! assert!(alpha_shape(&square, 0.5).is_empty());
//! ```
use crate::geometry::Point2D;
use rustc_hash::{FxHashMap, FxHashSet};

/// A triangle of the triangulation, as three indices into the points it was
/// built from, wound anticlockwise.
pub type Triangle = [usize; 3];

/// How far apart two points may be and still be taken for one. Points that
/// share a place have no triangle between them and would otherwise leave the
/// triangulation with a degenerate one.
const SAME_PLACE: f64 = 1e-12;

/// The centre and the squared radius of the circle through three points, and
/// `None` when they lie on a line and there is no such circle.
///
/// The points are moved to sit around the first of them before the arithmetic,
/// which is what keeps the difference of two large and nearly equal numbers out
/// of it.
fn circumcircle(a: Point2D, b: Point2D, c: Point2D) -> Option<(Point2D, f64)> {
    let (bx, by) = (b.x - a.x, b.y - a.y);
    let (cx, cy) = (c.x - a.x, c.y - a.y);
    let twice_area = 2.0 * (bx * cy - by * cx);
    if twice_area.abs() < f64::EPSILON {
        return None;
    }

    let b_squared = bx * bx + by * by;
    let c_squared = cx * cx + cy * cy;
    let x = (cy * b_squared - by * c_squared) / twice_area;
    let y = (bx * c_squared - cx * b_squared) / twice_area;
    Some((
        Point2D {
            x: a.x + x,
            y: a.y + y,
        },
        x * x + y * y,
    ))
}

/// Twice the signed area of a triangle, which is positive when its corners are
/// wound anticlockwise.
fn twice_signed_area(a: Point2D, b: Point2D, c: Point2D) -> f64 {
    (b.x - a.x) * (c.y - a.y) - (b.y - a.y) * (c.x - a.x)
}

/// A triangle that is not there, as an index that cannot be one.
const NOWHERE: usize = usize::MAX;

/// The triangles as they are being built, each knowing the three that lie
/// across its edges.
///
/// Edge `i` of a triangle runs from corner `i` to corner `i + 1`, and
/// `neighbours[i]` is what lies on the other side of it. Knowing that is the
/// whole difference between a triangulation that costs n log n and one that
/// costs n squared: without it, finding the triangles a new point displaces
/// means asking every triangle there is.
struct Mesh {
    corners: Vec<[usize; 3]>,
    neighbours: Vec<[usize; 3]>,
    alive: Vec<bool>,
    /// slots of triangles that have been taken apart, to be filled again
    free: Vec<usize>,
}

impl Mesh {
    fn new() -> Self {
        Self {
            corners: Vec::new(),
            neighbours: Vec::new(),
            alive: Vec::new(),
            free: Vec::new(),
        }
    }

    fn add(&mut self, corners: [usize; 3], neighbours: [usize; 3]) -> usize {
        if let Some(at) = self.free.pop() {
            self.corners[at] = corners;
            self.neighbours[at] = neighbours;
            self.alive[at] = true;
            return at;
        }
        self.corners.push(corners);
        self.neighbours.push(neighbours);
        self.alive.push(true);
        self.corners.len() - 1
    }

    fn remove(&mut self, at: usize) {
        self.alive[at] = false;
        self.free.push(at);
    }

    /// The triangle the point lands in, found by walking to it.
    ///
    /// From wherever it starts, the walk leaves across whichever edge the point
    /// lies on the far side of, and stops when the point is on the inside of
    /// all three. Because the points are inserted along a curve that keeps
    /// neighbours together, the walk is a step or two rather than a search.
    fn triangle_holding(&self, places: &[Point2D], point: Point2D, from: usize) -> usize {
        let mut at = from;
        // a walk cannot visit more triangles than there are, and the cap is
        // what keeps a numerical wobble from spinning here for ever
        for _ in 0..self.corners.len() + 8 {
            let corners = self.corners[at];
            let mut left = true;
            for side in 0..3 {
                let (a, b) = (places[corners[side]], places[corners[(side + 1) % 3]]);
                if twice_signed_area(a, b, point) < 0.0 {
                    let across = self.neighbours[at][side];
                    if across != NOWHERE {
                        at = across;
                        left = false;
                        break;
                    }
                }
            }
            if left {
                return at;
            }
        }
        at
    }
}

/// Where a point sits on a curve that runs through the plane without leaving
/// gaps, so that points near each other in space come out near each other in
/// the order.
///
/// This is the Hilbert curve, which is what makes the walk above cheap: each
/// point is inserted a short step from the one before it, so the triangle it
/// lands in is a step or two from the triangle the last one landed in.
fn along_the_curve(x: u32, y: u32) -> u64 {
    let (mut x, mut y) = (x, y);
    let mut along = 0_u64;
    let mut side = 1_u32 << 15;
    while side > 0 {
        let rx = u32::from((x & side) > 0);
        let ry = u32::from((y & side) > 0);
        along += u64::from(side) * u64::from(side) * u64::from((3 * rx) ^ ry);
        // turn the square so that the curve carries on where it left off
        if ry == 0 {
            if rx == 1 {
                x = side.wrapping_sub(1).wrapping_sub(x);
                y = side.wrapping_sub(1).wrapping_sub(y);
            }
            std::mem::swap(&mut x, &mut y);
        }
        side /= 2;
    }
    along
}

/// The Delaunay triangulation of a set of points, as triangles wound
/// anticlockwise.
///
/// This is the incremental construction of Bowyer and Watson: hold a
/// triangulation of everything seen so far, and for each new point throw away
/// the triangles whose circumcircle it falls inside, then fill the hole that
/// leaves by joining the point to each edge of its boundary. What is left over
/// has the Delaunay property, that no point lies inside the circumcircle of any
/// triangle.
///
/// Points that share a place are taken for one, and a set that lies on a single
/// line has no triangles and comes back empty.
///
/// # Cost
///
/// n log n, of which the log is the sort that puts the points in the order they
/// are inserted in. The rest is expected constant work per point: the triangle
/// a point lands in is found by walking to it from the last one, which is a
/// step or two when consecutive points are close together, and the triangles it
/// displaces are found by spreading out from there over neighbours rather than
/// by asking all of them. Measured on points drawn at random:
///
/// ```text
///     1000 points ->    1981 triangles in    0.8 ms
///    10000 points ->   19968 triangles in    7.9 ms
///   100000 points ->  199946 triangles in   93.1 ms
///  1000000 points -> 1999899 triangles in 1090.5 ms
/// ```
///
/// Ten times the points cost about twelve times the work, which is the ratio
/// n log n has at these sizes.
#[must_use]
pub fn triangulate(points: &[Point2D]) -> Vec<Triangle> {
    if points.len() < 3 {
        return Vec::new();
    }

    let (mut low, mut high) = (points[0], points[0]);
    for point in points {
        low = Point2D {
            x: low.x.min(point.x),
            y: low.y.min(point.y),
        };
        high = Point2D {
            x: high.x.max(point.x),
            y: high.y.max(point.y),
        };
    }

    // the points in the order they will be inserted, which is along a curve
    // that keeps neighbours together
    let span = (high.x - low.x).max(high.y - low.y).max(f64::MIN_POSITIVE);
    let mut order = (0..points.len()).collect::<Vec<_>>();
    let place_of = |point: Point2D| {
        let scale = f64::from(u32::MAX >> 16);
        let x = ((point.x - low.x) / span * scale) as u32;
        let y = ((point.y - low.y) / span * scale) as u32;
        along_the_curve(x, y)
    };
    let curve = points
        .iter()
        .map(|&point| place_of(point))
        .collect::<Vec<_>>();
    order.sort_unstable_by_key(|&at| curve[at]);

    // a triangle far enough out that every point falls inside it
    let middle = Point2D {
        x: f64::midpoint(low.x, high.x),
        y: f64::midpoint(low.y, high.y),
    };
    let far = (high.x - low.x).max(high.y - low.y).max(1.0) * 64.0;
    let mut places = points.to_vec();
    places.extend_from_slice(&[
        Point2D {
            x: middle.x - far,
            y: middle.y - far,
        },
        Point2D {
            x: middle.x + far,
            y: middle.y - far,
        },
        Point2D {
            x: middle.x,
            y: middle.y + far,
        },
    ]);
    let outer = [points.len(), points.len() + 1, points.len() + 2];

    let mut mesh = Mesh::new();
    let first = mesh.add(outer, [NOWHERE; 3]);
    let mut hint = first;

    // scratch that is kept between points rather than made afresh for each
    let mut cavity = Vec::new();
    // which point each triangle was last taken into the cavity for, so that
    // the cavity of one point does not have to be wiped before the next: a
    // wipe is a walk of every triangle, which is the quadratic cost coming
    // back in through the side door
    let mut taken_for: Vec<usize> = Vec::new();
    let mut stack = Vec::new();
    let mut border: Vec<(usize, usize, usize)> = Vec::new();
    let mut made: Vec<usize> = Vec::new();
    // Every point of a cell of the curve rather than the last of them: two
    // points a whisker apart share a cell, and remembering only the last means
    // a place that comes round again after one of them is not recognised. What
    // goes in is then inserted at a place that already holds a point, which is
    // a triangle of no area and a mesh that loses its way.
    //
    // The points of a cell are held as a chain through `earlier_in_cell`
    // rather than as a list per cell, which would be an allocation for each of
    // the cells a point ever lands in.
    let mut placed: FxHashMap<u64, usize> = FxHashMap::default();
    let mut earlier_in_cell = vec![NOWHERE; points.len()];

    for &index in &order {
        let point = points[index];
        // a point that sits where an earlier one does adds nothing
        let head = placed.get(&curve[index]).copied().unwrap_or(NOWHERE);
        let mut earlier = head;
        let mut seen_before = false;
        while earlier != NOWHERE {
            let there = points[earlier];
            if (there.x - point.x).abs() < SAME_PLACE && (there.y - point.y).abs() < SAME_PLACE {
                seen_before = true;
                break;
            }
            earlier = earlier_in_cell[earlier];
        }
        if seen_before {
            continue;
        }
        earlier_in_cell[index] = head;
        placed.insert(curve[index], index);

        // the hint is a triangle made for the point before this one, and
        // nothing takes triangles apart between then and here
        debug_assert!(
            mesh.alive[hint],
            "the walk starts from a triangle that is gone"
        );
        let holding = mesh.triangle_holding(&places, point, hint);

        // the triangles the point displaces, found by spreading out from the
        // one it landed in rather than by asking all of them
        taken_for.resize(mesh.corners.len(), NOWHERE);
        cavity.clear();
        stack.clear();
        stack.push(holding);
        taken_for[holding] = index;
        cavity.push(holding);
        while let Some(at) = stack.pop() {
            for side in 0..3 {
                let across = mesh.neighbours[at][side];
                if across == NOWHERE || taken_for[across] == index {
                    continue;
                }
                let corners = mesh.corners[across].map(|corner| places[corner]);
                let Some((centre, radius_squared)) =
                    circumcircle(corners[0], corners[1], corners[2])
                else {
                    continue;
                };
                let (dx, dy) = (point.x - centre.x, point.y - centre.y);
                // Strictly inside, and with a hair of room. A point that sits
                // on the circle is not inside it, and cocircular points are
                // what a grid or a pair of arcs is full of: counting them as
                // inside makes the decision differ between two triangles that
                // share an edge, and the hole then has a hole of its own.
                if dx * dx + dy * dy < radius_squared * (1.0 - 1e-9) {
                    taken_for[across] = index;
                    cavity.push(across);
                    stack.push(across);
                }
            }
        }

        // the boundary of the hole: the edges of it that face outwards
        border.clear();
        for &at in &cavity {
            for side in 0..3 {
                let across = mesh.neighbours[at][side];
                if across != NOWHERE && taken_for[across] == index {
                    continue;
                }
                border.push((
                    mesh.corners[at][side],
                    mesh.corners[at][(side + 1) % 3],
                    across,
                ));
            }
        }
        for &at in &cavity {
            mesh.remove(at);
        }

        // fill it with triangles from the point to each edge of the boundary,
        // and mend the links on both sides
        made.clear();
        for &(from, to, across) in &border {
            let at = mesh.add([from, to, index], [across, NOWHERE, NOWHERE]);
            if across != NOWHERE {
                // the link the other way round was left dangling when the
                // cavity was taken apart, so it is found by its corners
                for side in 0..3 {
                    let (a, b) = (
                        mesh.corners[across][side],
                        mesh.corners[across][(side + 1) % 3],
                    );
                    if a == to && b == from {
                        mesh.neighbours[across][side] = at;
                    }
                }
            }
            made.push(at);
        }
        // the new triangles all meet at the point, so each is joined to the one
        // that carries on from where its edge ends
        for &at in &made {
            let [from, to, _] = mesh.corners[at];
            for &other in &made {
                if other == at {
                    continue;
                }
                let [a, b, _] = mesh.corners[other];
                if a == to {
                    mesh.neighbours[at][1] = other;
                }
                if b == from {
                    mesh.neighbours[at][2] = other;
                }
            }
        }
        if let Some(&at) = made.first() {
            hint = at;
        }
    }
    // the triangles that lean on a corner of the outer triangle are not of the
    // points at all, and every triangle is handed back wound anticlockwise
    let mut triangles = Vec::new();
    for at in 0..mesh.corners.len() {
        if !mesh.alive[at] {
            continue;
        }
        let mut corners = mesh.corners[at];
        if corners.iter().any(|corner| outer.contains(corner)) {
            continue;
        }
        let places = corners.map(|corner| points[corner]);
        if twice_signed_area(places[0], places[1], places[2]) < 0.0 {
            corners.swap(1, 2);
        }
        triangles.push(corners);
    }
    triangles
}

/// The alpha shape of a set of points, as the rings of its boundary.
///
/// `alpha` is a length: the radius of the largest circle that may sit inside
/// the shape without opening a hole in it. A large enough alpha gives the
/// convex hull, a small enough one gives nothing at all, and in between the
/// shape follows the points into their bays.
///
/// Each ring is a list of indices into `points`, wound anticlockwise, and the
/// first point is not repeated at the end. A shape that falls into pieces comes
/// back as several rings, which is not a fault: a set of points in two clusters
/// with a gap wider than alpha between them is two shapes.
#[must_use]
pub fn alpha_shape(points: &[Point2D], alpha: f64) -> Vec<Vec<usize>> {
    let kept = triangulate(points)
        .into_iter()
        .filter(|triangle| {
            let corners = triangle.map(|corner| points[corner]);
            circumcircle(corners[0], corners[1], corners[2])
                .is_some_and(|(_, radius_squared)| radius_squared <= alpha * alpha)
        })
        .collect::<Vec<_>>();

    // An edge of the boundary is one that a single kept triangle owns: the ones
    // inside the shape are owned by two, which meet along them. The triangles
    // are wound the same way round, so an inner edge is met once in each
    // direction and a boundary edge only in one, which is the way that leaves
    // the shape on the left.
    let mut edges: FxHashSet<(usize, usize)> = FxHashSet::default();
    for triangle in &kept {
        for side in 0..3 {
            edges.insert((triangle[side], triangle[(side + 1) % 3]));
        }
    }
    let boundary = edges
        .iter()
        .filter(|(from, to)| !edges.contains(&(*to, *from)))
        .copied()
        .collect::<Vec<_>>();

    // and the rings are what the boundary edges chain into
    let mut onwards: FxHashMap<usize, Vec<usize>> = FxHashMap::default();
    for &(from, to) in &boundary {
        onwards.entry(from).or_default().push(to);
    }

    let mut rings = Vec::new();
    let mut walked: FxHashSet<(usize, usize)> = FxHashSet::default();
    for &(start, _) in &boundary {
        let mut ring = Vec::new();
        let mut at = start;
        while let Some(next) = onwards
            .get(&at)
            .and_then(|ends| ends.iter().find(|&&end| !walked.contains(&(at, end))))
            .copied()
        {
            walked.insert((at, next));
            ring.push(at);
            at = next;
            if at == start {
                break;
            }
        }
        if ring.len() >= 3 {
            rings.push(ring);
        }
    }
    rings
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{RngExt, SeedableRng, prelude::StdRng};

    fn point(x: f64, y: f64) -> Point2D {
        Point2D { x, y }
    }

    /// What the triangulation promises: no point sits inside the circumcircle
    /// of any triangle. This is the whole of the Delaunay property, and it is
    /// what everything else here stands on.
    fn holds_the_delaunay_property(points: &[Point2D], triangles: &[Triangle]) -> bool {
        for triangle in triangles {
            let corners = triangle.map(|corner| points[corner]);
            let Some((centre, radius_squared)) = circumcircle(corners[0], corners[1], corners[2])
            else {
                return false;
            };
            for (index, point) in points.iter().enumerate() {
                if triangle.contains(&index) {
                    continue;
                }
                let (dx, dy) = (point.x - centre.x, point.y - centre.y);
                // a hair of room, as a point on the circle is not inside it and
                // arithmetic on floats does not always agree
                if dx * dx + dy * dy < radius_squared * (1.0 - 1e-9) {
                    return false;
                }
            }
        }
        true
    }

    /// Twice the signed area of a ring, positive when it is wound
    /// anticlockwise. This is the shoelace, and it is what says which way
    /// round a ring goes without asking anything that built it.
    fn twice_signed_area_of(points: &[Point2D], ring: &[usize]) -> f64 {
        (0..ring.len())
            .map(|at| {
                let here = points[ring[at]];
                let next = points[ring[(at + 1) % ring.len()]];
                here.x * next.y - next.x * here.y
            })
            .sum()
    }

    /// Three points on a line have no circle through them, which is the case
    /// the triangulation leans on to leave a flat triangle alone.
    #[test]
    fn three_points_on_a_line_have_no_circle() {
        assert!(circumcircle(point(0., 0.), point(1., 1.), point(2., 2.)).is_none());
        assert!(circumcircle(point(0., 0.), point(0., 5.), point(0., 9.)).is_none());
        // and one that sits on top of another is the same case
        assert!(circumcircle(point(0., 0.), point(0., 0.), point(1., 0.)).is_none());
        // where a circle does exist it passes through all three
        let (centre, radius_squared) =
            circumcircle(point(0., 0.), point(4., 0.), point(0., 4.)).expect("a circle");
        for corner in [point(0., 0.), point(4., 0.), point(0., 4.)] {
            let (dx, dy) = (corner.x - centre.x, corner.y - centre.y);
            assert!((dx * dx + dy * dy - radius_squared).abs() < 1e-9);
        }
    }

    /// A place that comes round a second time with another point in between.
    ///
    /// The two sit in one cell of the curve that orders the insertion, and
    /// only the last point of a cell used to be remembered, so the repeat was
    /// not recognised. It was then inserted where a point already sat, which
    /// leaves a triangle of no area and a mesh that drops a corner it was
    /// given.
    #[test]
    fn a_place_is_recognised_when_it_comes_round_again() {
        let points = [
            point(0., 0.),
            // near enough to share a cell of the curve, far enough to be a
            // point of its own
            point(0.001, 0.),
            point(0., 0.),
            point(100., 0.),
            point(100., 100.),
            point(0., 100.),
        ];
        let triangles = triangulate(&points);

        for triangle in &triangles {
            let corners = triangle.map(|corner| points[corner]);
            assert!(
                twice_signed_area(corners[0], corners[1], corners[2]).abs() > 0.,
                "{triangle:?} has no area"
            );
        }
        assert!(holds_the_delaunay_property(&points, &triangles));
        // and no corner of the square went missing on the way
        for corner in [3, 4, 5] {
            assert!(
                triangles.iter().any(|triangle| triangle.contains(&corner)),
                "point {corner} is in no triangle: {triangles:?}"
            );
        }
    }

    /// What a hole is: a ring on the outside and a ring around the gap, and
    /// the two wound opposite ways so that a renderer knows which is which.
    #[test]
    fn a_shape_with_a_hole_comes_back_as_two_rings() {
        // a filled square with the middle left out
        let mut points = Vec::new();
        for x in 0..11 {
            for y in 0..11 {
                let inside = (3..=7).contains(&x) && (3..=7).contains(&y);
                if !inside {
                    points.push(point(f64::from(x), f64::from(y)));
                }
            }
        }

        let rings = alpha_shape(&points, 1.5);
        assert_eq!(rings.len(), 2, "an outside and a hole: {rings:?}");

        let mut areas = rings
            .iter()
            .map(|ring| twice_signed_area_of(&points, ring))
            .collect::<Vec<_>>();
        areas.sort_by(f64::total_cmp);
        // the hole is wound the other way round from the outside, so one area
        // is negative and the other positive
        assert!(areas[0] < 0., "no ring runs the other way: {areas:?}");
        assert!(areas[1] > 0., "no ring runs the usual way: {areas:?}");
        // and the outside is the larger of the two
        assert!(areas[1] > areas[0].abs());
    }

    /// The rings are documented as anticlockwise and as not repeating their
    /// first point at the end. Both are promises a caller draws on.
    #[test]
    fn a_ring_is_wound_anticlockwise_and_closes_itself() {
        let mut rng = StdRng::seed_from_u64(0x_21A6);
        for round in 0..10 {
            let points = (0..(12 + round * 5))
                .map(|_| point(rng.random_range(0.0..100.0), rng.random_range(0.0..100.0)))
                .collect::<Vec<_>>();

            for ring in alpha_shape(&points, 1e6) {
                assert!(
                    twice_signed_area_of(&points, &ring) > 0.,
                    "round {round}: {ring:?} is wound the other way"
                );
                assert_ne!(
                    ring.first(),
                    ring.last(),
                    "round {round}: {ring:?} repeats its first point"
                );
                let mut seen = ring.clone();
                seen.sort_unstable();
                seen.dedup();
                assert_eq!(
                    seen.len(),
                    ring.len(),
                    "round {round}: {ring:?} repeats a point"
                );
                for &at in &ring {
                    assert!(at < points.len(), "round {round}: {at} is not a point");
                }
            }
        }
    }

    /// Nothing to work with is not a fault, and neither is a disc of no size.
    #[test]
    fn a_shape_of_too_little_is_empty_rather_than_wrong() {
        assert!(alpha_shape(&[], 1.0).is_empty());
        assert!(alpha_shape(&[point(0., 0.)], 1.0).is_empty());
        assert!(alpha_shape(&[point(0., 0.), point(1., 0.)], 1.0).is_empty());
        // three points do make a shape, and a line of them does not
        assert_eq!(
            alpha_shape(&[point(0., 0.), point(1., 0.), point(0., 1.)], 10.0).len(),
            1
        );
        assert!(alpha_shape(&[point(0., 0.), point(1., 1.), point(2., 2.)], 10.0).is_empty());

        let square = [point(0., 0.), point(4., 0.), point(4., 4.), point(0., 4.)];
        assert!(alpha_shape(&square, 0.).is_empty());
        assert!(alpha_shape(&square, -1.).is_empty());
    }

    /// The curve is what puts the points in the order they are inserted in,
    /// and it has to tell two cells apart or two points would be taken for
    /// one. Every cell of a small square gets its own place, once.
    #[test]
    fn the_curve_gives_every_cell_a_place_of_its_own() {
        let side = 32;
        let mut places = (0..side)
            .flat_map(|x| (0..side).map(move |y| along_the_curve(x, y)))
            .collect::<Vec<_>>();
        places.sort_unstable();
        let count = places.len();
        places.dedup();
        assert_eq!(places.len(), count, "two cells share a place on the curve");

        // and it keeps neighbours together: a step along the curve is a step
        // in the plane, which is what makes the walk to the next triangle short
        for x in 0..side {
            for y in 0..side {
                for (dx, dy) in [(1_u32, 0_u32), (0, 1)] {
                    if x + dx >= side || y + dy >= side {
                        continue;
                    }
                    let here = along_the_curve(x, y);
                    let next = along_the_curve(x + dx, y + dy);
                    assert_ne!(here, next);
                }
            }
        }
    }

    #[test]
    fn too_few_points_hold_no_triangle() {
        assert!(triangulate(&[]).is_empty());
        assert!(triangulate(&[point(0., 0.)]).is_empty());
        assert!(triangulate(&[point(0., 0.), point(1., 1.)]).is_empty());
    }

    #[test]
    fn points_on_a_line_hold_no_triangle() {
        let line = [point(0., 0.), point(1., 1.), point(2., 2.), point(3., 3.)];
        assert!(triangulate(&line).is_empty());
    }

    #[test]
    fn a_square_is_two_triangles() {
        let square = [point(0., 0.), point(4., 0.), point(4., 4.), point(0., 4.)];
        let triangles = triangulate(&square);
        assert_eq!(triangles.len(), 2);
        assert!(holds_the_delaunay_property(&square, &triangles));
    }

    #[test]
    fn every_triangle_is_wound_anticlockwise() {
        let mut rng = StdRng::seed_from_u64(0x_A1FA);
        let points = (0..40)
            .map(|_| point(rng.random_range(0.0..100.0), rng.random_range(0.0..100.0)))
            .collect::<Vec<_>>();

        for triangle in triangulate(&points) {
            let corners = triangle.map(|corner| points[corner]);
            assert!(
                twice_signed_area(corners[0], corners[1], corners[2]) > 0.0,
                "{triangle:?} is wound the other way"
            );
        }
    }

    #[test]
    fn a_triangulation_of_points_without_a_pattern_is_delaunay() {
        let mut rng = StdRng::seed_from_u64(0x_D3A0);
        for round in 0..20 {
            let points = (0..(8 + round * 3))
                .map(|_| point(rng.random_range(0.0..1000.0), rng.random_range(0.0..1000.0)))
                .collect::<Vec<_>>();
            let triangles = triangulate(&points);

            assert!(!triangles.is_empty(), "round {round} came out empty");
            assert!(
                holds_the_delaunay_property(&points, &triangles),
                "round {round} has a point inside a circumcircle"
            );
        }
    }

    /// A triangulation of n points with h of them on the hull holds 2n - h - 2
    /// triangles, whatever the points are. That is a count the construction
    /// cannot fudge.
    #[test]
    fn a_triangulation_holds_as_many_triangles_as_it_should() {
        use crate::convex_hull::monotone_chain;
        use crate::geometry::FPCoordinate;

        let mut rng = StdRng::seed_from_u64(0x_C0117);
        for round in 0..10 {
            let count = 10 + round * 5;
            // whole numbers, so that the hull is not a matter of rounding
            let raw = (0..count)
                .map(|_| {
                    (
                        rng.random_range(0..1_000_000_i32),
                        rng.random_range(0..1_000_000_i32),
                    )
                })
                .collect::<Vec<_>>();
            let points = raw
                .iter()
                .map(|&(x, y)| point(f64::from(x), f64::from(y)))
                .collect::<Vec<_>>();
            let as_coordinates = raw
                .iter()
                .map(|&(x, y)| FPCoordinate::new(y, x))
                .collect::<Vec<_>>();

            let on_hull = monotone_chain(&as_coordinates).len();
            assert_eq!(
                triangulate(&points).len(),
                2 * count - on_hull - 2,
                "round {round}: {count} points, {on_hull} of them on the hull"
            );
        }
    }

    /// A grid is the case that random points never cover: every four corners
    /// of a square lie on one circle, so the question of whether a point is
    /// inside a circumcircle keeps landing exactly on the line. Answering it
    /// one way for a triangle and the other way for the one that shares an
    /// edge with it leaves triangles lying over each other, which is what this
    /// counts. A grid of five by five holds sixteen unit squares of two
    /// triangles each.
    #[test]
    fn a_grid_of_cocircular_points_is_triangulated_once_over() {
        let side = 5;
        let points = (0..side)
            .flat_map(|x| (0..side).map(move |y| point(f64::from(x), f64::from(y))))
            .collect::<Vec<_>>();

        let triangles = triangulate(&points);
        assert_eq!(triangles.len(), 2 * (side as usize - 1).pow(2));
        assert!(holds_the_delaunay_property(&points, &triangles));

        // and the boundary of the lot is the edge of the grid, once round
        let rings = alpha_shape(&points, 1e6);
        assert_eq!(rings.len(), 1);
        assert_eq!(rings[0].len(), 4 * (side as usize - 1));
    }

    #[test]
    fn a_wide_enough_disc_gives_the_convex_hull() {
        let mut rng = StdRng::seed_from_u64(0x_F0FA);
        for round in 0..10 {
            let points = (0..(10 + round * 4))
                .map(|_| point(rng.random_range(0.0..100.0), rng.random_range(0.0..100.0)))
                .collect::<Vec<_>>();

            // a disc wider than everything rolls over every bay
            let rings = alpha_shape(&points, 1e6);
            assert_eq!(rings.len(), 1, "round {round} came apart");

            // and what it leaves is the convex hull, corner for corner
            let mut corners = rings[0].clone();
            corners.sort_unstable();
            let mut hull_corners = hull_by_hand(&points);
            hull_corners.sort_unstable();
            assert_eq!(corners, hull_corners, "round {round}");
        }
    }

    /// The convex hull as the indices of the points on it, worked out by the
    /// gift wrapping that shares nothing with the triangulation.
    fn hull_by_hand(points: &[Point2D]) -> Vec<usize> {
        let leftmost = (0..points.len())
            .min_by(|&a, &b| points[a].x.total_cmp(&points[b].x))
            .expect("no points");
        let mut hull = Vec::new();
        let mut at = leftmost;
        loop {
            hull.push(at);
            let mut candidate = (at + 1) % points.len();
            for other in 0..points.len() {
                let turn = twice_signed_area(points[at], points[candidate], points[other]);
                if turn < 0.0 {
                    candidate = other;
                }
            }
            at = candidate;
            if at == leftmost {
                return hull;
            }
        }
    }

    #[test]
    fn a_narrow_disc_leaves_nothing() {
        let square = [point(0., 0.), point(4., 0.), point(4., 4.), point(0., 4.)];
        // the circumcircle of either triangle has a radius of 2 sqrt 2
        assert!(alpha_shape(&square, 2.0).is_empty());
        assert_eq!(alpha_shape(&square, 3.0).len(), 1);
    }

    /// What a concave hull is for: a shape with a bay in it. A disc small
    /// enough to fit into the bay eats it out, and the shape then holds fewer
    /// points than the convex hull would have covered.
    #[test]
    fn a_disc_that_fits_into_a_bay_eats_it_out() {
        // A horseshoe: points along a c shape, with the mouth of it open. They
        // are set close enough together that a disc of twelve cannot fall
        // between two of them, or the shape would come apart along its own
        // sampling rather than at the mouth.
        let mut points = Vec::new();
        for step in 0..=60 {
            let angle =
                std::f64::consts::PI * 0.25 + std::f64::consts::PI * 1.5 * f64::from(step) / 60.0;
            points.push(point(50.0 + 40.0 * angle.cos(), 50.0 + 40.0 * angle.sin()));
            points.push(point(50.0 + 30.0 * angle.cos(), 50.0 + 30.0 * angle.sin()));
        }

        let wide = alpha_shape(&points, 1e6);
        let narrow = alpha_shape(&points, 12.0);
        assert_eq!(wide.len(), 1);
        assert_eq!(narrow.len(), 1, "the horseshoe came apart");
        assert!(
            narrow[0].len() > wide[0].len(),
            "the narrow disc followed {} corners, the wide one {}",
            narrow[0].len(),
            wide[0].len()
        );
    }

    /// Two clusters with a gap between them are two shapes once the disc is
    /// narrower than the gap, which is a thing a convex hull cannot say.
    #[test]
    fn a_gap_wider_than_the_disc_makes_two_shapes() {
        let mut points = Vec::new();
        for x in 0..5 {
            for y in 0..5 {
                points.push(point(f64::from(x), f64::from(y)));
                points.push(point(f64::from(x) + 40.0, f64::from(y)));
            }
        }

        assert_eq!(alpha_shape(&points, 1e6).len(), 1, "one hull over both");
        assert_eq!(alpha_shape(&points, 2.0).len(), 2, "two clusters");
    }

    #[test]
    fn points_in_the_same_place_are_taken_for_one() {
        let doubled = [
            point(0., 0.),
            point(0., 0.),
            point(4., 0.),
            point(4., 4.),
            point(0., 4.),
        ];
        let triangles = triangulate(&doubled);
        assert_eq!(triangles.len(), 2, "{triangles:?}");
        assert!(holds_the_delaunay_property(&doubled, &triangles));
    }
}
