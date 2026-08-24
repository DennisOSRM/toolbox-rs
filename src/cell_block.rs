//! A run of cells of one level, with their tables written back to back.
//!
//! # What a block does not hold
//!
//! Almost everything a table needs to be read is already known without it.
//!
//! How wide a cell's table is, is how many border nodes the cell has, which
//! the [cell tree](crate::cell_tree::CellTree) says. A block that stored it
//! again would pay four bytes a cell for an answer it was handed. Where a
//! cell's table begins follows from the widths and the bit widths of the cells
//! before it, so it is added up when the block is read rather than written
//! down. What is left is one byte a cell, the width its entries are packed at,
//! and the entries themselves.
//!
//! That is what "the framing moves into the directory" comes to: held one
//! table to a struct, the frame was 19.0 MiB over europe.ptv and 15.2 MiB of
//! it on the finest level alone.
//!
//! # Which node a place in a table is
//!
//! A table is addressed by where a node sits in it, so reading one means
//! turning a place back into a node. Under
//! [`Numbering::CellPath`](crate::node_ordering::Numbering::CellPath) the
//! nodes of a cell are exactly the run of numbers between its first and its
//! last, measured over europe.ptv and true of every cell of every level. So a
//! place is a node as soon as it is known where in the run the border nodes
//! are.
//!
//! On the finest level they are the front of it: a node on the border of its
//! level-0 cell is a node on a border at all, and the numbering puts those
//! first inside a cell. Measured, that holds for every one of 497,965 cells,
//! and a block of the finest level therefore stores nothing at all about
//! which nodes its tables are about.
//!
//! Above the finest it cannot hold, and not because the numbering is wrong. A
//! level-1 cell's run is the runs of its children laid end to end, each sorted
//! within itself; gathering its border nodes to the front would take them out
//! of their children and the children would no longer be runs. One or the
//! other, not both. Measured: 1.9% of level-1 cells have their border nodes in
//! front, and near none above that.
//!
//! So above the finest level a block holds where each border node sits inside
//! its cell's run, packed at as many bits as the widest run in the block
//! needs. That is an offset into a cell rather than a node of the graph, which
//! on europe.ptv is between six and twenty bits rather than thirty-two.

use rkyv::{Archive, Deserialize, Serialize};

use crate::packed_distances::{bits_for, read_at, write_at};

/// The version this is written under.
pub const VERSION: u16 = 1;

/// A run of cells of one level and everything needed to read their tables.
#[derive(Clone, Debug, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub struct CellBlock {
    version: u16,
    level: u8,
    /// the first cell of the run
    first: u32,
    /// how many bits an entry of each cell's table takes, one a cell
    bits: Vec<u8>,
    /// Where each border node sits in its cell's run of nodes, at
    /// `place_bits` apiece and laid out cell after cell.
    ///
    /// Empty where the border nodes are the front of the run, which is the
    /// finest level, and there is nothing to say.
    places: Vec<u8>,
    place_bits: u8,
    /// the tables, back to back, each at its own width
    tables: Vec<u8>,
}

/// What a block is built from, one of these a cell, in key order.
pub struct CellEntry<'a> {
    /// the cell's table, `wide * wide` entries by row
    pub matrix: &'a [u32],
    /// how many border nodes the cell has, which is the side of the table
    pub wide: usize,
    /// where each border node sits in the cell's run of nodes, and empty
    /// where they are the front of it
    pub places: &'a [u32],
    /// how many nodes the cell holds, which bounds a place
    pub holds: usize,
}

