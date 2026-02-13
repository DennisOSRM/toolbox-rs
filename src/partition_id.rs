use core::cmp::max;
use rkyv::{Archive, Deserialize, Serialize};
use std::{
    fmt::Display,
    hash::Hash,
    ops::{BitAnd, BitOr},
};

/// represents the hiearchical partition id scheme. The root id has ID 1 and
/// children are shifted to the left by one and plus 0/1. The parent child
/// relationship can thus be queried in constant time.
#[derive(
    Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Archive, Serialize, Deserialize,
)]
pub struct PartitionID(pub u32);

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
    pub fn make_leftmost_descendant(&mut self, k: usize) {
        self.0 <<= k;
    }

    /// Transform ID to its right-most descendant k levels down
    pub fn make_rightmost_descendant(&mut self, k: usize) {
        self.make_leftmost_descendant(k);
        self.0 += (1 << k) - 1;
    }

    /// Transform the ID into its left child
    pub fn make_left_child(&mut self) {
        self.make_leftmost_descendant(1);
    }

    /// Transform the ID into its right child
    pub fn make_right_child(&mut self) {
        self.make_rightmost_descendant(1);
    }

    /// Returns a new PartitionID from an u32
    pub fn new(id: u32) -> Self {
        // the id scheme is designed in a way that the number of leading zeros is always odd
        debug_assert!(id != 0);
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

    // Returns the lowest common ancestor of this and the other ID
    pub fn lowest_common_ancestor(&self, other: &PartitionID) -> PartitionID {
        let mut left = *self;
        let mut right = *other;

        let left_level = left.level();
        let right_level = right.level();

        if left_level > right_level {
            left.0 >>= left_level - right_level;
        }
        if right_level > left_level {
            right.0 >>= right_level - left_level;
        }

        while left != right {
            left = left.parent();
            right = right.parent();
        }
        left
    }

    pub fn extract_bit(&self, index: usize) -> bool {
        let mask = 1 << index;
        mask & self.0 > 0
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

impl BitAnd for PartitionID {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl BitOr for PartitionID {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

#[cfg(test)]
mod tests {

    use crate::partition_id::PartitionID;

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
    fn make_left_child() {
        let mut id = PartitionID(12345);
        let (left_child, _) = id.children();
        id.make_left_child();
        assert_eq!(left_child, id);
    }

    #[test]
    fn make_right_child() {
        let mut id = PartitionID(12345);
        let (_, right_child) = id.children();
        id.make_right_child();
        assert_eq!(right_child, id);
    }

    #[test]
    fn into_usize() {
        let id = PartitionID(12345);
        let id_usize = usize::from(id);
        assert_eq!(12345, id_usize);
    }

    #[test]
    fn make_leftmost_descendant() {
        let id = PartitionID(1);
        let mut current = id;
        for i in 1..30 {
            let mut id = id;
            id.make_leftmost_descendant(i);
            assert_eq!(current.left_child(), id);
            current = current.left_child();
        }
    }

    #[test]
    fn make_rightmost_descendant() {
        let id = PartitionID(1);
        let mut current = id;
        for i in 1..30 {
            let mut id = id;
            id.make_rightmost_descendant(i);
            assert_eq!(current.right_child(), id);
            current = current.right_child();
        }
    }

    #[test]
    fn display() {
        for i in 0..100 {
            let id = PartitionID(i);
            let string = format!("{id}");
            let recast_id = PartitionID(string.parse::<u32>().unwrap());
            assert_eq!(id, recast_id);
        }
    }

    #[test]
    fn partial_eq() {
        for i in 0..100 {
            let id = PartitionID(i);
            let string = format!("{id}");
            let recast_id = PartitionID(string.parse::<u32>().unwrap());
            assert_eq!(id, recast_id);
        }
    }

    #[test]
    fn parent_at_level() {
        let id = PartitionID::new(0xffff_ffff);
        let levels = [0, 3, 9, 15, 20];
        let results = [
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
        let levels = [0, 3, 9, 15, 20];
        let results = [
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

    #[test]
    fn lowest_common_ancestor() {
        let a = PartitionID(0b1000);
        let b = PartitionID(0b1001);
        assert_eq!(a.lowest_common_ancestor(&b), b.lowest_common_ancestor(&a));

        let expected = PartitionID(0b100);
        assert_eq!(a.lowest_common_ancestor(&b), expected);

        let a = PartitionID(0b1001);
        let b = PartitionID(0b1111);
        assert_eq!(a.lowest_common_ancestor(&b), b.lowest_common_ancestor(&a));

        assert_eq!(a.lowest_common_ancestor(&b), PartitionID::root());
    }

    #[test]
    fn bitand() {
        let a = PartitionID(0b1000);
        let b = PartitionID(0b1001);
        assert_eq!(PartitionID(0b1000), a & b);
    }

    #[test]
    fn bitor() {
        let a = PartitionID(0b1000);
        let b = PartitionID(0b1001);
        assert_eq!(PartitionID(0b1001), a | b);
    }

    #[test]
    fn extract_bit() {
        let a = PartitionID(0b1001);
        assert!(a.extract_bit(0));
        assert!(!a.extract_bit(1));
        assert!(!a.extract_bit(2));
        assert!(a.extract_bit(3));
        assert!(!a.extract_bit(4));

        let a = PartitionID(0b100000000100000001000);
        // [0, 3, 7, 11, 15]
        assert!(!a.extract_bit(0));
        assert!(a.extract_bit(3));
        assert!(!a.extract_bit(7));
        assert!(a.extract_bit(11));
        assert!(!a.extract_bit(15));
    }

    #[test]
    fn make_left_child_parent() {
        let mut id = PartitionID(12345);
        let original = id;
        id.make_left_child();
        assert_eq!(original, id.parent());
        assert!(id.is_left_child());
        assert!(!id.is_right_child());
    }

    #[test]
    fn make_right_child_parent() {
        let mut id = PartitionID(12345);
        let original = id;
        id.make_right_child();
        assert_eq!(original, id.parent());
        assert!(!id.is_left_child());
        assert!(id.is_right_child());
    }

    #[test]
    fn lowest_common_ancestor_comprehensive() {
        // Test case 1: Same node - should return itself
        let node = PartitionID(0b1000);
        assert_eq!(node.lowest_common_ancestor(&node), node);

        // Test case 2: Parent-child relationship
        let parent = PartitionID(0b100);
        let left_child = PartitionID(0b1000);
        let right_child = PartitionID(0b1001);
        assert_eq!(left_child.lowest_common_ancestor(&parent), parent);
        assert_eq!(right_child.lowest_common_ancestor(&parent), parent);
        assert_eq!(parent.lowest_common_ancestor(&left_child), parent);
        assert_eq!(parent.lowest_common_ancestor(&right_child), parent);

        // Test case 3: Siblings
        assert_eq!(left_child.lowest_common_ancestor(&right_child), parent);
        assert_eq!(right_child.lowest_common_ancestor(&left_child), parent);

        // Test case 4: Nodes at very different levels
        let deep_node = PartitionID(0b1000_0000); // Level 6
        let shallow_node = PartitionID(0b10); // Level 1
        let expected_ancestor = PartitionID(0b10); // Their LCA should be at level 1
        assert_eq!(
            deep_node.lowest_common_ancestor(&shallow_node),
            expected_ancestor
        );
        assert_eq!(
            shallow_node.lowest_common_ancestor(&deep_node),
            expected_ancestor
        );

        // Test case 5: Root cases
        let root = PartitionID::root();
        let any_node = PartitionID(0b1111);
        assert_eq!(root.lowest_common_ancestor(&any_node), root);
        assert_eq!(any_node.lowest_common_ancestor(&root), root);
        assert_eq!(root.lowest_common_ancestor(&root), root);

        // Test case 6: Complex tree relationship
        // Create a more complex scenario:
        //           1
        //         /   \
        //       2       3
        //      / \     / \
        //     4   5   6   7
        //    /
        //   8
        let node_1 = PartitionID::root(); // 1
        let node_2 = PartitionID(0b10); // 2
        let node_3 = PartitionID(0b11); // 3
        let node_4 = PartitionID(0b100); // 4
        let node_5 = PartitionID(0b101); // 5
        let node_6 = PartitionID(0b110); // 6
        let node_7 = PartitionID(0b111); // 7
        let node_8 = PartitionID(0b1000); // 8

        // Test various relationships
        assert_eq!(node_4.lowest_common_ancestor(&node_5), node_2); // Siblings under 2
        assert_eq!(node_6.lowest_common_ancestor(&node_7), node_3); // Siblings under 3
        assert_eq!(node_4.lowest_common_ancestor(&node_6), node_1); // Cousins
        assert_eq!(node_8.lowest_common_ancestor(&node_5), node_2); // Uncle relationship
        assert_eq!(node_8.lowest_common_ancestor(&node_7), node_1); // Different subtrees
    }
}
