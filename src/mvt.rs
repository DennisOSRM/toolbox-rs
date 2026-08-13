//! Geometry encoding for Mapbox Vector Tiles.
//!
//! The geometry of a vector tile feature is not stored as coordinates, but as a
//! sequence of drawing commands, each followed by the parameters it takes. This
//! module builds that sequence.
//!
//! # Commands
//!
//! A command integer packs the id of the command into its lowest three bits and
//! the number of times it repeats into the remaining ones:
//!
//! ```text
//! command_integer = (id & 0x7) | (count << 3)
//! ```
//!
//! | command    | id | parameters | meaning                                    |
//! |------------|----|------------|--------------------------------------------|
//! | `MoveTo`   | 1  | 2 per count| start a new part at the given position     |
//! | `LineTo`   | 2  | 2 per count| draw a line to the given position          |
//! | `ClosePath`| 7  | 0          | close the current ring, cursor stays put    |
//!
//! # Parameters
//!
//! Positions are relative to the position the previous command left the cursor
//! at, and the resulting deltas are zigzag encoded so that small negative
//! numbers stay small. The cursor starts at the origin of the tile.
//!
//! # Examples
//!
//! The line string `(2,2), (2,10), (10,10)` of the vector tile specification:
//!
//! ```rust
//! use toolbox_rs::mvt::GeometryEncoder;
//!
//! let mut encoder = GeometryEncoder::new();
//! encoder.move_to(&[(2, 2)]);
//! encoder.line_to(&[(2, 10), (10, 10)]);
//! assert_eq!(encoder.build(), vec![9, 4, 4, 18, 0, 16, 16, 0]);
//! ```
use crate::math::zigzag_encode;

/// Starts a new part of the geometry at the position it is given.
pub const MOVE_TO: u32 = 1;
/// Draws a line from the cursor to the position it is given.
pub const LINE_TO: u32 = 2;
/// Closes the current ring. It takes no parameters and leaves the cursor where
/// it is.
pub const CLOSE_PATH: u32 = 7;

/// The number of times a single command may repeat. The count occupies the bits
/// above the three that hold the id.
pub const MAX_COMMAND_COUNT: u32 = (1 << 29) - 1;

/// Packs a command id and the number of times it repeats into a command
/// integer.
///
/// # Examples
///
/// ```rust
/// use toolbox_rs::mvt::{command_integer, LINE_TO};
///
/// // a LineTo that repeats three times
/// assert_eq!(command_integer(LINE_TO, 3), 26);
/// ```
///
/// # Panics
///
/// Panics if `count` does not fit into the 29 bits that are left next to the
/// id, as the command integer would otherwise silently address a different
/// command.
#[must_use]
pub const fn command_integer(id: u32, count: u32) -> u32 {
    assert!(count <= MAX_COMMAND_COUNT, "command count is out of range");
    (id & 0x7) | (count << 3)
}

/// Splits a command integer back into the id of its command and the number of
/// times it repeats.
///
/// # Examples
///
/// ```rust
/// use toolbox_rs::mvt::{command_and_count, MOVE_TO};
///
/// assert_eq!(command_and_count(9), (MOVE_TO, 1));
/// ```
#[must_use]
pub const fn command_and_count(command_integer: u32) -> (u32, u32) {
    (command_integer & 0x7, command_integer >> 3)
}

/// Builds the command sequence of a feature geometry, keeping track of the
/// cursor that the positions are relative to.
///
/// # Examples
///
/// The polygon `(3,6), (8,12), (20,34)` of the vector tile specification:
///
/// ```rust
/// use toolbox_rs::mvt::GeometryEncoder;
///
/// let mut encoder = GeometryEncoder::new();
/// encoder.move_to(&[(3, 6)]);
/// encoder.line_to(&[(8, 12), (20, 34)]);
/// encoder.close_path();
/// assert_eq!(encoder.build(), vec![9, 6, 12, 18, 10, 12, 24, 44, 15]);
/// ```
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GeometryEncoder {
    data: Vec<u32>,
    x: i32,
    y: i32,
}

