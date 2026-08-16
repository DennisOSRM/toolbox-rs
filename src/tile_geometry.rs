//! The geometry of a vector tile: where a coordinate lands on one, and what
//! of a shape is still on it.
//!
//! A tile is a square of `TILE_EXTENT` units with a margin around it. A reader
//! honours a buffer of its own and drops or mangles whatever reaches past it,
//! so anything handed over has to be cut down to that square first. An arc of a
//! road network is far longer than a tile at a low zoom level, and the hull of
//! a cell can cover a country, so the cutting is not a rare case.
//!
//! Nothing here knows about a server, a partition or a graph. It is the tile
//! and what is being drawn on it.

use crate::{
    bounding_box::BoundingBox,
    geometry::FPCoordinate,
    vector_tile::{TILE_SIZE, degree_to_pixel_lat, degree_to_pixel_lon, pixel_to_degree},
    wgs84::{FloatLatitude, FloatLongitude},
};

/// The extent a tile draws its geometry on. It matches the grid the pixel
/// conversions of the library work in, so a global pixel coordinate minus the
/// origin of the tile is already the number a tile carries.
pub const TILE_EXTENT: u32 = TILE_SIZE as u32;

/// How far outside of a tile geometry is still drawn, in tile units. Renderers
/// need a margin to draw the width of a line whose center lies outside.
pub const TILE_MARGIN: f64 = 128.;

/// One edge of the tile as the clip sees it: whether a point is on the inside
/// of it, and where a segment crosses it.
type TileEdge = (
    fn(&(i32, i32)) -> bool,
    fn((i32, i32), (i32, i32)) -> (i32, i32),
);

/// Converts a coordinate into the grid that the tile at the given position
/// draws on. Coordinates outside of the tile keep their offset instead of being
/// clamped onto its border, which is what lets a line that crosses the border
/// leave it at the right angle.
#[must_use]
pub fn to_tile_coordinate(
    coordinate: FPCoordinate,
    zoom: u32,
    tile_x: u32,
    tile_y: u32,
) -> (i32, i32) {
    let (lon, lat) = coordinate.to_lon_lat_pair();
    let x =
        degree_to_pixel_lon(FloatLongitude(lon), zoom) - f64::from(tile_x) * f64::from(TILE_EXTENT);
    let y =
        degree_to_pixel_lat(FloatLatitude(lat), zoom) - f64::from(tile_y) * f64::from(TILE_EXTENT);

    // the grid is far smaller than the range of an i32, but a coordinate of a
    // broken input should not wrap around into a plausible looking one
    (
        x.round().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32,
        y.round().clamp(f64::from(i32::MIN), f64::from(i32::MAX)) as i32,
    )
}

/// Whether a position lies on the tile, margin included. A point either sits on
/// the tile or it does not, so unlike a segment it cannot span it.
#[must_use]
pub fn is_within_tile(position: (i32, i32)) -> bool {
    let low = -TILE_MARGIN;
    let high = f64::from(TILE_EXTENT) + TILE_MARGIN;
    let within = |value: i32| f64::from(value) >= low && f64::from(value) <= high;
    within(position.0) && within(position.1)
}

/// The ground a tile covers, margin included, as a box in coordinates. This is
/// what the tree is asked about.
#[must_use]
pub fn tile_bounds(zoom: u32, x: u32, y: u32) -> BoundingBox {
    tile_bounds_with_margin(zoom, x, y, TILE_MARGIN)
}

/// The same, with the margin given rather than taken from the constant. With
/// no margin this is the plain ground of the tile, which is what the library
/// works out by a different road in `vector_tile::get_tile_bounds`.
#[must_use]
pub fn tile_bounds_with_margin(zoom: u32, x: u32, y: u32, margin: f64) -> BoundingBox {
    let shift = (1 << zoom) * TILE_SIZE;
    let margin = margin * f64::from(TILE_SIZE as u32) / f64::from(TILE_EXTENT);
    let corner = |across: f64, down: f64| {
        let (mut lon, mut lat) = (
            f64::from(x) * TILE_SIZE as f64 + across,
            f64::from(y) * TILE_SIZE as f64 + down,
        );
        pixel_to_degree(shift, &mut lon, &mut lat);
        FPCoordinate::new_from_lat_lon(lat, lon)
    };
    let side = TILE_SIZE as f64;
    BoundingBox::from_coordinates(&[
        corner(-margin, -margin),
        corner(side + margin, side + margin),
    ])
}

