//! Which cell a node sits in on every level, in one word.
//!
//! # Why one word
//!
//! A search over the cells asks, for every node it settles, the highest level
//! whose cell holds neither end. Asked of a [`LevelDirectory`] that is a walk
//! of the parents; asked of a cell array per level it is one read into a
//! different array of the whole graph for each level, and on a continent each
//! of those arrays is seventy odd megabytes. The read that matters is a miss
//! either way, but six arrays cost six misses where one costs one, and they
//! cost six times the room to keep warm.
//!
//! Packed, the cells of every level a node sits in are one load. A continent
//! of six levels needs seventy six bits for them, so the word is sixteen bytes
//! rather than eight: still one cache line, and less in all than the arrays it
//! replaces.
//!
//! # What the packing buys beyond the load
//!
//! The levels are laid out finest first, so a coarser cell sits in the higher
//! bits. Cells nest, which means two nodes sharing a cell on some level share
//! one on every level above it, and so the levels on which two nodes differ
//! are exactly the levels below the highest one they differ on. That makes the
//! whole question one of bits: the cells of two nodes differ on a level while
//! the word holds a bit set in their difference at or above where that level
//! begins.
//!
//! So the highest level whose cells differ is read straight off the top bit of
//! the difference of the two words, and whether two nodes share a cell on a
//! level is one shift and one comparison. Neither asks what the cell ids
//! actually are. This is what OSRM's `MultiLevelPartition` does with a
//! `GetHighestDifferentLevel` over sixty four bits.

use std::{
    cell::Cell,
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::{
    graph::NodeID,
    level_directory::{CellId, LevelDirectory},
};

/// How many bits a word has to spend, and the most a partition may ask for.
const BITS: u32 = 128;

/// Tells one partition from another, so a thread's memo of the last run it
/// wanted is not offered to a partition it did not come from.
static NEXT_PARTITION: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// The last run this thread wanted, and whose partition it belongs to.
    ///
    /// A search settles a node and then another, and the nodes of a cell are a
    /// run, so the two are usually in the same one. This is only a shortcut:
    /// where it holds nothing, or another partition's run, the run is looked
    /// up in the ordinary way.
    static RECENT: Cell<Option<(usize, u32, u32, u128)>> = const { Cell::new(None) };
}

/// Room past the last word, so a read of a whole word from the last one does
/// not run off the end and does not have to be checked for.
const SPILL: usize = 32;

/// The narrowest and widest a bucket may be, as a power of two nodes.
///
/// Narrow buckets are more of them; wide ones are a longer walk. Between these
/// the width is chosen from the graph, so that a bucket holds about one run.
const NARROWEST: u32 = 4;
const WIDEST: u32 = 16;

/// Puts a word down at a bit offset.
fn write_word(packed: &mut [u8], at: u64, width: u32, value: u128) {
    let byte = (at / 8) as usize;
    let shift = (at % 8) as u32;
    let mask = if width >= BITS {
        u128::MAX
    } else {
        (1_u128 << width) - 1
    };
    let value = value & mask;

    // a word straddles at most seventeen bytes, which two overlapping sixteen
    // byte reads cover
    let low = u128::from_le_bytes(packed[byte..byte + 16].try_into().expect("sixteen bytes"));
    packed[byte..byte + 16].copy_from_slice(&(low | (value << shift)).to_le_bytes());
    if shift > 0 {
        let over = value >> (BITS - shift);
        if over != 0 {
            let high = u128::from_le_bytes(
                packed[byte + 16..byte + 32]
                    .try_into()
                    .expect("sixteen bytes"),
            );
            packed[byte + 16..byte + 32].copy_from_slice(&(high | over).to_le_bytes());
        }
    }
}

