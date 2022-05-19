use core::cmp::max;

/// represents the hiearchical partition id scheme. The root id has ID 1 and
/// children are shifted to the left by one and plus 0/1. The parent child
/// relationship can thus be queried in constant time.
#[derive(Debug, PartialEq)]
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

    /// Returns a left-right ordered tuple of children for a given ID
    pub fn children(&self) -> (PartitionID, PartitionID) {
        let temp = self.0 << 1;
        (PartitionID(temp + 0), PartitionID(temp + 1))
    }

    /// Returns the left child of a ID
    pub fn left_child(&self) -> PartitionID {
        let temp = self.0 << 1;
        PartitionID(temp + 0)
    }

    /// Returns the right child of a ID
    pub fn right_child(&self) -> PartitionID {
        let temp = self.0 << 1;
        PartitionID(temp + 1)
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
}
