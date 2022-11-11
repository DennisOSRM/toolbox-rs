use core::cmp::max;
use serde::{Deserialize, Serialize};
use std::{fmt::Display, hash::Hash};

/// represents the hiearchical partition id scheme. The root id has ID 1 and
/// children are shifted to the left by one and plus 0/1. The parent child
/// relationship can thus be queried in constant time.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PartitionID(u32);

impl PartitionID {
    /// Returns the root id
    pub fn root() -> PartitionID {
        PartitionID(1)
    }

    /// Returns the parent of a given ID.
    /// Note that the parent of the root id is always 1
    pub fn parent(&self) -> PartitionID {
        let new_id = max(1, self.0 >> 1);
        PartitionID::new(new_id)
    }

    pub fn parent_at_level(&self, level: u32) -> PartitionID {
        let parent = self.0 & (0xffff_ffff ^ ((1 << level) - 1));
        PartitionID::new(parent)
    }

    /// Returns a left-right ordered tuple of children for a given ID
    pub fn children(&self) -> (PartitionID, PartitionID) {
        let temp = self.0 << 1;
        (PartitionID(temp), PartitionID(temp + 1))
    }

    /// Returns the left child of a ID
    pub fn left_child(&self) -> PartitionID {
        let temp = self.0 << 1;
        PartitionID(temp)
    }

    /// Returns the right child of a ID
    pub fn right_child(&self) -> PartitionID {
        let temp = self.0 << 1;
        PartitionID(temp + 1)
    }

    /// Transform ID to its left-most descendant k levels down
    pub fn inplace_leftmost_descendant(&mut self, k: usize) {
        self.0 <<= k;
    }

    /// Transform ID to its right-most descendant k levels down
    pub fn inplace_rightmost_descendant(&mut self, k: usize) {
        self.inplace_leftmost_descendant(k);
        self.0 += (1 << k) - 1;
    }

    /// Transform the ID into its left child
    pub fn inplace_left_child(&mut self) {
        self.inplace_leftmost_descendant(1);
    }

    /// Transform the ID into its right child
    pub fn inplace_right_child(&mut self) {
        self.inplace_rightmost_descendant(1);
    }

    /// Returns a new PartitionID from an u32
    pub fn new(id: u32) -> Self {
        // the id scheme is designed in a way that the number of leading zeros is always odd
        assert!(id != 0);
        PartitionID(id)
    }

    /// The level in this scheme is defined by the the number of leading zeroes.
    pub fn level(&self) -> u8 {
        // magic number 31 := 32 - 1, as 1 is the root's ID
        (31 - self.0.leading_zeros()).try_into().unwrap()
    }

    /// Returns whether the ID id a left child
    pub fn is_left_child(&self) -> bool {
        self.0 % 2 == 0
    }

    /// Returns whether the ID id a right child
    pub fn is_right_child(&self) -> bool {
        self.0 % 2 == 1
    }
}

impl Display for PartitionID {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<PartitionID> for usize {
    fn from(s: PartitionID) -> usize {
        s.0.try_into().unwrap()
    }
}

impl core::fmt::Binary for PartitionID {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let val = self.0;
        core::fmt::Binary::fmt(&val, f) // delegate to u32's implementation
    }
}

#[cfg(test)]
mod tests {

    use crate::partition::PartitionID;

    #[test]
    fn parent_id() {
        let id = PartitionID::new(4);
        assert_eq!(id.parent(), PartitionID::new(2));
    }

    #[test]
    fn new_id() {
        let id = PartitionID::new(1);
        assert_eq!(id.parent(), PartitionID::root());
    }

    #[test]
    fn children_ids() {
        let id = PartitionID::new(0b0101_0101_0101_0101u32);
        assert_eq!(id.level(), 14);
        let (child0, child1) = id.children();
        assert_eq!(child0, PartitionID::new(0b1010_1010_1010_1010u32));
        assert_eq!(child1, PartitionID::new(0b1010_1010_1010_1011u32));
    }

