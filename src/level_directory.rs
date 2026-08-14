//! Which cell a node sits in on each level of a nested partition.
//!
//! The levels are nested: a cell of one level lies inside exactly one cell of
//! the level above it. Only the lowest level is therefore stored per node. Each
//! level above it is a table from the cells below to the cells above, which is
//! as long as that level has cells rather than as long as the graph has nodes.
//! A hierarchy of six levels over eighteen million nodes costs the eighteen
//! million entries of the lowest level plus a few hundred thousand for the rest.
//!
//! # Examples
//!
//! ```rust
//! use toolbox_rs::level_directory::LevelDirectory;
//!
//! //  level 1:      0        1
//! //               / \       |
//! //  level 0:    0   1      2
//! //             /|   |\     |
//! //  nodes:    0 1   2 3    4
//! let directory = LevelDirectory::new(vec![0, 0, 1, 1, 2], vec![vec![0, 0, 1]]);
//!
//! assert_eq!(directory.levels(), 2);
//! // node 3 sits in cell 1 down below, which lies in cell 0 above
//! assert_eq!(directory.cell_of(3, 0), 1);
//! assert_eq!(directory.cell_of(3, 1), 0);
//! ```
use crate::graph::NodeID;
use rkyv::{Archive, Deserialize, Serialize};

/// A cell of one level. Cells are numbered from zero per level, so an id only
/// means something together with the level it belongs to.
pub type CellId = u32;

/// The cells of a nested partition, level by level.
#[derive(Clone, Debug, Default, PartialEq, Eq, Archive, Serialize, Deserialize)]
pub struct LevelDirectory {
    /// the cell of each node on the lowest level
    base: Vec<CellId>,
    /// for each level above the lowest, the cell of the next level up that each
    /// of its cells belongs to
    parents: Vec<Vec<CellId>>,
}

impl LevelDirectory {
    /// # Panics
    ///
    /// Panics if a cell is said to lie in one that the level above it does not
    /// have, as every query would run past the end of a table.
    #[must_use]
    pub fn new(base: Vec<CellId>, parents: Vec<Vec<CellId>>) -> Self {
        let directory = Self { base, parents };
        assert!(directory.is_consistent(), "the levels do not nest");
        directory
    }

    /// Whether every cell lies in one that the level above it actually has.
    fn is_consistent(&self) -> bool {
        let mut below = self.base.iter().max().map_or(0, |cell| *cell as usize + 1);
        for parents in &self.parents {
            if parents.len() < below {
                return false;
            }
            below = parents.iter().max().map_or(0, |cell| *cell as usize + 1);
        }
        true
    }

    /// How many levels the hierarchy has, the lowest one included.
    #[must_use]
    pub fn levels(&self) -> usize {
        1 + self.parents.len()
    }

    #[must_use]
    pub fn number_of_nodes(&self) -> usize {
        self.base.len()
    }

    /// How many cells a level holds.
    #[must_use]
    pub fn cells_on_level(&self, level: usize) -> usize {
        if level == 0 {
            self.base.iter().max().map_or(0, |cell| *cell as usize + 1)
        } else {
            self.parents[level - 1]
                .iter()
                .max()
                .map_or(0, |cell| *cell as usize + 1)
        }
    }

    /// The cell a node sits in on the given level.
    ///
    /// # Panics
    ///
    /// Panics if the hierarchy has no such level.
    #[must_use]
    pub fn cell_of(&self, node: NodeID, level: usize) -> CellId {
        assert!(level < self.levels(), "no level {level} in the hierarchy");
        let mut cell = self.base[node];
        for parents in &self.parents[..level] {
            cell = parents[cell as usize];
        }
        cell
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ```text
    ///  level 2:          0
    ///                   / \
    ///  level 1:        0   1
    ///                 / \   \
    ///  level 0:      0   1   2
    ///               /|   |   |\
    ///  nodes:      0 1   2   3 4
    /// ```
    fn directory() -> LevelDirectory {
        LevelDirectory::new(vec![0, 0, 1, 2, 2], vec![vec![0, 0, 1], vec![0, 0]])
    }

    #[test]
    fn a_node_sits_in_one_cell_per_level() {
        let directory = directory();
        assert_eq!(directory.levels(), 3);
        assert_eq!(directory.number_of_nodes(), 5);

        assert_eq!(directory.cell_of(0, 0), 0);
        assert_eq!(directory.cell_of(0, 1), 0);
        assert_eq!(directory.cell_of(0, 2), 0);

        assert_eq!(directory.cell_of(3, 0), 2);
        assert_eq!(directory.cell_of(3, 1), 1);
        assert_eq!(directory.cell_of(3, 2), 0);
    }

    #[test]
    fn the_levels_report_how_many_cells_they_hold() {
        let directory = directory();
        assert_eq!(directory.cells_on_level(0), 3);
        assert_eq!(directory.cells_on_level(1), 2);
        assert_eq!(directory.cells_on_level(2), 1);
    }

    #[test]
    fn one_level_is_a_hierarchy_of_its_own() {
        let directory = LevelDirectory::new(vec![0, 0, 1], Vec::new());
        assert_eq!(directory.levels(), 1);
        assert_eq!(directory.cells_on_level(0), 2);
    }

    #[test]
    fn a_hierarchy_of_one_node_is_answerable() {
        let directory = LevelDirectory::new(vec![0], vec![vec![0], vec![0]]);
        assert_eq!(directory.number_of_nodes(), 1);
        assert_eq!(directory.levels(), 3);
        assert_eq!(directory.cell_of(0, 2), 0);
    }

    #[test]
    #[should_panic(expected = "the levels do not nest")]
    fn a_cell_that_the_level_above_does_not_have_is_caught() {
        // the lowest level has three cells, the table above it only two
        let _ = LevelDirectory::new(vec![0, 1, 2], vec![vec![0, 0]]);
    }

    #[test]
    #[should_panic(expected = "no level 3 in the hierarchy")]
    fn a_level_the_hierarchy_does_not_have_is_caught() {
        let _ = directory().cell_of(0, 3);
    }

    #[test]
    fn the_levels_survive_being_written_and_read() {
        let directory = directory();
        let bytes = rkyv::to_bytes::<rkyv::rancor::Error>(&directory).expect("cannot be written");
        let read: LevelDirectory = rkyv::from_bytes::<LevelDirectory, rkyv::rancor::Error>(&bytes)
            .expect("cannot be read");

        assert_eq!(read, directory);
        for node in 0..directory.number_of_nodes() {
            for level in 0..directory.levels() {
                assert_eq!(read.cell_of(node, level), directory.cell_of(node, level));
            }
        }
    }
}
