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
    /// where it holds nothing, or another partition's run, the search is done.
    static RECENT: Cell<Option<(usize, u32, u32, u128)>> = const { Cell::new(None) };
}

/// Writes a sorted run of entries down in Eytzinger order.
///
/// Walked in order, `k`, `2k`, `2k + 1` visits the entries smallest first, so
/// filling them in that order puts each where a search will look for it.
fn lay_out(
    k: usize,
    runs: usize,
    at: &mut usize,
    from: &[(u32, u32, u128)],
    begins: &mut [u32],
    ends: &mut [u32],
    words: &mut [u128],
) {
    if k > runs {
        return;
    }
    lay_out(2 * k, runs, at, from, begins, ends, words);
    let (first, upto, word) = from[*at];
    begins[k] = first;
    ends[k] = upto;
    words[k] = word;
    *at += 1;
    lay_out(2 * k + 1, runs, at, from, begins, ends, words);
}

/// The cell each node sits in on every level.
///
/// # One entry a cell, not a word a node
///
/// Every node of a cell of the finest level is in that cell, and so in its
/// parent, and in its parent's parent: the whole word is the same for all of
/// them. The nodes were renumbered so that a cell's nodes are a run, which is
/// what let a cell table be found by a range and a block of arcs be keyed by
/// one. It does the same thing here: the words come in runs, and a run is
/// worth storing once.
///
/// On a continent that is six hundred thousand runs against eighteen million
/// nodes. A partition that took a hundred and sixty three mebibytes takes
/// fourteen.
///
/// Where the numbering is *not* in cell order this still holds -- a run is
/// ended wherever the word changes, and in the worst case there is one a node,
/// which is what the old layout was. It costs correctness nothing and saves
/// nothing.
///
/// # Why the runs are laid out the way they are
///
/// A node is turned back into its run by looking for the last run that begins
/// at or before it, which is a binary search. Laid out in order, that search
/// touches a different cache line at every step and the last few steps are the
/// only ones that were ever going to be near each other.
///
/// So they are laid out in Eytzinger order instead: the array is the binary
/// search tree written down breadth first, so the root is first, its two
/// children next to each other after it, their four children after those. The
/// steps a search takes are `k`, `2k`, `4k` -- reads that walk forward through
/// memory rather than halving their way across it, and the first several steps
/// share a cache line.
///
/// # And most searches do not happen
///
/// A search settles a node and then another, and the nodes of a cell are a
/// run, so the next node is very often in the run just used. Each thread keeps
/// the last run it wanted, which turns the common case into two comparisons.
pub struct PackedPartition {
    /// where each run begins and ends, and the word every node of it has, all
    /// three in Eytzinger order and all three one based: entry zero is unused
    /// so that the children of `k` are `2k` and `2k + 1`
    begins: Vec<u32>,
    ends: Vec<u32>,
    words: Vec<u128>,
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

        let mut sorted: Vec<(u32, u32, u128)> = Vec::new();
        for node in 0..nodes {
            let word = word_of(node);
            let at = u32::try_from(node).expect("a node in four bytes");
            match sorted.last_mut() {
                Some((_, upto, held)) if *held == word => *upto = at + 1,
                _ => sorted.push((at, at + 1, word)),
            }
        }

        let runs = sorted.len();
        let mut begins = vec![0_u32; runs + 1];
        let mut ends = vec![0_u32; runs + 1];
        let mut words = vec![0_u128; runs + 1];
        let mut placed = 0;
        lay_out(
            1,
            runs,
            &mut placed,
            &sorted,
            &mut begins,
            &mut ends,
            &mut words,
        );
        debug_assert_eq!(placed, runs, "a run was not laid out");
        drop(sorted);

        Self {
            begins,
            ends,
            words,
            runs,
            nodes,
            width: at.max(1),
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
            + self.ends.capacity() * size_of::<u32>()
            + self.words.capacity() * size_of::<u128>()
            + self.level_of_bit.capacity()
            + self.begins_at.capacity() * size_of::<u32>()
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

        // Eytzinger: the tree is walked from its root, going right where a run
        // begins at or before the node and left where it begins after, and the
        // last one gone right at is the run wanted
        let mut k = 1;
        let mut found = 0;
        while k <= self.runs {
            if self.begins[k] <= node {
                found = k;
                k = 2 * k + 1;
            } else {
                k *= 2;
            }
        }
        assert!(found > 0, "no run holds node {node}");
        RECENT.set(Some((
            self.which,
            self.begins[found],
            self.ends[found],
            self.words[found],
        )));
        self.words[found]
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
