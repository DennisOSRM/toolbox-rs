//! Writing the blocks of a store to a file, and reading one back out of it.
//!
//! # What a read costs
//!
//! Finding a cell is a binary search of the block map, which is in memory.
//! Then one positional read of the block's bytes, one pass of the codec over
//! them, and the entries of the one cell asked for unpacked into a buffer the
//! caller keeps. Nothing else in the block is touched.
//!
//! The read is positional rather than a seek and a read, so a store may be
//! read from several threads through one open file without any of them moving
//! another's cursor.
//!
//! # What is not here
//!
//! No cache. A store that pages wants one, and where it goes is above this:
//! this is what a fault does, not when a fault happens. Nor is there anything
//! about which levels are pinned, for the same reason.

use std::{
    fs::File,
    io::{self, BufWriter, Write},
    path::Path,
};

use crate::{
    block_codec::Codec,
    block_map::{BlockEntry, BlockMap},
    cell_block::CellBlock,
    cell_tree::CellTree,
    level_directory::CellId,
};

/// Reads exactly as many bytes as the buffer holds, from a place in a file.
///
/// Positional, so that one open file answers several threads at once.
#[cfg(unix)]
fn read_at(file: &File, at: u64, into: &mut [u8]) -> io::Result<()> {
    use std::os::unix::fs::FileExt;
    file.read_exact_at(into, at)
}

#[cfg(windows)]
fn read_at(file: &File, at: u64, into: &mut [u8]) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    let mut read = 0;
    while read < into.len() {
        match file.seek_read(&mut into[read..], at + read as u64)? {
            0 => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "the file ended inside a block",
                ));
            }
            some => read += some,
        }
    }
    Ok(())
}

/// Lays blocks into a file one after another and remembers where each went.
pub struct BlockWriter {
    out: BufWriter<File>,
    at: u64,
    entries: Vec<BlockEntry>,
}

impl BlockWriter {
    /// # Errors
    ///
    /// Returns what went wrong opening the file.
    pub fn create(path: &Path) -> io::Result<Self> {
        Ok(Self {
            out: BufWriter::new(File::create(path)?),
            at: 0,
            entries: Vec::new(),
        })
    }

    /// Writes one block down and notes what it covers.
    ///
    /// # Errors
    ///
    /// Returns what went wrong writing, or if the block will not serialize.
    ///
    /// # Panics
    ///
    /// Panics if a block is larger than four thousand million bytes, which is
    /// not a block anybody should be cutting.
    pub fn push(
        &mut self,
        block: &CellBlock,
        keys: (u128, u128),
        cells: (CellId, u32),
        nodes: (u32, u32),
        codec: Codec,
        effort: i32,
    ) -> io::Result<()> {
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(block)
            .map_err(|why| io::Error::other(format!("a block will not serialize: {why}")))?;
        let stored = codec.encode(&bytes, effort);
        self.out.write_all(&stored)?;

        self.entries.push(BlockEntry {
            first_key: keys.0,
            last_key: keys.1,
            level: u8::try_from(block.level()).expect("more levels than a byte counts"),
            codec: codec.id(),
            first_cell: cells.0,
            cells: cells.1,
            first_node: nodes.0,
            nodes: nodes.1,
            at: self.at,
            stored: u32::try_from(stored.len())
                .expect("a block of more than four thousand million"),
            unpacked: u32::try_from(bytes.len())
                .expect("a block of more than four thousand million"),
            hash: xxhash_rust::xxh3::xxh3_64(&bytes),
        });
        self.at += stored.len() as u64;
        Ok(())
    }

    /// Closes the file and hands back the map of what went into it.
    ///
    /// # Errors
    ///
    /// Returns what went wrong flushing.
    pub fn finish(mut self) -> io::Result<BlockMap> {
        self.out.flush()?;
        Ok(BlockMap::of(self.entries))
    }
}

/// A store open for reading.
pub struct BlockStore {
    blocks: File,
    map: BlockMap,
    tree: CellTree,
}