/// Whether any part of a ring lies near enough to the tile to be worth handing
/// over. A ring that surrounds the tile without a corner inside it counts, as
/// what it covers is the whole tile.
#[must_use]
pub fn ring_reaches_tile(ring: &[(i32, i32)]) -> bool {
    let margin = TILE_MARGIN as i32;
    let (low, high) = (-margin, TILE_EXTENT as i32 + margin);
    let (mut left, mut right, mut top, mut bottom) = (i32::MAX, i32::MIN, i32::MAX, i32::MIN);
    for &(x, y) in ring {
        left = left.min(x);
        right = right.max(x);
        top = top.min(y);
        bottom = bottom.max(y);
    }
    left <= high && right >= low && top <= high && bottom >= low
}

/// Whether the box a hull covers reaches the tile at all, so that the hulls of
/// a continent can be passed over without walking each of them. The y of a tile
/// grows southwards, so the corners swap on the way in.
#[must_use]
pub fn box_reaches_tile(corners: &[FPCoordinate; 2], zoom: u32, x: u32, y: u32) -> bool {
    let one = to_tile_coordinate(corners[0], zoom, x, y);
    let other = to_tile_coordinate(corners[1], zoom, x, y);
    let (low_x, high_x) = (one.0.min(other.0), one.0.max(other.0));
    let (low_y, high_y) = (one.1.min(other.1), one.1.max(other.1));
    let margin = TILE_MARGIN as i32;
    low_x <= TILE_EXTENT as i32 + margin
        && high_x >= -margin
        && low_y <= TILE_EXTENT as i32 + margin
        && high_y >= -margin
}

/// Cuts a ring down to the part of it that lies on the tile, by clipping it
/// against one edge of the tile after another.
///
/// This is the Sutherland and Hodgman clip. It holds for a convex ring, which
/// a hull is, and the tile it is cut against is convex too, so what comes back
/// is one ring rather than several. A ring that misses the tile altogether
/// comes back empty.
#[must_use]
pub fn clip_ring_to_tile(ring: &[(i32, i32)]) -> Vec<(i32, i32)> {
    // which side of an edge a point is on, and where a segment crosses it
    let edges: [TileEdge; 4] = [
        (
            |p| p.0 >= -(TILE_MARGIN as i32),
            |a, b| cross_x(a, b, -(TILE_MARGIN as i32)),
        ),
        (
            |p| p.0 <= TILE_EXTENT as i32 + TILE_MARGIN as i32,
            |a, b| cross_x(a, b, TILE_EXTENT as i32 + TILE_MARGIN as i32),
        ),
        (
            |p| p.1 >= -(TILE_MARGIN as i32),
            |a, b| cross_y(a, b, -(TILE_MARGIN as i32)),
        ),
        (
            |p| p.1 <= TILE_EXTENT as i32 + TILE_MARGIN as i32,
            |a, b| cross_y(a, b, TILE_EXTENT as i32 + TILE_MARGIN as i32),
        ),
    ];

    let mut ring = ring.to_vec();
    for (inside, cross) in edges {
        if ring.is_empty() {
            break;
        }
        let mut kept = Vec::with_capacity(ring.len() + 4);
        for (index, &point) in ring.iter().enumerate() {
            let previous = ring[(index + ring.len() - 1) % ring.len()];
            let (was_in, is_in) = (inside(&previous), inside(&point));
            if is_in != was_in {
                kept.push(cross(previous, point));
            }
            if is_in {
                kept.push(point);
            }
        }
        ring = kept;
    }
    ring
}

/// Where a segment crosses a vertical line.
fn cross_x(a: (i32, i32), b: (i32, i32), x: i32) -> (i32, i32) {
    let span = i64::from(b.0 - a.0);
    if span == 0 {
        return (x, a.1);
    }
    let along = i64::from(x - a.0);
    (x, a.1 + (i64::from(b.1 - a.1) * along / span) as i32)
}

/// Where a segment crosses a horizontal line.
fn cross_y(a: (i32, i32), b: (i32, i32), y: i32) -> (i32, i32) {
    let span = i64::from(b.1 - a.1);
    if span == 0 {
        return (a.0, y);
    }
    let along = i64::from(y - a.1);
    (a.0 + (i64::from(b.0 - a.0) * along / span) as i32, y)
}