/// Picks a word back up from a bit offset.
#[inline]
fn read_word(packed: &[u8], at: u64, width: u32) -> u128 {
    let byte = (at / 8) as usize;
    let shift = (at % 8) as u32;
    let low = u128::from_le_bytes(packed[byte..byte + 16].try_into().expect("sixteen bytes"));
    let mut held = low >> shift;
    if shift > 0 {
        let high = u128::from_le_bytes(
            packed[byte + 16..byte + 32]
                .try_into()
                .expect("sixteen bytes"),
        );
        held |= high << (BITS - shift);
    }
    if width >= BITS {
        held
    } else {
        held & ((1_u128 << width) - 1)
    }
}

/// The cell each node sits in on every level.
///
/// # One entry a cell, not a word a node
///
/// Every node of a cell of the finest level is in that cell, and so in its
/// parent, and in its parent's parent: the whole word is the same for all of
/// them. The nodes were renumbered so that a cell's nodes are a run, which is
/// what let a cell table be found by a range and a block of arcs be keyed by
/// one. It does the same thing here, and the words come in runs worth storing
/// once. On a continent that is five hundred thousand runs against eighteen
/// million nodes.
///
/// Where the numbering is *not* in cell order this still holds -- a run is
/// ended wherever the word changes, and at worst there is one a node, which is
/// what a word a node was. It costs correctness nothing and saves nothing.
///
/// # A word is as wide as its levels ask for
///
/// A word is handed out as a `u128`, because that is what the shifting and the
/// exclusive or want. It is stored in the bits the levels actually ask for,
/// which on a continent cut into six is seventy six of the hundred and twenty
/// eight.
///
/// # Finding the run without searching for it
///
/// A node is turned back into its run by finding the last run that begins at
/// or before it. That is a binary search, and there are two ways to make one
/// fast: lay the array out so the search is cache friendly, or arrange not to
/// search.
///
/// This does the second. Alongside the runs is one entry for every block of
/// `1 << shift` nodes, saying which run was current where that block began,
/// with the width picked so a block holds about one run. A lookup reads its
/// block's entry and walks forward over the runs that began inside it, which
/// is usually none and rarely more than a few, and those runs are next to each
/// other in memory.
///
/// So it is two reads and a short forward walk, against the twenty dependent
/// reads a binary search over five hundred thousand runs takes however they
/// are laid out. An Eytzinger layout makes those twenty reads walk forward
/// instead of halving across memory, which is a real improvement on a search
/// and no improvement at all on not searching.
///
/// # And most lookups do not reach any of that
///
/// A search settles a node and then another, and the nodes of a cell are a
/// run, so the next node is very often in the run just used. Each thread keeps
/// the last run it wanted, which turns the common case into two comparisons.
pub struct PackedPartition {
    /// where each run begins, in order, with one past the last node on the end
    /// so that a run is `begins[r]..begins[r + 1]`
    begins: Vec<u32>,
    /// the word of each run, laid end to end at `width` bits apiece
    words: Vec<u8>,
    /// which run was current where each block of `1 << shift` nodes began
    buckets: Vec<u32>,
    /// how wide a block of nodes a bucket stands for, as a power of two
    shift: u32,
    /// how many runs there are
    runs: usize,
    nodes: usize,
    /// how many bits of a word the levels asked for
    width: u32,
    /// which partition this is, for the thread's memo
    which: usize,
    /// where the cell id of each level begins in the word, and one past the
    /// end of the topmost level in the last entry
    begins_at: Vec<u32>,
    /// which level the bit in each place belongs to, so the top bit of a
    /// difference names a level without a search
    level_of_bit: Vec<u8>,
    levels: usize,
}