/// What went wrong reading a cell.
#[derive(Debug)]
pub enum NotRead {
    /// no block holds it, which means the piece of the map it is in was not
    /// downloaded rather than that anything is wrong
    NotHere,
    /// the file names a codec this build has no reader for
    UnknownCodec(u8),
    /// the bytes are not what they say they are
    Corrupt(String),
    Io(io::Error),
}

impl std::fmt::Display for NotRead {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotHere => write!(f, "no block of this store holds that cell"),
            Self::UnknownCodec(id) => write!(f, "no codec numbered {id}"),
            Self::Corrupt(why) => write!(f, "a block did not read back: {why}"),
            Self::Io(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for NotRead {}

impl From<io::Error> for NotRead {
    fn from(why: io::Error) -> Self {
        Self::Io(why)
    }
}

impl BlockStore {
    /// # Errors
    ///
    /// Returns what went wrong opening the file.
    pub fn open(blocks: &Path, map: BlockMap, tree: CellTree) -> io::Result<Self> {
        Ok(Self {
            blocks: File::open(blocks)?,
            map,
            tree,
        })
    }

    #[must_use]
    pub fn map(&self) -> &BlockMap {
        &self.map
    }

    #[must_use]
    pub fn tree(&self) -> &CellTree {
        &self.tree
    }

    /// A whole cell: its table, and which node each place of it is.
    ///
    /// The nodes are worked out rather than read. A place is an offset into
    /// the cell's run of numbers, which the tree says where to find, and on
    /// the finest level the border nodes are the front of the run so the
    /// offsets are nought upward and are not stored at all.
    ///
    /// # Errors
    ///
    /// [`NotRead::NotHere`] where no block holds the cell.
    pub fn cell_into(
        &self,
        level: usize,
        cell: CellId,
        table: &mut Vec<u32>,
        nodes: &mut Vec<u32>,
    ) -> Result<(), NotRead> {
        let entry = *self.map.holding_cell(level, cell).ok_or(NotRead::NotHere)?;
        let block = self.block_at(&entry)?;
        let widths = self.widths_of(&entry, level);
        let which = (cell - entry.first_cell) as usize;
        block.unpack_into(which, &widths, table);

        let begins = self.tree.nodes_begin(level, cell);
        block.places_into(which, &widths, nodes);
        if nodes.is_empty() {
            // the border nodes lead the run, so the places are nought upward
            nodes.extend((0..widths[which] as u32).map(|at| begins + at));
        } else {
            for node in nodes.iter_mut() {
                *node += begins;
            }
        }
        Ok(())
    }

    /// The entry naming the block a cell is in, and nothing where no block
    /// holds it.
    #[must_use]
    pub fn entry_of(&self, level: usize, cell: CellId) -> Option<BlockEntry> {
        self.map.holding_cell(level, cell).copied()
    }

    /// Reads and decodes the block an entry names.
    ///
    /// # Errors
    ///
    /// What the codec or the file said.
    pub fn block_at(&self, entry: &crate::block_map::BlockEntry) -> Result<CellBlock, NotRead> {
        let codec = Codec::of(entry.codec).map_err(|why| NotRead::UnknownCodec(why.0))?;
        let mut stored = vec![0_u8; entry.stored as usize];
        read_at(&self.blocks, entry.at, &mut stored)?;
        let bytes = codec
            .decode(&stored, entry.unpacked as usize)
            .map_err(NotRead::Corrupt)?;
        rkyv::from_bytes::<CellBlock, rkyv::rancor::Error>(&bytes)
            .map_err(|why| NotRead::Corrupt(why.to_string()))
    }

    /// How wide each table of a block is, which the tree knows.
    #[must_use]
    pub fn widths_of(&self, entry: &BlockEntry, level: usize) -> Vec<usize> {
        (0..entry.cells)
            .map(|at| self.tree.facts(level, entry.first_cell + at).on_border as usize)
            .collect()
    }

    /// The distances across one cell, read out of whichever block holds it.
    ///
    /// Into a buffer the caller keeps, since a search asks for one cell after
    /// another and each answer is the same shape.
    ///
    /// # Errors
    ///
    /// [`NotRead::NotHere`] where no block holds the cell, which is a region
    /// nobody downloaded rather than a fault.
    pub fn table_into(
        &self,
        level: usize,
        cell: CellId,
        out: &mut Vec<u32>,
    ) -> Result<(), NotRead> {
        let entry = *self.map.holding_cell(level, cell).ok_or(NotRead::NotHere)?;
        let block = self.block_at(&entry)?;
        let widths = self.widths_of(&entry, level);
        block.unpack_into((cell - entry.first_cell) as usize, &widths, out);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cell_block::CellEntry;

    fn tiny_tree() -> CellTree {
        use crate::{
            edge::InputEdge, geometry::FPCoordinate, grid_graph::grid_directory,
            packed_partition::PackedPartition, static_graph::StaticGraph,
        };
        let side = 8;
        let mut edges = Vec::new();
        for row in 0..side {
            for column in 0..side {
                let node = row * side + column;
                if column + 1 < side {
                    edges.push(InputEdge::new(node, node + 1, 1_u32));
                    edges.push(InputEdge::new(node + 1, node, 1_u32));
                }
                if row + 1 < side {
                    edges.push(InputEdge::new(node, node + side, 1_u32));
                    edges.push(InputEdge::new(node + side, node, 1_u32));
                }
            }
        }
        let graph = StaticGraph::new(edges);
        let directory = grid_directory(side);
        let partition = PackedPartition::of(&directory);
        let coordinates = vec![FPCoordinate::new(0, 0); side * side];
        CellTree::of(&directory, &partition, &graph, &coordinates)
    }

    #[test]
    fn a_table_written_to_a_store_reads_back_out_of_it() {
        let tree = tiny_tree();
        let held = tempfile::tempdir().expect("a directory to write in");
        let path = held.path().join("blocks");

        // two blocks of the finest level, each of two cells, with tables as
        // wide as the tree says those cells are
        let mut matrices = Vec::new();
        for cell in 0..4_u32 {
            let wide = tree.facts(0, cell).on_border as usize;
            matrices.push(
                (0..wide * wide)
                    .map(|at| (at as u32 + cell) * 3)
                    .collect::<Vec<u32>>(),
            );
        }

        let mut writer = BlockWriter::create(&path).expect("a file to write");
        for (which, codec) in [(0_u32, Codec::Stored), (2, Codec::Lz4)] {
            let entries = (which..which + 2)
                .map(|cell| CellEntry {
                    matrix: &matrices[cell as usize],
                    wide: tree.facts(0, cell).on_border as usize,
                    places: &[],
                    holds: 16,
                })
                .collect::<Vec<_>>();
            let block = CellBlock::of(0, which, &entries, true);
            let keys = (tree.range_of(0, which).0, tree.range_of(0, which + 1).1);
            writer
                .push(&block, keys, (which, 2), (which * 4, 8), codec, 3)
                .expect("a block to write");
        }
        let map = writer.finish().expect("a file to close");
        assert_eq!(map.len(), 2);

        let store = BlockStore::open(&path, map, tree).expect("a store to open");
        let mut out = Vec::new();
        for cell in 0..4_u32 {
            store.table_into(0, cell, &mut out).expect("a cell to read");
            assert_eq!(out, matrices[cell as usize], "cell {cell}");
        }
    }

    #[test]
    fn a_cell_no_block_holds_says_so_rather_than_failing() {
        let tree = tiny_tree();
        let held = tempfile::tempdir().expect("a directory to write in");
        let path = held.path().join("blocks");
        let writer = BlockWriter::create(&path).expect("a file to write");
        let map = writer.finish().expect("a file to close");

        let store = BlockStore::open(&path, map, tree).expect("a store to open");
        let mut out = Vec::new();
        let why = store
            .table_into(0, 0, &mut out)
            .expect_err("nothing is there");
        assert!(matches!(why, NotRead::NotHere), "{why}");
        assert_eq!(why.to_string(), "no block of this store holds that cell");
    }
}