    #[test]
    fn level() {
        let root = PartitionID::root();
        assert_eq!(root.level(), 0);
        let (child0, child1) = root.children();

        assert_eq!(child0.level(), 1);
        assert_eq!(child1.level(), 1);
    }

    #[test]
    fn root_parent() {
        let root = PartitionID::root();
        let roots_parent = root.parent();
        assert_eq!(root, roots_parent);
    }

    #[test]
    fn left_right_childs() {
        let id = PartitionID(12345);
        let (left_child, right_child) = id.children();
        assert_eq!(left_child, id.left_child());
        assert_eq!(right_child, id.right_child());
    }

    #[test]
    fn is_left_right_child() {
        let id = PartitionID(12345);
        let (left_child, right_child) = id.children();
        assert_eq!(left_child, id.left_child());
        assert_eq!(right_child, id.right_child());
        assert!(left_child.is_left_child());
        assert!(right_child.is_right_child());
    }

    #[test]
    fn inplace_left_child() {
        let mut id = PartitionID(12345);
        let (left_child, _) = id.children();
        id.inplace_left_child();
        assert_eq!(left_child, id);
    }

    #[test]
    fn inplace_right_child() {
        let mut id = PartitionID(12345);
        let (_, right_child) = id.children();
        id.inplace_right_child();
        assert_eq!(right_child, id);
    }

    #[test]
    fn into_usize() {
        let id = PartitionID(12345);
        let id_usize = usize::from(id);
        assert_eq!(12345, id_usize);
    }

    #[test]
    fn inplace_leftmost_descendant() {
        let id = PartitionID(1);
        let mut current = id;
        for i in 1..30 {
            let mut id = id.clone();
            id.inplace_leftmost_descendant(i);
            assert_eq!(current.left_child(), id);
            current = current.left_child();
        }
    }

    #[test]
    fn inplace_rightmost_descendant() {
        let id = PartitionID(1);
        let mut current = id;
        for i in 1..30 {
            let mut id = id.clone();
            id.inplace_rightmost_descendant(i);
            assert_eq!(current.right_child(), id);
            current = current.right_child();
        }
    }

    #[test]
    fn display() {
        for i in 0..100 {
            let id = PartitionID(i);
            let string = format!("{}", id);
            let recast_id = PartitionID(string.parse::<u32>().unwrap());
            assert_eq!(id, recast_id);
        }
    }

    #[test]
    fn partial_eq() {
        for i in 0..100 {
            let id = PartitionID(i);
            let string = format!("{}", id);
            let recast_id = PartitionID(string.parse::<u32>().unwrap());
            assert_eq!(id, recast_id);
        }
    }

    #[test]
    fn parent_at_level() {
        let id = PartitionID::new(0xffff_ffff);
        let levels = vec![0, 3, 9, 15, 20];
        let results = vec![
            PartitionID::new(0b11111111111111111111111111111111),
            PartitionID::new(0b11111111111111111111111111111000),
            PartitionID::new(0b11111111111111111111111000000000),
            PartitionID::new(0b11111111111111111000000000000000),
            PartitionID::new(0b11111111111100000000000000000000),
        ];
        levels
            .iter()
            .zip(results.iter())
            .for_each(|(level, expected)| {
                assert_eq!(id.parent_at_level(*level), *expected);
            });
    }

    #[test]
    fn binary_trait() {
        let id = PartitionID::new(0xffff_ffff);
        let levels = vec![0, 3, 9, 15, 20];
        let results = vec![
            "0b11111111111111111111111111111111",
            "0b11111111111111111111111111111000",
            "0b11111111111111111111111000000000",
            "0b11111111111111111000000000000000",
            "0b11111111111100000000000000000000",
        ];
        levels
            .iter()
            .zip(results.iter())
            .for_each(|(level, expected)| {
                assert_eq!(format!("{:#032b}", id.parent_at_level(*level)), *expected);
            });
    }
}
