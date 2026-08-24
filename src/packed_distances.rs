//! A cell's table of distances, written down small.
//!
//! # What the numbers turned out to be
//!
//! The tables are the bulk of what a store ships, so how they are written down
//! was chosen by measuring europe.ptv rather than by picking a scheme. Four
//! were counted, against 252.8 MiB of four-byte entries:
//!
//! ```text
//!   the least of a row, then two bytes an entry   161.1 MiB   64%
//!   the least, then a width per row               126.5 MiB   50%
//!   the least, then a width per table             130.7 MiB   52%
//!   a width per table and no least                112.5 MiB   44%
//! ```
//!
//! Two of those numbers are worth keeping.
//!
//! **Two bytes an entry is the wrong shape.** It is what a delta wants when
//! the values of a row sit close together, and on the coarse levels they do
//! not: at the top level only 8.4% of rows fit, 2.88 million entries have to
//! be written out of line, and the result is *larger* than writing the numbers
//! out in full. A width chosen to fit whatever the table holds has no such
//! cliff.
//!
//! **The least of a row is always nought.** A row holds the distance from a
//! node to every border node of its cell, itself among them, and that one is
//! nought. Measured over all 4,783,400 rows of europe.ptv, not one had a
//! smallest reachable distance above nought, so subtracting it saves nothing
//! and writing it down costs four bytes a row, which is 18.2 MiB.
//!
//! # The encoding
//!
//! One width for the whole table, as many bits as its widest distance needs,
//! and the entries back to back at that width. What cannot be reached takes
//! the largest value the width holds, so the width has room for one more than
//! the widest real distance.
//!
//! A width per row is 14 MiB smaller in the entries alone, and was not taken:
//! rows are read at random, so a width per row wants a table of where each row
//! begins, which is four bytes a row and gives the 14 MiB back with 4 MiB of
//! its own. With one width the row begins at `row * wide * bits` and there is
//! nothing to store or look up.
//!
//! # What this is not
//!
//! This is how a table is written down, not how it is read. A search reads a
//! row as a run of four-byte numbers and this is not that, so a block is
//! unpacked when it is faulted in and the search sees what it always saw.

use rkyv::{Archive, Deserialize, Serialize};

/// A table of distances at one width.
#[derive(Clone, Debug, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub struct PackedDistances {
    /// how many border nodes the cell has, which is the side of the table
    wide: u32,
    /// how many bits an entry takes, one to thirty-two
    bits: u8,
    /// the entries, back to back, at `bits` apiece
    packed: Vec<u8>,
}

/// How many bits it takes to hold every number up to and including `largest`.
pub(crate) fn bits_for(largest: u32) -> u32 {
    if largest == 0 {
        1
    } else {
        u32::BITS - largest.leading_zeros()
    }
}

impl PackedDistances {
    /// Writes a table down, row by row, at one width.
    ///
    /// `matrix` is `wide * wide` entries by row, with [`u32::MAX`] where
    /// nothing can be reached.
    ///
    /// # Panics
    ///
    /// Panics unless the matrix is square and as wide as it says.
    #[must_use]
    pub fn of(matrix: &[u32], wide: usize) -> Self {
        assert_eq!(matrix.len(), wide * wide, "a table is square");

        // the width has to hold the widest real distance and one more, that
        // one being what stands for what cannot be reached
        let widest = matrix
            .iter()
            .copied()
            .filter(|&at| at != u32::MAX)
            .max()
            .unwrap_or(0);
        let bits = bits_for(widest.saturating_add(1)).min(u32::BITS);
        let unreachable = mask_of(bits);

        let mut packed = vec![0_u8; (matrix.len() * bits as usize).div_ceil(8)];
        for (place, &at) in matrix.iter().enumerate() {
            let value = if at == u32::MAX { unreachable } else { at };
            write_at(&mut packed, place * bits as usize, bits, value);
        }
        Self {
            wide: u32::try_from(wide).expect("a cell wider than four bytes count"),
            bits: u8::try_from(bits).expect("a width of more bits than a byte counts"),
            packed,
        }
    }

    #[must_use]
    pub fn wide(&self) -> usize {
        self.wide as usize
    }

    #[must_use]
    pub fn bits(&self) -> u32 {
        u32::from(self.bits)
    }

    /// What the table takes up.
    #[must_use]
    pub fn bytes(&self) -> usize {
        size_of::<Self>() + self.packed.capacity()
    }

    /// Reads the whole table back, as the four-byte numbers a search wants.
    ///
    /// Into a buffer the caller keeps, a block holding many tables and each
    /// being unpacked once when the block is faulted in.
    pub fn unpack_into(&self, out: &mut Vec<u32>) {
        out.clear();
        let entries = self.wide as usize * self.wide as usize;
        out.reserve(entries);
        let bits = u32::from(self.bits);
        let unreachable = mask_of(bits);
        for place in 0..entries {
            let value = read_at(&self.packed, place * bits as usize, bits);
            out.push(if value == unreachable {
                u32::MAX
            } else {
                value
            });
        }
    }