impl PackedPartition {
    /// Packs a directory, one word per node.
    ///
    /// # Panics
    ///
    /// Panics if the cells of the partition do not fit in a word, which takes
    /// a directory far finer than one a road network is cut into: a continent
    /// of six levels spends seventy six bits of the hundred and twenty eight.
    #[must_use]
    pub fn of(directory: &LevelDirectory) -> Self {
        let levels = directory.levels();
        let nodes = directory.number_of_nodes();

        // a level is given room for the largest cell id it holds, and the
        // levels are laid down finest first so that a coarser cell sits above
        // a finer one
        let mut begins_at = Vec::with_capacity(levels + 1);
        let mut at = 0_u32;
        for level in 0..levels {
            begins_at.push(at);
            at += bits_for(directory.cells_on_level(level));
        }
        begins_at.push(at);
        assert!(
            at <= BITS,
            "the cells of {levels} levels want {at} bits and a word holds {BITS}"
        );

        let mut level_of_bit = vec![0_u8; BITS as usize];
        for level in 0..levels {
            for bit in begins_at[level]..begins_at[level + 1] {
                level_of_bit[bit as usize] =
                    u8::try_from(level).expect("a partition of more levels than a byte counts");
            }
        }
        // a bit above the topmost level can only be set by a difference of
        // nothing, which is never asked about, but it names the topmost level
        // rather than a level there is not
        for bit in at..BITS {
            level_of_bit[bit as usize] = u8::try_from(levels.saturating_sub(1)).unwrap_or(u8::MAX);
        }

        // A word is built a node at a time, walking the parents up from the
        // cell it is in at the finest level, and kept only where it differs
        // from the one before: every node of a cell has the same word, so what
        // this writes down is a run a cell rather than a word a node.
        let parents: Vec<&[CellId]> = (0..levels.saturating_sub(1))
            .map(|level| directory.parents_on_level(level))
            .collect();

        let word_of = |node: NodeID| -> u128 {
            let mut cell = directory.cell_of(node, 0);
            let mut word = u128::from(cell) << begins_at[0];
            for (level, &begins) in begins_at.iter().enumerate().take(levels).skip(1) {
                cell = parents[level - 1][cell as usize];
                word |= u128::from(cell) << begins;
            }
            word
        };

        // the runs, in order, and the word each of them has
        let mut begins: Vec<u32> = Vec::new();
        let mut of_run: Vec<u128> = Vec::new();
        let mut last: Option<u128> = None;
        for node in 0..nodes {
            let word = word_of(node);
            if last != Some(word) {
                begins.push(u32::try_from(node).expect("a node in four bytes"));
                of_run.push(word);
                last = Some(word);
            }
        }
        let runs = of_run.len();
        // one past the last node, so a run is always a span between two entries
        begins.push(u32::try_from(nodes).expect("a graph in four bytes"));

        let width = at.max(1);
        let mut words = vec![0_u8; (runs as u64 * u64::from(width)).div_ceil(8) as usize + SPILL];
        for (run, &word) in of_run.iter().enumerate() {
            write_word(&mut words, run as u64 * u64::from(width), width, word);
        }
        drop(of_run);

        // A bucket for about every run: narrower is more of them and wider is
        // a longer walk, and one run apiece is where the two meet.
        let shift = nodes.checked_div(runs).map_or(NARROWEST, |apiece| {
            apiece
                .next_power_of_two()
                .trailing_zeros()
                .clamp(NARROWEST, WIDEST)
        });
        let mut buckets = vec![0_u32; (nodes >> shift) + 2];
        let mut run = 0_usize;
        for (bucket, held) in buckets.iter_mut().enumerate() {
            let first = (bucket << shift) as u32;
            // the last run that had begun by the time this block of nodes did
            while run + 1 < runs && begins[run + 1] <= first {
                run += 1;
            }
            *held = u32::try_from(run).expect("a run count in four bytes");
        }

        Self {
            begins,
            words,
            buckets,
            shift,
            runs,
            nodes,
            width,
            which: NEXT_PARTITION.fetch_add(1, Ordering::Relaxed),
            begins_at,
            level_of_bit,
            levels,
        }
    }

    /// how many levels the partition has
    #[must_use]
    pub const fn levels(&self) -> usize {
        self.levels
    }

    /// Where each level's cell id begins in a word, finest level first, with
    /// the width of a whole word in the last entry.
    ///
    /// This is the layout of a key, and a format that stores keys has to store
    /// it beside them: how many bits a level was given depends on how many
    /// cells it turned out to have.
    #[must_use]
    pub fn level_layout(&self) -> &[u32] {
        &self.begins_at
    }