/// Cuts a segment down to the part of it that lies on the tile, margin
/// included, and hands back `None` for one that misses the tile altogether.
///
/// An arc of a road network is far longer than a tile at a low zoom level, and
/// an endpoint of it can land thousands of units outside the grid. A reader
/// only honours a buffer of its own around the tile and drops or mangles what
/// reaches past it, so the part that hangs over is cut off here rather than
/// handed over.
///
/// The segment is clipped against the four edges by the Liang-Barsky method:
/// the segment is walked as `source + t * (target - source)` for `t` in
/// `[0, 1]`, and each edge either moves the near end forward or the far end
/// back until the interval either is the part that lies on the tile or has
/// closed, in which case the segment never touches it.
#[must_use]
pub fn clip_to_tile(source: (i32, i32), target: (i32, i32)) -> Option<((i32, i32), (i32, i32))> {
    let low = -TILE_MARGIN;
    let high = f64::from(TILE_EXTENT) + TILE_MARGIN;

    let (x, y) = (f64::from(source.0), f64::from(source.1));
    let (dx, dy) = (f64::from(target.0) - x, f64::from(target.1) - y);

    let mut near = 0_f64;
    let mut far = 1_f64;
    // one pair per edge: how fast the segment approaches it, and how far the
    // near end still is from it
    for (speed, distance) in [
        (-dx, x - low),
        (dx, high - x),
        (-dy, y - low),
        (dy, high - y),
    ] {
        if speed == 0. {
            // parallel to this edge, so it either lies on the tile or misses it
            // no matter how far it is walked
            if distance < 0. {
                return None;
            }
            continue;
        }
        let crossing = distance / speed;
        if speed < 0. {
            if crossing > far {
                return None;
            }
            near = near.max(crossing);
        } else {
            if crossing < near {
                return None;
            }
            far = far.min(crossing);
        }
    }

    let at = |t: f64| ((x + t * dx).round() as i32, (y + t * dy).round() as i32);
    let (from, to) = (at(near), at(far));

    // a segment whose ends round onto the same position of the grid draws
    // nothing, which is what thins out a tile of a low zoom level
    (from != to).then_some((from, to))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vector_tile::{coordinate_to_tile_number, get_tile_bounds};
    use crate::wgs84::FloatCoordinate;

    const ZOOM: u32 = 14;
    const LAT: f64 = 50.20731;
    const LON: f64 = 8.57747;

    fn tile_of_probe() -> (u32, u32) {
        coordinate_to_tile_number(
            FloatCoordinate {
                lat: FloatLatitude(LAT),
                lon: FloatLongitude(LON),
            },
            ZOOM,
        )
    }

    #[test]
    fn the_corner_of_a_tile_sits_at_its_origin() {
        let (x, y) = tile_of_probe();
        let bounds = get_tile_bounds(ZOOM, x, y);
        let corner = FPCoordinate::new_from_lat_lon(bounds.min_lat.0, bounds.min_lon.0);

        let (tile_x, tile_y) = to_tile_coordinate(corner, ZOOM, x, y);
        // the north west corner of a tile is the origin of its grid
        assert!(tile_x.abs() <= 1, "x of the corner is {tile_x}");
        assert!(tile_y.abs() <= 1, "y of the corner is {tile_y}");
    }

    #[test]
    fn a_coordinate_of_the_tile_lands_inside_its_extent() {
        let (x, y) = tile_of_probe();
        let (tile_x, tile_y) =
            to_tile_coordinate(FPCoordinate::new_from_lat_lon(LAT, LON), ZOOM, x, y);

        assert!((0..TILE_EXTENT as i32).contains(&tile_x), "x is {tile_x}");
        assert!((0..TILE_EXTENT as i32).contains(&tile_y), "y is {tile_y}");
    }

    #[test]
    fn a_segment_that_draws_nothing_is_dropped() {
        assert!(clip_to_tile((10, 10), (10, 10)).is_none());
        assert!(clip_to_tile((10, 10), (11, 10)).is_some());
    }

    #[test]
    fn segments_off_one_side_are_dropped() {
        let outside = TILE_EXTENT as i32 * 4;
        assert!(clip_to_tile((-outside, 10), (-outside - 5, 10)).is_none());
        assert!(clip_to_tile((outside, 10), (outside + 5, 10)).is_none());
        assert!(clip_to_tile((10, -outside), (10, -outside - 5)).is_none());
        assert!(clip_to_tile((10, outside), (10, outside + 5)).is_none());
    }

    /// The reason the clipping exists: an arc far longer than the tile has to
    /// arrive cut down to the part of it that lies on the tile, or a reader
    /// drops it for reaching past the buffer it keeps around the tile.
    #[test]
    fn a_segment_across_the_tile_is_cut_to_it() {
        let outside = TILE_EXTENT as i32 * 4;
        let extent = TILE_EXTENT as i32;
        let margin = TILE_MARGIN as i32;

        let (from, to) = clip_to_tile((-outside, 2048), (outside, 2048)).expect("crosses the tile");
        assert_eq!(from, (-margin, 2048));
        assert_eq!(to, (extent + margin, 2048));

        let (from, to) = clip_to_tile((2048, -outside), (2048, outside)).expect("crosses the tile");
        assert_eq!(from, (2048, -margin));
        assert_eq!(to, (2048, extent + margin));
    }

    #[test]
    fn only_the_end_that_hangs_over_is_cut() {
        let extent = TILE_EXTENT as i32;
        let margin = TILE_MARGIN as i32;
        // starts on the tile and runs far off to the east
        let (from, to) =
            clip_to_tile((1000, 1000), (extent * 5, 1000)).expect("starts on the tile");
        assert_eq!(from, (1000, 1000), "the end on the tile is left alone");
        assert_eq!(to, (extent + margin, 1000), "the end off it is cut back");
    }

    #[test]
    fn a_segment_within_the_tile_is_left_alone() {
        let segment = ((100, 200), (3000, 3500));
        assert_eq!(clip_to_tile(segment.0, segment.1), Some(segment));
    }

    #[test]
    fn a_diagonal_keeps_its_direction_when_cut() {
        let extent = TILE_EXTENT as i32;
        let (from, to) = clip_to_tile((-extent, -extent), (2 * extent, 2 * extent))
            .expect("crosses the tile diagonally");
        // the segment runs at 45 degrees, so the cut ends do too
        assert_eq!(from.0, from.1);
        assert_eq!(to.0, to.1);
        assert!(from.0 < to.0);
    }

    /// A clipped segment has to stay within the grid a reader accepts.
    #[test]
    fn what_is_handed_over_stays_within_the_margin() {
        let outside = TILE_EXTENT as i32 * 9;
        let bound = TILE_EXTENT as i32 + TILE_MARGIN as i32;
        for segment in [
            ((-outside, -outside), (outside, outside)),
            ((-outside, 2048), (outside, 2048)),
            ((2048, outside), (2048, -outside)),
            ((-outside, 4095), (outside, 0)),
        ] {
            let (from, to) = clip_to_tile(segment.0, segment.1).expect("crosses the tile");
            for point in [from, to] {
                assert!(point.0 >= -bound && point.0 <= bound, "x of {point:?}");
                assert!(point.1 >= -bound && point.1 <= bound, "y of {point:?}");
            }
        }
    }

    /// A ring that lies inside the tile is handed back as it was, and one that
    /// misses it altogether comes back empty rather than as something to draw.
    #[test]
    fn a_ring_is_cut_down_to_the_tile() {
        let inside = [(100, 100), (300, 100), (300, 300), (100, 300)];
        assert_eq!(clip_ring_to_tile(&inside), inside.to_vec());

        let far_off = [(20_000, 20_000), (21_000, 20_000), (21_000, 21_000)];
        assert!(clip_ring_to_tile(&far_off).is_empty());
    }

    /// A ring larger than the tile comes back as the tile itself, margin and
    /// all, rather than as coordinates a reader has to make sense of.
    #[test]
    fn a_ring_around_the_tile_is_cut_to_it() {
        let around = [
            (-9000, -9000),
            (9000 + TILE_EXTENT as i32, -9000),
            (9000 + TILE_EXTENT as i32, 9000 + TILE_EXTENT as i32),
            (-9000, 9000 + TILE_EXTENT as i32),
        ];
        let clipped = clip_ring_to_tile(&around);
        assert_eq!(clipped.len(), 4, "{clipped:?}");
        let margin = TILE_MARGIN as i32;
        for (x, y) in clipped {
            assert!((-margin..=TILE_EXTENT as i32 + margin).contains(&x), "{x}");
            assert!((-margin..=TILE_EXTENT as i32 + margin).contains(&y), "{y}");
        }
    }

    /// The box a tile covers with no margin is the box the library works out
    /// for the same tile by a different road. They are two ways of asking one
    /// question, and this is what says they still agree.
    #[test]
    fn the_bounds_of_a_tile_agree_with_the_library() {
        for (zoom, x, y) in [(14, 8802, 5373), (7, 66, 41), (0, 0, 0), (10, 0, 0)] {
            let theirs = get_tile_bounds(zoom, x, y);
            let mine = tile_bounds_with_margin(zoom, x, y, 0.);
            let corner = |lat: f64, lon: f64| FPCoordinate::new_from_lat_lon(lat, lon);
            let expected = BoundingBox::from_coordinates(&[
                corner(theirs.min_lat.0, theirs.min_lon.0),
                corner(theirs.max_lat.0, theirs.max_lon.0),
            ]);
            assert_eq!(mine, expected, "tile {zoom}/{x}/{y}");
        }
    }

    /// And the margin only ever grows the box, never moves it.
    #[test]
    fn the_margin_grows_the_box_around_the_tile() {
        let plain = tile_bounds_with_margin(14, 8802, 5373, 0.);
        let padded = tile_bounds(14, 8802, 5373);
        let mut grown = padded;
        grown.extend_with(&plain);
        assert_eq!(grown, padded, "the margin does not cover the tile itself");
        assert_ne!(plain, padded, "the margin changes nothing");
    }
}