    /// One entry, without unpacking the rest.
    ///
    /// # Panics
    ///
    /// Panics if the entry is not in the table.
    #[must_use]
    pub fn at(&self, source: usize, target: usize) -> u32 {
        let wide = self.wide as usize;
        assert!(source < wide && target < wide, "no such entry");
        let bits = u32::from(self.bits);
        let value = read_at(&self.packed, (source * wide + target) * bits as usize, bits);
        if value == mask_of(bits) {
            u32::MAX
        } else {
            value
        }
    }
}

/// The largest value a width holds, which is what stands for the unreachable.
fn mask_of(bits: u32) -> u32 {
    if bits >= u32::BITS {
        u32::MAX
    } else {
        (1_u32 << bits) - 1
    }
}

/// Writes `bits` of `value` at a bit offset.
///
/// An entry straddles at most five bytes at any width up to thirty-two, so the
/// bytes it touches are read into a wide accumulator, the value laid into it,
/// and the accumulator written back.
pub(crate) fn write_at(packed: &mut [u8], at: usize, bits: u32, value: u32) {
    let byte = at / 8;
    let shift = (at % 8) as u32;
    let mut held = 0_u64;
    for (place, &part) in packed[byte..].iter().take(5).enumerate() {
        held |= u64::from(part) << (place * 8);
    }
    held &= !(u64::from(mask_of(bits)) << shift);
    held |= u64::from(value & mask_of(bits)) << shift;
    for (place, part) in packed[byte..].iter_mut().take(5).enumerate() {
        *part = u8::try_from((held >> (place * 8)) & 0xFF).expect("a byte of a byte");
    }
}

/// Reads `bits` from a bit offset.
pub(crate) fn read_at(packed: &[u8], at: usize, bits: u32) -> u32 {
    let byte = at / 8;
    let shift = (at % 8) as u32;
    let mut held = 0_u64;
    for (place, &part) in packed[byte..].iter().take(5).enumerate() {
        held |= u64::from(part) << (place * 8);
    }
    u32::try_from((held >> shift) & u64::from(mask_of(bits))).expect("a value of its own width")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{RngExt, SeedableRng, rngs::StdRng};

    fn round_trip(matrix: &[u32], wide: usize) {
        let packed = PackedDistances::of(matrix, wide);
        let mut read = Vec::new();
        packed.unpack_into(&mut read);
        assert_eq!(read, matrix, "at {} bits", packed.bits());
        for source in 0..wide {
            for target in 0..wide {
                assert_eq!(
                    packed.at(source, target),
                    matrix[source * wide + target],
                    "entry {source},{target}"
                );
            }
        }
    }

    #[test]
    fn a_table_reads_back_as_it_was_written() {
        let mut rng = StdRng::seed_from_u64(0x5EED);
        for wide in [1_usize, 2, 3, 7, 16, 33] {
            for largest in [0_u32, 1, 2, 255, 256, 65_535, 65_536, 1_000_000] {
                let matrix = (0..wide * wide)
                    .map(|_| rng.random_range(0..=largest))
                    .collect::<Vec<_>>();
                round_trip(&matrix, wide);
            }
        }
    }

    #[test]
    fn what_cannot_be_reached_reads_back_unreachable() {
        let matrix = vec![u32::MAX; 9];
        round_trip(&matrix, 3);

        // and mixed in with what can
        let matrix = vec![0, u32::MAX, 5, u32::MAX, 0, u32::MAX, 7, 9, 0];
        round_trip(&matrix, 3);
    }

    /// The width has to leave room for the unreachable above the widest real
    /// distance, or a table whose widest distance is a power of two less one
    /// would read that distance back as unreachable.
    #[test]
    fn the_widest_distance_is_not_mistaken_for_unreachable() {
        for widest in [1_u32, 3, 7, 255, 65_535] {
            let matrix = vec![0, widest, widest, 0];
            let packed = PackedDistances::of(&matrix, 2);
            assert_eq!(packed.at(0, 1), widest, "at {} bits", packed.bits());
            round_trip(&matrix, 2);
        }
    }

    #[test]
    fn a_table_of_the_widest_numbers_there_are_still_reads_back() {
        // one below the unreachable is a real distance and has to survive
        let matrix = vec![0, u32::MAX - 1, 1, 0];
        round_trip(&matrix, 2);
        assert_eq!(PackedDistances::of(&matrix, 2).bits(), 32);
    }

    #[test]
    fn a_narrow_table_takes_narrow_room() {
        // sixteen entries that fit in three bits apiece is six bytes, against
        // sixty-four for four bytes each
        let matrix = (0..16).map(|at| at % 6).collect::<Vec<_>>();
        let packed = PackedDistances::of(&matrix, 4);
        assert_eq!(packed.bits(), 3);
        round_trip(&matrix, 4);
    }

    #[test]
    fn a_table_serializes() {
        let matrix = vec![0, 3, 4, 0];
        let packed = PackedDistances::of(&matrix, 2);
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&packed).expect("serializes");
        let read: PackedDistances =
            rkyv::from_bytes::<PackedDistances, rkyv::rancor::Error>(&bytes).expect("deserializes");
        assert_eq!(read, packed);
    }
}