    /// how many nodes it was built over
    #[must_use]
    pub fn number_of_nodes(&self) -> usize {
        self.nodes
    }

    /// What the partition takes, which is the bits the levels asked for and
    /// not a word a node.
    #[must_use]
    pub fn bytes(&self) -> usize {
        self.begins.capacity() * size_of::<u32>()
            + self.words.capacity()
            + self.buckets.capacity() * size_of::<u32>()
            + self.level_of_bit.capacity()
            + self.begins_at.capacity() * size_of::<u32>()
    }

    /// How wide a block of nodes a bucket of the index stands for.
    #[must_use]
    pub fn bucket_shift(&self) -> u32 {
        self.shift
    }

    /// How many bits a word is stored in.
    #[must_use]
    pub fn width(&self) -> u32 {
        self.width
    }

    /// The cells of every level this node sits in, as the one word a caller
    /// reads once and then asks questions of.
    ///
    /// # Panics
    ///
    /// Panics for a node the partition was not built over.
    #[must_use]
    #[inline]
    pub fn word(&self, node: NodeID) -> u128 {
        assert!(node < self.nodes, "no node {node} in the partition");
        let node = node as u32;

        // the run this thread wanted last, where the node is in it too
        if let Some((which, first, upto, word)) = RECENT.get()
            && which == self.which
            && node >= first
            && node < upto
        {
            return word;
        }

        // otherwise the bucket this node's block of nodes falls in, and then
        // forward over whichever runs began inside that block
        let mut run = self.buckets[(node >> self.shift) as usize] as usize;
        while run + 1 < self.runs && self.begins[run + 1] <= node {
            run += 1;
        }
        let word = read_word(&self.words, run as u64 * u64::from(self.width), self.width);
        RECENT.set(Some((
            self.which,
            self.begins[run],
            self.begins[run + 1],
            word,
        )));
        word
    }

    /// How many runs the partition came to.
    ///
    /// One a cell of the finest level where the nodes are numbered in cell
    /// order, and one a node at worst.
    #[must_use]
    pub fn runs(&self) -> usize {
        self.runs
    }

    /// The cell a node sits in on a level.
    ///
    /// # Panics
    ///
    /// Panics for a node or a level the partition does not have.
    #[must_use]
    #[inline]
    pub fn cell_of(&self, node: NodeID, level: usize) -> CellId {
        self.cell_in(self.word(node), level)
    }

    /// The same, of a word already in hand.
    ///
    /// # Panics
    ///
    /// Panics for a level the partition does not have.
    #[must_use]
    #[inline]
    pub fn cell_in(&self, word: u128, level: usize) -> CellId {
        let begins = self.begins_at[level];
        let width = self.begins_at[level + 1] - begins;
        let held = (word >> begins) & ((1_u128 << width) - 1);
        CellId::try_from(held).expect("a cell id wider than the level it was given room for")
    }

    /// Whether two words hold the same cell on a level.
    ///
    /// Cells nest, so sharing a cell on this level is sharing one on every
    /// level above it too, which is to say the two words agree from where this
    /// level begins upwards.
    #[must_use]
    #[inline]
    pub fn same_cell_at(&self, first: u128, second: u128, level: usize) -> bool {
        (first ^ second) >> self.begins_at[level] == 0
    }

    /// The highest level whose cells the two words differ on, and `None` when
    /// they sit in the same cell on every level.
    #[must_use]
    #[inline]
    pub fn highest_different_level(&self, first: u128, second: u128) -> Option<usize> {
        let apart = first ^ second;
        if apart == 0 {
            return None;
        }
        // the top bit of the difference is the coarsest level they part on,
        // and every level below it they part on too
        let top = BITS - 1 - apart.leading_zeros();
        Some(self.level_of_bit[top as usize] as usize)
    }

