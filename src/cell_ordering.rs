//! A numbering of the cells that runs the way their keys do.
//!
//! # Why the numbers matter
//!
//! A block of a store holds a run of cells, and the store finds it by the keys
//! it covers. Those two only agree if a cell's number and a cell's key run the
//! same way, and out of the assembly they do not: a cell is numbered as the
//! merging happened to reach it, so a block covering one range of keys covers
//! a scattering of numbers and has to carry a list of which cells it holds.
//!
//! Numbered as the keys run, a block is a range of keys, a range of cell
//! numbers and a range of node numbers at once, and none of the three has to
//! be written down as anything but a first and a last.
//!
//! Two more things fall out, both of which a store would otherwise pay for:
//!
//! - **The children of a cell are a run.** Cells with the same parent share
//!   the whole of their key above them and differ below, so sorting by key
//!   puts them side by side. A cell's children become a first and a count.
//! - **The cells of a level are in the order their nodes are.** The nodes were
//!   numbered by cell path too, so walking the cells of a level in order walks
//!   the nodes of the graph in order.
//!
//! # It does not move the nodes
//!
//! A node's number comes from the order of the packed words, and a word is
//! built out of cell numbers, so renumbering cells rewrites every word. It
//! does not reorder them: the new numbers run the way the keys do and the keys
//! are what the old words were sorted by, so every word lands where it already
//! was. An instance whose nodes are numbered by cell path stays numbered.

use rkyv::{Archive, Deserialize, Serialize};

use crate::{
    level_directory::{CellId, LevelDirectory},
    packed_partition::PackedPartition,
};

/// Which number each cell of each level had, and has.
#[derive(Clone, Debug, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub struct CellOrdering {
    /// per level, the new number of the cell that had each old number
    to_new: Vec<Vec<CellId>>,
    /// per level, the old number of the cell that has each new number
    to_old: Vec<Vec<CellId>>,
}

impl CellOrdering {
    /// Works out the numbering of a directory.
    ///
    /// The key of a cell is the path from the root down to it, so the keys of
    /// a level are built out of the keys of the level above rather than walked
    /// up to one cell at a time.
    ///
    /// # Panics
    ///
    /// Panics if the directory and the partition are not over the same levels.
    #[must_use]
    pub fn of(directory: &LevelDirectory, partition: &PackedPartition) -> Self {
        let levels = directory.levels();
        assert_eq!(levels, partition.levels(), "another directory");
        let begins_at = partition.level_layout();

        // the key of every cell, from the top down: a cell's key is its
        // parent's with its own number laid into the bits of its level
        let mut keys: Vec<Vec<u128>> = vec![Vec::new(); levels];
        let top = levels - 1;
        keys[top] = (0..directory.cells_on_level(top) as u128)
            .map(|cell| cell << begins_at[top])
            .collect();
        for level in (0..top).rev() {
            let above = directory.parents_on_level(level);
            keys[level] = above
                .iter()
                .enumerate()
                .map(|(cell, &parent)| {
                    keys[level + 1][parent as usize] | ((cell as u128) << begins_at[level])
                })
                .collect();
        }

        let mut to_new = Vec::with_capacity(levels);
        let mut to_old = Vec::with_capacity(levels);
        for of_level in &keys {
            let count = of_level.len();
            let mut order = (0..count as CellId).collect::<Vec<_>>();
            order.sort_unstable_by_key(|&cell| of_level[cell as usize]);
            let mut places = vec![0 as CellId; count];
            for (place, &cell) in order.iter().enumerate() {
                places[cell as usize] =
                    CellId::try_from(place).expect("more cells than a cell id counts");
            }
            to_new.push(places);
            to_old.push(order);
        }
        Self { to_new, to_old }
    }

    #[must_use]
    pub fn levels(&self) -> usize {
        self.to_new.len()
    }

    #[must_use]
    pub fn cells_on_level(&self, level: usize) -> usize {
        self.to_new[level].len()
    }

    /// The number a cell has now.
    #[must_use]
    pub fn new_of(&self, level: usize, cell: CellId) -> CellId {
        self.to_new[level][cell as usize]
    }

    /// The number a cell had.
    #[must_use]
    pub fn old_of(&self, level: usize, cell: CellId) -> CellId {
        self.to_old[level][cell as usize]
    }

