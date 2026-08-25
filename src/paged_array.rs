//! A fixed-width array read off a file, holding nothing but where it is.
//!
//! # Why there is no index
//!
//! Every array an instance was still keeping whole -- what each cell holds,
//! where each cell's nodes begin, the runs of the partition and the buckets
//! over them, where each block of arcs starts -- is asked the same question:
//! give me the entry at this number. Not *find* it. The number is worked out
//! from a cell, or a node, or a block, and the entry is at that place.
//!
//! So there is nothing to search and nothing to search with. The block an
//! entry is in is its number divided by how many go in a block, and where that
//! block sits in the file is that block's number times how long one is. Both
//! are arithmetic. What stays resident is a file handle, four numbers and a
//! share of the pool: the same however many entries there are.
//!
//! That is the whole of the difference from a B-tree, which is what a sorted
//! array wants when the question is *find the entry at or before this key*.
//! One of those keeps its root resident and reads its way down. This keeps
//! nothing and computes.
//!
//! # Why the blocks are not compressed
//!
//! A block's place in the file has to be arithmetic, and a compressed block is
//! only as long as it turned out to be, so where the next one starts is a
//! thing you have to be told. Being told means an offset an entry, which is
//! the resident array this was written to abolish.
//!
//! These arrays are the small ones -- a continent's come to fifteen mebibytes
//! against four hundred of arcs -- so the room a codec would save is worth
//! less than the index it would cost.

use std::{
    fs::File,
    io::{BufWriter, Write},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use crate::{
    block_store::{NotRead, read_at},
    pool::{Held, Key, Pool},
};

/// How many bytes a block of an array holds.
///
/// The same sixty four kibibytes the cell tables are packed into, for the same
/// reason: it is what a read costs least per entry brought back.
pub const BLOCK_BYTES: usize = 64 * 1024;

/// Tells one array from another, so their blocks do not collide in the pool.
static NEXT_ARRAY: AtomicUsize = AtomicUsize::new(0);

/// A run of fixed-width entries on a file.
#[derive(Debug)]
pub struct PagedArray {
    held: File,
    /// where the entries begin in the file
    at: u64,
    /// how many entries there are, and how wide one is
    entries: usize,
    wide: usize,
    /// how many go in a block
    apiece: usize,
    /// which array this is, for the pool
    which: u16,
    pool: Arc<Pool>,
    reads: AtomicUsize,
}

impl PagedArray {
    /// Opens an array of `entries` entries of `wide` bytes, beginning at `at`.
    ///
    /// # Errors
    ///
    /// Returns whatever went wrong opening the file.
    ///
    /// # Panics
    ///
    /// Panics for an entry wider than a block, which no array here has.
    pub fn open(
        path: &Path,
        at: u64,
        entries: usize,
        wide: usize,
        pool: Arc<Pool>,
    ) -> std::io::Result<Self> {
        assert!(wide > 0 && wide <= BLOCK_BYTES, "an entry of {wide} bytes");
        Ok(Self {
            held: File::open(path)?,
            at,
            entries,
            wide,
            apiece: BLOCK_BYTES / wide,
            which: u16::try_from(NEXT_ARRAY.fetch_add(1, Ordering::Relaxed))
                .expect("more arrays than a short counts"),
            pool,
            reads: AtomicUsize::new(0),
        })
    }

    /// How many entries it holds.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries == 0
    }

    /// How many blocks were read off the file.
    #[must_use]
    pub fn reads(&self) -> usize {
        self.reads.load(Ordering::Relaxed)
    }

    /// What this costs standing still, whatever it holds.
    ///
    /// The blocks are the pool's; this is the handle and the four numbers, and
    /// it is the same for an array of ten entries and one of ten million.
    #[must_use]
    pub fn bytes(&self) -> usize {
        size_of::<Self>()
    }

    /// The entry at a number, and nothing past the end.
    ///
    /// `N` is how wide an entry is and has to be the width the array was
    /// opened with.
    ///
    /// # Panics
    ///
    /// Panics if `N` is not that width, which is a caller reading an array as
    /// something it is not.
    #[must_use]
    pub fn get<const N: usize>(&self, index: usize) -> Option<[u8; N]> {
        assert_eq!(N, self.wide, "an entry read at the wrong width");
        if index >= self.entries {
            return None;
        }
        let block = index / self.apiece;
        let held = self.block(block).ok()?;
        let at = (index % self.apiece) * self.wide;
        held.get(at..at + N)?.try_into().ok()
    }

    /// The block at an ordinal, read if the pool is not holding it.
    fn block(&self, which: usize) -> Result<Arc<Vec<u8>>, NotRead> {
        let key = Key::Array(
            self.which,
            u32::try_from(which).map_err(|_| NotRead::NotHere)?,
        );
        if let Some(Held::Bytes(held)) = self.pool.get(&key) {
            return Ok(held);
        }
        // the last block is short where the entries do not fill it
        let first = which * self.apiece;
        let held = (self.entries - first).min(self.apiece) * self.wide;
        let mut into = vec![0_u8; held];
        read_at(
            &self.held,
            self.at + (which * self.apiece * self.wide) as u64,
            &mut into,
        )?;
        self.reads.fetch_add(1, Ordering::Relaxed);
        self.pool.note_read();
        let held = Arc::new(into);
        self.pool.put(key, Held::Bytes(Arc::clone(&held)));
        Ok(held)
    }
}