impl CellBlock {
    /// Writes a run of cells down.
    ///
    /// `border_leads` says that the border nodes of every cell are the front
    /// of its run, so that nothing is stored about where they are.
    ///
    /// # Panics
    ///
    /// Panics if a matrix is not square, or if places are given when the
    /// border nodes are said to lead, or missing when they are not.
    #[must_use]
    pub fn of(level: usize, first: u32, cells: &[CellEntry<'_>], border_leads: bool) -> Self {
        let mut bits = Vec::with_capacity(cells.len());
        let mut table_bits = 0_usize;
        let mut widest_run = 0_u32;
        let mut places_count = 0_usize;
        for cell in cells {
            assert_eq!(
                cell.matrix.len(),
                cell.wide * cell.wide,
                "a table is square"
            );
            assert_eq!(
                cell.places.is_empty(),
                border_leads,
                "a cell says one thing about its border and the block another"
            );
            let widest = cell
                .matrix
                .iter()
                .copied()
                .filter(|&at| at != u32::MAX)
                .max()
                .unwrap_or(0);
            let width = bits_for(widest.saturating_add(1)).min(u32::BITS);
            bits.push(u8::try_from(width).expect("a width of more bits than a byte counts"));
            table_bits += cell.matrix.len() * width as usize;
            widest_run = widest_run.max(u32::try_from(cell.holds).unwrap_or(u32::MAX));
            places_count += cell.places.len();
        }

        // a place is an offset into a cell rather than a node of the graph, so
        // it needs room for the longest run in the block and no more
        let place_bits = if border_leads {
            0
        } else {
            bits_for(widest_run.max(1))
        };

        let mut tables = vec![0_u8; table_bits.div_ceil(8)];
        let mut places = vec![0_u8; (places_count * place_bits as usize).div_ceil(8)];
        let mut at_table = 0_usize;
        let mut at_place = 0_usize;
        for (cell, &width) in cells.iter().zip(&bits) {
            let width = u32::from(width);
            let unreachable = mask_of(width);
            for &entry in cell.matrix {
                let value = if entry == u32::MAX {
                    unreachable
                } else {
                    entry
                };
                write_at(&mut tables, at_table, width, value);
                at_table += width as usize;
            }
            for &place in cell.places {
                write_at(&mut places, at_place, place_bits, place);
                at_place += place_bits as usize;
            }
        }

        Self {
            version: VERSION,
            level: u8::try_from(level).expect("more levels than a byte counts"),
            first,
            bits,
            places,
            place_bits: u8::try_from(place_bits).expect("a width of more bits than a byte counts"),
            tables,
        }
    }

    #[must_use]
    pub fn level(&self) -> usize {
        self.level as usize
    }

    #[must_use]
    pub fn first(&self) -> u32 {
        self.first
    }

    #[must_use]
    pub fn cells(&self) -> usize {
        self.bits.len()
    }

    /// Whether the border nodes of every cell are the front of its run.
    #[must_use]
    pub fn border_leads(&self) -> bool {
        self.places.is_empty()
    }

    /// What the block takes up.
    #[must_use]
    pub fn bytes(&self) -> usize {
        size_of::<Self>() + self.bits.capacity() + self.places.capacity() + self.tables.capacity()
    }

    /// What is written down beyond the entries themselves.
    #[must_use]
    pub fn framing_bytes(&self) -> usize {
        self.bits.capacity() + self.places.capacity()
    }

    /// Reads one cell's table back, as the four-byte numbers a search wants.
    ///
    /// `widths` says how wide each cell of the block is, which the cell tree
    /// knows and the block does not store. It is read from the front of the
    /// block up to the cell asked for, so a caller reading the whole block
    /// should walk it in order.
    ///
    /// # Panics
    ///
    /// Panics if the cell is not in the block, or if fewer widths are given
    /// than the block has cells.
    pub fn unpack_into(&self, which: usize, widths: &[usize], out: &mut Vec<u32>) {
        assert!(which < self.cells(), "no such cell in the block");
        assert!(
            widths.len() >= self.cells(),
            "a width is wanted for each cell"
        );

        // where the table begins follows from what came before it
        let mut at = 0_usize;
        for (before, &wide) in widths.iter().enumerate().take(which) {
            at += wide * wide * self.bits[before] as usize;
        }

        let width = u32::from(self.bits[which]);
        let unreachable = mask_of(width);
        let wide = widths[which];
        out.clear();
        out.reserve(wide * wide);
        for _ in 0..wide * wide {
            let value = read_at(&self.tables, at, width);
            out.push(if value == unreachable {
                u32::MAX
            } else {
                value
            });
            at += width as usize;
        }
    }

    /// Where the border nodes of a cell sit in its run of nodes.
    ///
    /// Empty where they are the front of it and there was nothing to store.
    ///
    /// # Panics
    ///
    /// Panics if the cell is not in the block.
    pub fn places_into(&self, which: usize, widths: &[usize], out: &mut Vec<u32>) {
        assert!(which < self.cells(), "no such cell in the block");
        out.clear();
        if self.border_leads() {
            return;
        }
        let bits = u32::from(self.place_bits);
        let mut at = widths.iter().take(which).sum::<usize>() * bits as usize;
        out.reserve(widths[which]);
        for _ in 0..widths[which] {
            out.push(read_at(&self.places, at, bits));
            at += bits as usize;
        }
    }

    /// Refuses a block written under a version this does not know.
    ///
    /// # Errors
    ///
    /// Returns the version found when it is not the one this reads.
    pub fn check_version(&self) -> Result<(), u16> {
        if self.version == VERSION {
            Ok(())
        } else {
            Err(self.version)
        }
    }
}

fn mask_of(bits: u32) -> u32 {
    if bits >= u32::BITS {
        u32::MAX
    } else {
        (1_u32 << bits) - 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{RngExt, SeedableRng, rngs::StdRng};

    fn matrices(rng: &mut StdRng, sizes: &[usize], largest: u32) -> Vec<Vec<u32>> {
        sizes
            .iter()
            .map(|&wide| {
                (0..wide * wide)
                    .map(|at| {
                        if at % 11 == 3 {
                            u32::MAX
                        } else {
                            rng.random_range(0..=largest)
                        }
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn every_table_of_a_block_reads_back() {
        let mut rng = StdRng::seed_from_u64(0x5EED);
        for largest in [1_u32, 255, 70_000, 3_000_000] {
            let sizes = vec![1_usize, 4, 2, 9, 3];
            let held = matrices(&mut rng, &sizes, largest);
            let cells = held
                .iter()
                .zip(&sizes)
                .map(|(matrix, &wide)| CellEntry {
                    matrix,
                    wide,
                    places: &[],
                    holds: wide * 3,
                })
                .collect::<Vec<_>>();
            let block = CellBlock::of(0, 100, &cells, true);
            assert_eq!(block.cells(), sizes.len());
            assert_eq!(block.first(), 100);
            assert!(block.border_leads());

            let mut out = Vec::new();
            for (which, matrix) in held.iter().enumerate() {
                block.unpack_into(which, &sizes, &mut out);
                assert_eq!(&out, matrix, "cell {which} at largest {largest}");
            }
        }
    }

    #[test]
    fn the_places_of_a_block_read_back() {
        let sizes = vec![2_usize, 3];
        let held: Vec<Vec<u32>> = vec![vec![0, 4, 4, 0], vec![0, 1, 2, 1, 0, 3, 2, 3, 0]];
        // where each border node sits inside a run of a hundred nodes
        let places: Vec<Vec<u32>> = vec![vec![0, 57], vec![3, 40, 99]];
        let cells = held
            .iter()
            .zip(&sizes)
            .zip(&places)
            .map(|((matrix, &wide), places)| CellEntry {
                matrix,
                wide,
                places,
                holds: 100,
            })
            .collect::<Vec<_>>();
        let block = CellBlock::of(2, 7, &cells, false);
        assert!(!block.border_leads());
        // a hundred nodes wants seven bits, not thirty-two
        assert_eq!(block.place_bits, 7);

        let mut out = Vec::new();
        for (which, wanted) in places.iter().enumerate() {
            block.places_into(which, &sizes, &mut out);
            assert_eq!(&out, wanted, "cell {which}");
        }
    }

    #[test]
    fn a_block_carries_nothing_it_was_handed() {
        // eight cells of four border nodes at eight bits is 128 bytes of
        // entries, and the framing is one byte a cell and nothing else
        let sizes = vec![4_usize; 8];
        let held = sizes
            .iter()
            .map(|&wide| (0..wide * wide).map(|at| (at % 200) as u32).collect())
            .collect::<Vec<Vec<u32>>>();
        let cells = held
            .iter()
            .zip(&sizes)
            .map(|(matrix, &wide)| CellEntry {
                matrix,
                wide,
                places: &[],
                holds: wide,
            })
            .collect::<Vec<_>>();
        let block = CellBlock::of(0, 0, &cells, true);
        assert_eq!(block.framing_bytes(), 8, "one byte a cell and no more");
    }

    #[test]
    fn a_block_reads_back_as_it_was_written() {
        let sizes = vec![3_usize, 5];
        let held: Vec<Vec<u32>> = vec![
            (0..9).map(|at| at * 7).collect(),
            (0..25).map(|at| at * 3).collect(),
        ];
        let cells = held
            .iter()
            .zip(&sizes)
            .map(|(matrix, &wide)| CellEntry {
                matrix,
                wide,
                places: &[],
                holds: wide,
            })
            .collect::<Vec<_>>();
        let block = CellBlock::of(1, 42, &cells, true);
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&block).expect("serializes");
        let read: CellBlock =
            rkyv::from_bytes::<CellBlock, rkyv::rancor::Error>(&bytes).expect("deserializes");
        assert_eq!(read, block);
        assert!(read.check_version().is_ok());
    }
}
