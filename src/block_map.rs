//! Where every block is, and what it covers.
//!
//! # Four ranges that are the same range
//!
//! A block holds a run of cells of one level. Numbered as their keys run, that
//! run is at once a range of keys, a range of cell numbers and a range of node
//! numbers, so an entry says all three as bounds and the store looks a block
//! up by whichever it happens to hold.
//!
//! That is what the two renumberings bought. Out of the assembly none of the
//! three agreed with either of the others and an entry would have had to carry
//! a list of the cells it held.
//!
//! # What is absent means what was not downloaded
//!
//! A store is shipped in pieces and a device may hold some of them. There is
//! no flag for that: a key that no entry covers is a key nobody has, and a
//! lookup says so by finding nothing. A region is a range of keys that is
//! present or is not.
//!
//! # Blocks are named by what is in them
//!
//! Each entry carries a hash of the block's bytes. Two builds that produce the
//! same block produce the same name for it, so a device updating from one
//! release to the next downloads the blocks whose contents changed rather than
//! the blocks whose numbers changed. Nothing here does that yet; the naming is
//! what has to be in the format from the start, because it cannot be added to
//! files already shipped.

use rkyv::{Archive, Deserialize, Serialize};

use crate::level_directory::CellId;

/// The version this is written under.
pub const VERSION: u16 = 1;

/// Where one block sits and what it covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub struct BlockEntry {
    /// the first key of the range this block covers, and the last
    pub first_key: u128,
    pub last_key: u128,
    /// which level its cells are of
    pub level: u8,
    /// which codec its bytes were written with
    pub codec: u8,
    /// the first cell of the run, and how many
    pub first_cell: CellId,
    pub cells: u32,
    /// the first node the run holds, and how many
    pub first_node: u32,
    pub nodes: u32,
    /// where the bytes begin in the file they are in
    pub at: u64,
    /// how many bytes it takes on disk, and how many once read back
    pub stored: u32,
    pub unpacked: u32,
    /// a name for the bytes, so that a later release can be told what changed
    pub hash: u64,
}

impl BlockEntry {
    /// Whether a key falls in the range this block covers.
    #[must_use]
    pub fn holds_key(&self, key: u128) -> bool {
        (self.first_key..=self.last_key).contains(&key)
    }

    /// Whether a cell of this block's level falls in its run.
    #[must_use]
    pub fn holds_cell(&self, cell: CellId) -> bool {
        cell >= self.first_cell && cell - self.first_cell < self.cells
    }
}

/// Every block of a store, in the order they are laid out.
#[derive(Clone, Debug, Default, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub struct BlockMap {
    version: u16,
    /// sorted by level and then by the first cell of the run
    entries: Vec<BlockEntry>,
}