    /// The highest level whose cell around this node holds neither end, and
    /// `None` when even the finest one does.
    ///
    /// The levels on which the node differs from an end run from the finest up
    /// to the highest they differ on, so the levels on which it differs from
    /// both ends run up to the lower of the two.
    #[must_use]
    #[inline]
    pub fn query_level(&self, source: u128, target: u128, node: NodeID) -> Option<usize> {
        let word = self.word(node);
        // `None` orders below every `Some`, which is what is wanted: a node
        // sharing a cell with an end everywhere may step over nothing
        self.highest_different_level(source, word)
            .min(self.highest_different_level(target, word))
    }
}

/// How many bits the ids of a level need, and one for a level of a single cell
/// so that every level has a place of its own.
fn bits_for(cells: usize) -> u32 {
    match cells {
        0 | 1 => 1,
        _ => (cells - 1).ilog2() + 1,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{grid_graph::grid_directory, level_directory::LevelDirectory};

    /// The point of the runs: a node of a cell has the word every other node
    /// of that cell has, so the partition is a run a cell and not a word a
    /// node.
    #[test]
    fn a_partition_is_a_run_a_cell_and_not_a_word_a_node() {
        let directory = grid_directory(64);
        let packed = PackedPartition::of(&directory);
        let nodes = directory.number_of_nodes();

        assert!(
            packed.bucket_shift() >= NARROWEST && packed.bucket_shift() <= WIDEST,
            "the bucket width was not chosen from the graph"
        );
        assert!(
            packed.runs() < nodes,
            "{} runs against {nodes} nodes",
            packed.runs()
        );
        let whole = nodes * size_of::<u128>();
        assert!(
            packed.bytes() < whole,
            "{} bytes against {whole} at a word a node",
            packed.bytes()
        );

        // and every node still reads back the word its own cells make
        for node in 0..nodes {
            for level in 0..directory.levels() {
                assert_eq!(
                    packed.cell_of(node, level),
                    directory.cell_of(node, level),
                    "node {node} on level {level}"
                );
            }
        }
    }

    /// The search has to find the run whatever order the words come in, and a
    /// numbering that is not in cell order is the case that makes runs of one.
    #[test]
    fn every_node_finds_its_run_however_the_words_run() {
        for side in [4_usize, 8, 16, 32] {
            let directory = grid_directory(side);
            let packed = PackedPartition::of(&directory);
            let nodes = directory.number_of_nodes();
            assert!(packed.runs() >= 1);

            // asked out of order and twice over, so the thread's memo is both
            // used and missed rather than only ever walked forward through
            for node in (0..nodes).rev().chain(0..nodes) {
                assert_eq!(
                    packed.word(node),
                    packed
                        .level_layout()
                        .iter()
                        .take(directory.levels())
                        .enumerate()
                        .fold(0_u128, |word, (level, &begins)| word
                            | (u128::from(directory.cell_of(node, level)) << begins)),
                    "node {node} of a grid of {side}"
                );
            }
        }
    }

    #[test]
    fn a_level_is_given_room_for_the_ids_it_holds() {
        assert_eq!(bits_for(0), 1);
        assert_eq!(bits_for(1), 1);
        assert_eq!(bits_for(2), 1);
        assert_eq!(bits_for(3), 2);
        assert_eq!(bits_for(4), 2);
        assert_eq!(bits_for(5), 3);
        assert_eq!(bits_for(256), 8);
        assert_eq!(bits_for(257), 9);
    }

    /// The packing has to hand back what the directory was asked, for every
    /// node and every level, or nothing built on it means anything.
    #[test]
    fn every_cell_reads_back_what_the_directory_says() {
        for side in [4_usize, 8, 16, 64] {
            let directory = grid_directory(side);
            let packed = PackedPartition::of(&directory);
            assert_eq!(packed.levels(), directory.levels());
            assert_eq!(packed.number_of_nodes(), directory.number_of_nodes());
            for node in 0..directory.number_of_nodes() {
                for level in 0..directory.levels() {
                    assert_eq!(
                        packed.cell_of(node, level),
                        directory.cell_of(node, level),
                        "side {side}, node {node}, level {level}"
                    );
                }
            }
        }
    }

    /// The whole point of the packing, held against the walk it replaces.
    #[test]
    fn the_highest_level_two_nodes_part_on_is_the_one_the_directory_gives() {
        let side = 32;
        let directory = grid_directory(side);
        let packed = PackedPartition::of(&directory);
        let count = directory.number_of_nodes();

        for first in (0..count).step_by(37) {
            for second in (0..count).step_by(53) {
                let by_walk = (0..directory.levels()).rev().find(|&level| {
                    directory.cell_of(first, level) != directory.cell_of(second, level)
                });
                let by_bits =
                    packed.highest_different_level(packed.word(first), packed.word(second));
                assert_eq!(by_bits, by_walk, "{first} against {second}");
            }
        }
    }

    /// The rule the query runs on, held against the walk it replaces.
    #[test]
    fn the_query_level_is_the_one_the_walk_finds() {
        let side = 32;
        let directory = grid_directory(side);
        let packed = PackedPartition::of(&directory);
        let count = directory.number_of_nodes();

        for (source, target) in [(0, count - 1), (7, count / 2), (count / 3, 11), (5, 5)] {
            let source_word = packed.word(source);
            let target_word = packed.word(target);
            for node in (0..count).step_by(11) {
                let by_walk = (0..directory.levels()).rev().find(|&level| {
                    let cell = directory.cell_of(node, level);
                    cell != directory.cell_of(source, level)
                        && cell != directory.cell_of(target, level)
                });
                assert_eq!(
                    packed.query_level(source_word, target_word, node),
                    by_walk,
                    "{source} to {target}, node {node}"
                );
            }
        }
    }

    #[test]
    fn a_node_shares_every_cell_with_itself() {
        let directory = grid_directory(8);
        let packed = PackedPartition::of(&directory);
        for node in 0..directory.number_of_nodes() {
            let word = packed.word(node);
            assert_eq!(packed.highest_different_level(word, word), None);
            for level in 0..directory.levels() {
                assert!(packed.same_cell_at(word, word, level));
            }
        }
    }

    /// Sharing a cell on a level is sharing one on every level above it, which
    /// is the nesting the bit test leans on.
    #[test]
    fn sharing_a_cell_is_sharing_every_coarser_one() {
        let directory = grid_directory(16);
        let packed = PackedPartition::of(&directory);
        let count = directory.number_of_nodes();
        for first in (0..count).step_by(13) {
            for second in (0..count).step_by(7) {
                let (a, b) = (packed.word(first), packed.word(second));
                for level in 0..directory.levels() {
                    assert_eq!(
                        packed.same_cell_at(a, b, level),
                        directory.cell_of(first, level) == directory.cell_of(second, level),
                        "{first} against {second} on level {level}"
                    );
                }
            }
        }
    }

    /// A directory whose levels are wide enough to need most of a word still
    /// reads back, which is what says the packing is not quietly capped at the
    /// width of the levels a grid happens to have.
    #[test]
    fn wide_levels_are_packed_and_read_back() {
        // three levels of a thousand nodes: 1000, 100 and 10 cells, which is
        // ten, seven and four bits
        let nodes = 1000;
        let base = (0..nodes).map(|node| node as CellId).collect::<Vec<_>>();
        let first = (0..nodes)
            .map(|cell| (cell / 10) as CellId)
            .collect::<Vec<_>>();
        let second = (0..100)
            .map(|cell| (cell / 10) as CellId)
            .collect::<Vec<_>>();
        let directory = LevelDirectory::new(base, vec![first, second]);

        let packed = PackedPartition::of(&directory);
        for node in 0..nodes {
            for level in 0..directory.levels() {
                assert_eq!(packed.cell_of(node, level), directory.cell_of(node, level));
            }
        }
    }
}