impl GeometryEncoder {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            data: Vec::new(),
            x: 0,
            y: 0,
        }
    }

    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            data: Vec::with_capacity(capacity),
            x: 0,
            y: 0,
        }
    }

    /// The position the last command left the cursor at.
    #[must_use]
    pub const fn cursor(&self) -> (i32, i32) {
        (self.x, self.y)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Starts a new part of the geometry at each of the given positions. An
    /// empty slice is ignored, as a command that repeats zero times has no
    /// meaning and readers reject it.
    pub fn move_to(&mut self, positions: &[(i32, i32)]) -> &mut Self {
        self.command(MOVE_TO, positions)
    }

    /// Draws a line from the cursor through each of the given positions.
    pub fn line_to(&mut self, positions: &[(i32, i32)]) -> &mut Self {
        self.command(LINE_TO, positions)
    }

    /// Closes the current ring. The cursor does not move, so the position that
    /// follows is relative to the last one that was drawn rather than to the
    /// beginning of the ring.
    pub fn close_path(&mut self) -> &mut Self {
        self.data.push(command_integer(CLOSE_PATH, 1));
        self
    }

    fn command(&mut self, id: u32, positions: &[(i32, i32)]) -> &mut Self {
        if positions.is_empty() {
            return self;
        }
        let count = u32::try_from(positions.len()).expect("command count is out of range");
        self.data.push(command_integer(id, count));
        for &(x, y) in positions {
            // positions are relative to the one the cursor sits on
            self.data.push(zigzag_encode(x - self.x));
            self.data.push(zigzag_encode(y - self.y));
            self.x = x;
            self.y = y;
        }
        self
    }

    /// Hands out the command sequence.
    #[must_use]
    pub fn build(self) -> Vec<u32> {
        self.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // The examples of the vector tile specification, which is what a reader is
    // written against.

    #[test]
    fn spec_example_point() {
        let mut encoder = GeometryEncoder::new();
        encoder.move_to(&[(25, 17)]);
        assert_eq!(encoder.build(), vec![9, 50, 34]);
    }

    #[test]
    fn spec_example_multi_point() {
        let mut encoder = GeometryEncoder::new();
        encoder.move_to(&[(5, 7), (3, 2)]);
        assert_eq!(encoder.build(), vec![17, 10, 14, 3, 9]);
    }

    #[test]
    fn spec_example_linestring() {
        let mut encoder = GeometryEncoder::new();
        encoder.move_to(&[(2, 2)]);
        encoder.line_to(&[(2, 10), (10, 10)]);
        assert_eq!(encoder.build(), vec![9, 4, 4, 18, 0, 16, 16, 0]);
    }

    #[test]
    fn spec_example_multi_linestring() {
        let mut encoder = GeometryEncoder::new();
        encoder.move_to(&[(2, 2)]);
        encoder.line_to(&[(2, 10), (10, 10)]);
        // the second line string continues from where the first one ended
        encoder.move_to(&[(1, 1)]);
        encoder.line_to(&[(3, 5)]);
        assert_eq!(
            encoder.build(),
            vec![9, 4, 4, 18, 0, 16, 16, 0, 9, 17, 17, 10, 4, 8]
        );
    }

    #[test]
    fn spec_example_polygon() {
        let mut encoder = GeometryEncoder::new();
        encoder.move_to(&[(3, 6)]);
        encoder.line_to(&[(8, 12), (20, 34)]);
        encoder.close_path();
        assert_eq!(encoder.build(), vec![9, 6, 12, 18, 10, 12, 24, 44, 15]);
    }

    #[test]
    fn command_integers_round_trip() {
        for id in [MOVE_TO, LINE_TO, CLOSE_PATH] {
            for count in [1, 2, 3, 255, 4096, MAX_COMMAND_COUNT] {
                let (decoded_id, decoded_count) = command_and_count(command_integer(id, count));
                assert_eq!((decoded_id, decoded_count), (id, count));
            }
        }
    }

    #[test]
    fn command_integers_match_the_specification() {
        assert_eq!(command_integer(MOVE_TO, 1), 9);
        assert_eq!(command_integer(MOVE_TO, 2), 17);
        assert_eq!(command_integer(LINE_TO, 1), 10);
        assert_eq!(command_integer(LINE_TO, 2), 18);
        assert_eq!(command_integer(LINE_TO, 3), 26);
        assert_eq!(command_integer(CLOSE_PATH, 1), 15);
    }

    #[test]
    #[should_panic(expected = "command count is out of range")]
    fn command_count_out_of_range_is_caught() {
        let _ = command_integer(LINE_TO, MAX_COMMAND_COUNT + 1);
    }

    #[test]
    fn cursor_follows_the_last_position() {
        let mut encoder = GeometryEncoder::new();
        assert_eq!(encoder.cursor(), (0, 0));
        encoder.move_to(&[(7, -3)]);
        assert_eq!(encoder.cursor(), (7, -3));
        encoder.line_to(&[(7, -3), (0, 0)]);
        assert_eq!(encoder.cursor(), (0, 0));
    }

    #[test]
    fn close_path_leaves_the_cursor_alone() {
        let mut encoder = GeometryEncoder::new();
        encoder.move_to(&[(4, 4)]);
        encoder.line_to(&[(8, 4)]);
        let before = encoder.cursor();
        encoder.close_path();
        assert_eq!(encoder.cursor(), before);

        // the position after the ring is relative to the last drawn one, not to
        // where the ring started
        encoder.move_to(&[(9, 4)]);
        let data = encoder.build();
        assert_eq!(data[data.len() - 2..], [zigzag_encode(1), zigzag_encode(0)]);
    }

    #[test]
    fn empty_commands_are_dropped() {
        let mut encoder = GeometryEncoder::new();
        encoder.move_to(&[]);
        encoder.line_to(&[]);
        assert!(encoder.is_empty());
        assert_eq!(encoder.cursor(), (0, 0));
        assert_eq!(encoder.build(), Vec::<u32>::new());
    }

    #[test]
    fn repeated_positions_encode_as_zero_deltas() {
        let mut encoder = GeometryEncoder::new();
        encoder.move_to(&[(5, 5)]);
        encoder.line_to(&[(5, 5)]);
        assert_eq!(encoder.build(), vec![9, 10, 10, 10, 0, 0]);
    }

    #[test]
    fn negative_deltas_stay_small() {
        let mut encoder = GeometryEncoder::new();
        encoder.move_to(&[(0, 0)]);
        encoder.line_to(&[(-1, -1)]);
        // a delta of -1 encodes as 1 rather than as a large unsigned value
        assert_eq!(encoder.build(), vec![9, 0, 0, 10, 1, 1]);
    }

    #[test]
    fn extreme_positions_do_not_wrap() {
        let mut encoder = GeometryEncoder::new();
        encoder.move_to(&[(i32::MAX, i32::MIN)]);
        assert_eq!(encoder.cursor(), (i32::MAX, i32::MIN));
        let data = encoder.build();
        assert_eq!(data[0], 9);
        assert_eq!(data[1], zigzag_encode(i32::MAX));
        assert_eq!(data[2], zigzag_encode(i32::MIN));
    }
}
