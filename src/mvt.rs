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

#[cfg(test)]
mod tests {
    use super::*;

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
}