    /// The same directory with its cells renumbered.
    ///
    /// Which cell a node is in does not change, only what that cell is called,
    /// so nothing about the partition itself moves.
    #[must_use]
    pub fn renumber(&self, directory: &LevelDirectory) -> LevelDirectory {
        let levels = directory.levels();
        assert_eq!(levels, self.levels(), "another directory");

        let base = (0..directory.number_of_nodes())
            .map(|node| self.new_of(0, directory.cell_of(node, 0)))
            .collect();
        let parents = (0..levels - 1)
            .map(|level| {
                let above = directory.parents_on_level(level);
                // the cell that is now numbered `cell` was numbered
                // `old_of(cell)`, and its parent has moved too
                (0..above.len())
                    .map(|cell| {
                        let was = self.old_of(level, cell as CellId);
                        self.new_of(level + 1, above[was as usize])
                    })
                    .collect()
            })
            .collect();
        LevelDirectory::new(base, parents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::grid_graph::grid_directory;

    fn keys_of(directory: &LevelDirectory, level: usize) -> Vec<u128> {
        let partition = PackedPartition::of(directory);
        let begins_at = partition.level_layout();
        (0..directory.cells_on_level(level))
            .map(|cell| {
                // the path of a cell, walked up one level at a time
                let mut key = (cell as u128) << begins_at[level];
                let mut at = cell as CellId;
                for (above, &begins) in begins_at
                    .iter()
                    .enumerate()
                    .take(directory.levels())
                    .skip(level + 1)
                {
                    at = directory.parents_on_level(above - 1)[at as usize];
                    key |= (at as u128) << begins;
                }
                key
            })
            .collect()
    }

    #[test]
    fn the_cells_come_out_in_the_order_their_keys_run() {
        let directory = grid_directory(16);
        let ordering = CellOrdering::of(&directory, &PackedPartition::of(&directory));
        let moved = ordering.renumber(&directory);
        for level in 0..moved.levels() {
            let keys = keys_of(&moved, level);
            assert!(
                keys.windows(2).all(|pair| pair[0] < pair[1]),
                "level {level} is not in key order"
            );
        }
    }

    #[test]
    fn the_children_of_a_cell_are_a_run() {
        let directory = grid_directory(16);
        let ordering = CellOrdering::of(&directory, &PackedPartition::of(&directory));
        let moved = ordering.renumber(&directory);
        for level in 1..moved.levels() {
            let above = moved.parents_on_level(level - 1);
            // walking the children in order, the parent never goes back
            assert!(
                above.windows(2).all(|pair| pair[0] <= pair[1]),
                "level {level} has a parent out of order"
            );
        }
    }

    #[test]
    fn a_node_stays_in_the_cell_it_was_in() {
        let directory = grid_directory(16);
        let ordering = CellOrdering::of(&directory, &PackedPartition::of(&directory));
        let moved = ordering.renumber(&directory);
        for node in 0..directory.number_of_nodes() {
            for level in 0..directory.levels() {
                assert_eq!(
                    moved.cell_of(node, level),
                    ordering.new_of(level, directory.cell_of(node, level)),
                    "node {node} at level {level}"
                );
            }
        }
    }

    /// Two nodes that shared a cell still share one, and two that did not
    /// still do not. This is the whole of what a renumbering may not change.
    #[test]
    fn the_partition_itself_does_not_move() {
        let directory = grid_directory(8);
        let ordering = CellOrdering::of(&directory, &PackedPartition::of(&directory));
        let moved = ordering.renumber(&directory);
        for first in 0..directory.number_of_nodes() {
            for second in 0..directory.number_of_nodes() {
                assert_eq!(
                    directory.common_level(first, second),
                    moved.common_level(first, second),
                    "{first} and {second}"
                );
            }
        }
    }

    #[test]
    fn the_numbering_is_a_numbering() {
        let directory = grid_directory(16);
        let ordering = CellOrdering::of(&directory, &PackedPartition::of(&directory));
        for level in 0..ordering.levels() {
            let count = ordering.cells_on_level(level);
            for cell in 0..count as CellId {
                assert_eq!(ordering.old_of(level, ordering.new_of(level, cell)), cell);
                assert_eq!(ordering.new_of(level, ordering.old_of(level, cell)), cell);
            }
        }
    }

    #[test]
    fn an_ordering_reads_back_as_it_was_written() {
        let directory = grid_directory(8);
        let ordering = CellOrdering::of(&directory, &PackedPartition::of(&directory));
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&ordering).expect("serializes");
        let read: CellOrdering =
            rkyv::from_bytes::<CellOrdering, rkyv::rancor::Error>(&bytes).expect("deserializes");
        assert_eq!(read, ordering);
    }
}