impl BlockMap {
    /// Takes the entries and puts them in the order lookups want.
    #[must_use]
    pub fn of(mut entries: Vec<BlockEntry>) -> Self {
        entries.sort_unstable_by_key(|entry| (entry.level, entry.first_cell));
        Self {
            version: VERSION,
            entries,
        }
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[must_use]
    pub fn entries(&self) -> &[BlockEntry] {
        &self.entries
    }

    /// The block holding a cell, and nothing where no block does.
    ///
    /// Nothing means the piece of the map that cell is in was not downloaded,
    /// which is a question rather than a fault.
    #[must_use]
    pub fn holding_cell(&self, level: usize, cell: CellId) -> Option<&BlockEntry> {
        let level = u8::try_from(level).ok()?;
        // the last block of this level that begins at or before the cell
        let after = self
            .entries
            .partition_point(|entry| (entry.level, entry.first_cell) <= (level, cell));
        let entry = self.entries.get(after.checked_sub(1)?)?;
        (entry.level == level && entry.holds_cell(cell)).then_some(entry)
    }

    /// The block holding a key, and nothing where no block does.
    ///
    /// A key names a cell of one level, so the level is asked for too: the
    /// same subtree has a key at every level above its own and they share
    /// their upper bits.
    #[must_use]
    pub fn holding_key(&self, level: usize, key: u128) -> Option<&BlockEntry> {
        let level = u8::try_from(level).ok()?;
        self.entries
            .iter()
            .find(|entry| entry.level == level && entry.holds_key(key))
    }

    /// What the store comes to on disk, and what it comes to read back.
    #[must_use]
    pub fn bytes(&self) -> (u64, u64) {
        self.entries
            .iter()
            .fold((0, 0), |(stored, unpacked), entry| {
                (
                    stored + u64::from(entry.stored),
                    unpacked + u64::from(entry.unpacked),
                )
            })
    }

    /// Refuses a map written under a version this does not know.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(level: u8, first_cell: CellId, cells: u32, first_key: u128) -> BlockEntry {
        BlockEntry {
            first_key,
            last_key: first_key + u128::from(cells) - 1,
            level,
            codec: 0,
            first_cell,
            cells,
            first_node: first_cell * 10,
            nodes: cells * 10,
            at: u64::from(first_cell) * 100,
            stored: cells * 8,
            unpacked: cells * 16,
            hash: u64::from(first_cell),
        }
    }

    fn map() -> BlockMap {
        // deliberately out of order, since a build may emit them so
        BlockMap::of(vec![
            entry(1, 0, 3, 1000),
            entry(0, 10, 5, 100),
            entry(0, 0, 10, 0),
            entry(0, 20, 4, 200),
        ])
    }

    #[test]
    fn a_cell_finds_the_block_that_holds_it() {
        let map = map();
        for (level, cell, wanted) in [
            (0_usize, 0 as CellId, Some(0 as CellId)),
            (0, 9, Some(0)),
            (0, 10, Some(10)),
            (0, 14, Some(10)),
            (0, 20, Some(20)),
            (0, 23, Some(20)),
            (1, 2, Some(0)),
        ] {
            assert_eq!(
                map.holding_cell(level, cell).map(|entry| entry.first_cell),
                wanted,
                "level {level}, cell {cell}"
            );
        }
    }

    /// A cell in a gap is a cell nobody downloaded, and the answer is nothing
    /// rather than the block next door.
    #[test]
    fn a_cell_no_block_holds_finds_nothing() {
        let map = map();
        for (level, cell) in [(0_usize, 15 as CellId), (0, 24), (0, 999), (1, 3), (2, 0)] {
            assert!(
                map.holding_cell(level, cell).is_none(),
                "level {level}, cell {cell} found a block"
            );
        }
    }

    #[test]
    fn a_key_finds_the_block_whose_range_covers_it() {
        let map = map();
        assert_eq!(map.holding_key(0, 5).map(|entry| entry.first_cell), Some(0));
        assert_eq!(
            map.holding_key(0, 103).map(|entry| entry.first_cell),
            Some(10)
        );
        assert_eq!(map.holding_key(0, 50), None, "a key in a gap");
        assert_eq!(
            map.holding_key(1, 1001).map(|entry| entry.first_cell),
            Some(0)
        );
        // the same key at another level is another block, or none
        assert_eq!(map.holding_key(1, 5), None);
    }

    #[test]
    fn a_map_says_what_it_comes_to() {
        let map = map();
        assert_eq!(map.len(), 4);
        let (stored, unpacked) = map.bytes();
        assert_eq!(stored, (10 + 5 + 4 + 3) * 8);
        assert_eq!(unpacked, (10 + 5 + 4 + 3) * 16);
    }

    #[test]
    fn a_map_reads_back_as_it_was_written() {
        let map = map();
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&map).expect("serializes");
        let read: BlockMap =
            rkyv::from_bytes::<BlockMap, rkyv::rancor::Error>(&bytes).expect("deserializes");
        assert_eq!(read, map);
        assert!(read.check_version().is_ok());
    }
}