/// Writes a run of fixed-width entries out for [`PagedArray`] to read.
///
/// The entries go down one after another with nothing between them, since
/// where each one sits is worked out and not looked up.
///
/// # Errors
///
/// Returns whatever went wrong writing them.
pub fn write<const N: usize>(
    out: &mut BufWriter<File>,
    entries: &[[u8; N]],
) -> std::io::Result<()> {
    for entry in entries {
        out.write_all(entry)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An array of a known pattern, so a wrong entry is obvious.
    fn written(entries: usize) -> (tempfile::TempDir, std::path::PathBuf) {
        let held = tempfile::tempdir().expect("a directory to write in");
        let path = held.path().join("array");
        let mut out = BufWriter::new(File::create(&path).expect("a file"));
        let all: Vec<[u8; 8]> = (0..entries)
            .map(|at| (at as u64).wrapping_mul(0x9E37_79B9).to_le_bytes())
            .collect();
        write(&mut out, &all).expect("the entries");
        out.flush().expect("flushed");
        (held, path)
    }

    fn wanted(at: usize) -> [u8; 8] {
        (at as u64).wrapping_mul(0x9E37_79B9).to_le_bytes()
    }

    /// The one that matters: every entry, over more blocks than the pool can
    /// hold at once, so it is reading and letting go throughout.
    #[test]
    fn every_entry_reads_back_whatever_the_pool_is_holding() {
        let entries = 40_000;
        let (_held, path) = written(entries);
        let pool = Pool::of(3 * BLOCK_BYTES);
        let array = PagedArray::open(&path, 0, entries, 8, Arc::clone(&pool)).expect("an array");

        assert_eq!(array.len(), entries);
        // forwards, so the blocks come in order, and backwards, so they do not
        for at in (0..entries).chain((0..entries).rev()) {
            assert_eq!(array.get::<8>(at), Some(wanted(at)), "entry {at}");
        }
        assert_eq!(array.get::<8>(entries), None, "past the end");
        assert!(pool.faults().evicted > 0, "the pool never had to let go");
        assert!(
            array.reads() > entries * 8 / BLOCK_BYTES,
            "nothing was read"
        );
    }

    /// What the whole thing is for: what it costs does not go with what it
    /// holds.
    #[test]
    fn what_it_costs_standing_still_does_not_go_with_how_much_it_holds() {
        let pool = Pool::of(1 << 20);
        let (_small, small) = written(16);
        let (_large, large) = written(400_000);
        let small = PagedArray::open(&small, 0, 16, 8, Arc::clone(&pool)).expect("an array");
        let large = PagedArray::open(&large, 0, 400_000, 8, Arc::clone(&pool)).expect("an array");
        assert_eq!(small.bytes(), large.bytes());
        assert!(large.len() > small.len() * 1000);
    }

    #[test]
    fn a_block_already_held_is_not_read_again() {
        let (_held, path) = written(8_000);
        let pool = Pool::of(1 << 20);
        let array = PagedArray::open(&path, 0, 8_000, 8, pool).expect("an array");
        for _ in 0..4 {
            for at in 0..8_000 {
                assert_eq!(array.get::<8>(at), Some(wanted(at)));
            }
        }
        let blocks = (8_000 * 8usize).div_ceil(BLOCK_BYTES);
        assert_eq!(array.reads(), blocks, "a block was read more than once");
    }

    #[test]
    #[should_panic(expected = "the wrong width")]
    fn an_entry_read_at_the_wrong_width_is_refused() {
        let (_held, path) = written(8);
        let array = PagedArray::open(&path, 0, 8, 8, Pool::of(1 << 20)).expect("an array");
        let _ = array.get::<4>(0);
    }
}
