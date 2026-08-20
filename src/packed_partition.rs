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

use crate::{
    graph::NodeID,
    level_directory::{CellId, LevelDirectory},
};

/// How many bits a word has to spend, and the most a partition may ask for.
const BITS: u32 = 128;

/// The cell each node sits in on every level, one word apiece.
pub struct PackedPartition {
    of_node: Vec<u128>,
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

        // The cells of a level are read off the level below it rather than
        // asked of the directory per node, which would walk the parents of
        // every node once for every level above it.
        let mut of_node = vec![0_u128; nodes];
        let mut cell_of_node = (0..nodes)
            .map(|node| directory.cell_of(node, 0))
            .collect::<Vec<_>>();
        for (node, &cell) in cell_of_node.iter().enumerate() {
            of_node[node] |= u128::from(cell) << begins_at[0];
        }
        for (level, &begins) in begins_at.iter().enumerate().take(levels).skip(1) {
            let parents = directory.parents_on_level(level - 1);
            for cell in &mut cell_of_node {
                *cell = parents[*cell as usize];
            }
            for (node, &cell) in cell_of_node.iter().enumerate() {
                of_node[node] |= u128::from(cell) << begins;
            }
        }

        Self {
            of_node,
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

    /// how many nodes it was built over
    #[must_use]
    pub fn number_of_nodes(&self) -> usize {
        self.of_node.len()
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
        self.of_node[node]
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
