//! Which cell a node sits in on each level of a nested partition, and the level
//! on which two nodes first share one.
//!
//! # Shape
//!
//! The levels are nested: a cell of one level lies inside exactly one cell of
//! the level above it. Only the lowest level is therefore stored per node. Each
//! level above it is a table from the cells below to the cells above, which is
//! as long as that level has cells rather than as long as the graph has nodes.
//! A hierarchy of six levels over eighteen million nodes costs the eighteen
//! million entries of the lowest level plus a few hundred thousand for the rest.
//!
//! # The level two nodes meet on
//!
//! Two nodes that share a cell on one level share one on every level above it,
//! so the level they first meet on decides every question about them: it is the
//! level a query between them has to climb to, and the levels below it are the
//! ones a search may stay inside of. [`LevelDirectory::common_level`] walks both
//! nodes up in step and reports the level they land in the same cell on.
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
//!
//! // nodes 0 and 1 share a cell on the lowest level already
//! assert_eq!(directory.common_level(0, 1), Some(0));
//! // nodes 1 and 2 are apart down there and meet one level up
//! assert_eq!(directory.common_level(1, 2), Some(1));
//! // node 4 sits under a root of its own and never meets them
//! assert_eq!(directory.common_level(0, 4), None);
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
    ///
    /// # Panics
    ///
    /// Panics if the hierarchy has no such level.
    #[must_use]
    pub fn cells_on_level(&self, level: usize) -> usize {
        assert!(level < self.levels(), "no level {level} in the hierarchy");
        if level == 0 {
            self.base.iter().max().map_or(0, |cell| *cell as usize + 1)
        } else {
            self.parents[level - 1]
                .iter()
                .max()
                .map_or(0, |cell| *cell as usize + 1)
        }
    }

    /// For every cell of the given level, the cell of the level above that it
    /// lies in. This is what lets the cells of one level be built out of the
    /// cells of the one below it.
    ///
    /// # Panics
    ///
    /// Panics if the hierarchy has no such level, or if the level is the
    /// topmost one, which nothing lies above.
    #[must_use]
    pub fn parents_on_level(&self, level: usize) -> &[CellId] {
        // asked in this order, as a level the hierarchy does not have at all
        // would otherwise be reported as its topmost one
        assert!(level < self.levels(), "no level {level} in the hierarchy");
        assert!(
            level + 1 < self.levels(),
            "level {level} is the topmost one"
        );
        &self.parents[level]
    }

    /// The cell a node sits in on the given level.
    ///
    /// # Panics
    ///
    /// Panics if the hierarchy has no such level, or if it holds no such node.
    #[must_use]
    pub fn cell_of(&self, node: NodeID, level: usize) -> CellId {
        assert!(level < self.levels(), "no level {level} in the hierarchy");
        let mut cell = self.base[node];
        for parents in &self.parents[..level] {
            cell = parents[cell as usize];
        }
        cell
    }

    /// Whether two nodes share a cell on the given level.
    #[must_use]
    pub fn same_cell(&self, u: NodeID, v: NodeID, level: usize) -> bool {
        self.cell_of(u, level) == self.cell_of(v, level)
    }

    /// The lowest level on which two nodes share a cell, and `None` when they
    /// share none at all. A pair that shares a cell on one level shares one on
    /// every level above it, so this is the only level worth asking about.
    #[must_use]
    pub fn common_level(&self, u: NodeID, v: NodeID) -> Option<usize> {
        let (mut left, mut right) = (self.base[u], self.base[v]);
        if left == right {
            return Some(0);
        }
        for (level, parents) in self.parents.iter().enumerate() {
            left = parents[left as usize];
            right = parents[right as usize];
            if left == right {
                return Some(level + 1);
            }
        }
        None
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
    fn two_nodes_meet_on_the_level_their_cells_join() {
        let directory = directory();
        // together on the lowest level
        assert_eq!(directory.common_level(0, 1), Some(0));
        assert_eq!(directory.common_level(3, 4), Some(0));
        // apart down there, together one level up
        assert_eq!(directory.common_level(1, 2), Some(1));
        // and only under the root
        assert_eq!(directory.common_level(0, 3), Some(2));
        assert_eq!(directory.common_level(2, 4), Some(2));
    }

    #[test]
    fn a_node_meets_itself_at_the_bottom() {
        let directory = directory();
        for node in 0..directory.number_of_nodes() {
            assert_eq!(directory.common_level(node, node), Some(0));
        }
    }

    #[test]
    fn the_level_two_nodes_meet_on_does_not_depend_on_their_order() {
        let directory = directory();
        for u in 0..directory.number_of_nodes() {
            for v in 0..directory.number_of_nodes() {
                assert_eq!(directory.common_level(u, v), directory.common_level(v, u));
            }
        }
    }

    /// The level two nodes meet on has to be the first one they share a cell
    /// on, and they have to keep sharing one above it.
    #[test]
    fn nodes_stay_together_above_the_level_they_meet_on() {
        let directory = directory();
        for u in 0..directory.number_of_nodes() {
            for v in 0..directory.number_of_nodes() {
                let meeting = directory.common_level(u, v).expect("a shared root");
                for level in 0..meeting {
                    assert!(!directory.same_cell(u, v, level), "{u} and {v} at {level}");
                }
                for level in meeting..directory.levels() {
                    assert!(directory.same_cell(u, v, level), "{u} and {v} at {level}");
                }
            }
        }
    }

    #[test]
    fn nodes_under_separate_roots_never_meet() {
        // two hierarchies next to each other, with no level joining them
        let directory = LevelDirectory::new(vec![0, 1, 2, 3], vec![vec![0, 0, 1, 1]]);
        assert_eq!(directory.common_level(0, 1), Some(1));
        assert_eq!(directory.common_level(2, 3), Some(1));
        assert_eq!(directory.common_level(0, 2), None);
        assert_eq!(directory.common_level(1, 3), None);
    }

    #[test]
    fn every_node_alone_on_every_level_never_meets() {
        let directory = LevelDirectory::new(vec![0, 1, 2], vec![vec![0, 1, 2], vec![0, 1, 2]]);
        assert_eq!(directory.cells_on_level(2), 3);
        for u in 0..3 {
            for v in 0..3 {
                assert_eq!(
                    directory.common_level(u, v),
                    if u == v { Some(0) } else { None }
                );
            }
        }
    }

    #[test]
    fn one_level_is_a_hierarchy_of_its_own() {
        let directory = LevelDirectory::new(vec![0, 0, 1], Vec::new());
        assert_eq!(directory.levels(), 1);
        assert_eq!(directory.cells_on_level(0), 2);
        assert_eq!(directory.common_level(0, 1), Some(0));
        assert_eq!(directory.common_level(0, 2), None);
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
    #[should_panic(expected = "no level 3 in the hierarchy")]
    fn counting_the_cells_of_a_level_that_is_not_there_is_caught() {
        let _ = directory().cells_on_level(3);
    }

    #[test]
    fn a_level_says_which_cells_of_it_lie_in_which_cell_above() {
        let directory = directory();
        // cells 0 and 1 of the lowest level lie in cell 0 above, cell 2 in cell 1
        assert_eq!(directory.parents_on_level(0), &[0, 0, 1]);
        assert_eq!(directory.parents_on_level(1), &[0, 0]);
    }

    #[test]
    fn the_cells_above_agree_with_walking_a_node_up() {
        let directory = directory();
        for level in 0..directory.levels() - 1 {
            let parents = directory.parents_on_level(level);
            for node in 0..directory.number_of_nodes() {
                let below = directory.cell_of(node, level);
                assert_eq!(parents[below as usize], directory.cell_of(node, level + 1));
            }
        }
    }

    #[test]
    #[should_panic(expected = "level 2 is the topmost one")]
    fn asking_above_the_topmost_level_is_caught() {
        let _ = directory().parents_on_level(2);
    }

    #[test]
    #[should_panic(expected = "no level 7 in the hierarchy")]
    fn asking_for_a_level_that_is_not_there_says_so() {
        let _ = directory().parents_on_level(7);
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
