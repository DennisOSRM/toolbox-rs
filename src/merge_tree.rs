use std::collections::BinaryHeap;

use crate::merge_entry::MergeEntry;

pub trait MergeTree<T> {
    /// Pushes an item onto the merge tree
    fn push(&mut self, item: MergeEntry<T>);

    /// Removes and returns the minimum item from the tree
    fn pop(&mut self) -> Option<MergeEntry<T>>;

    /// Returns true if the tree is empty
    fn is_empty(&self) -> bool;

    /// Returns the number of items in the tree
    fn len(&self) -> usize;
}

impl<T: Ord> MergeTree<T> for BinaryHeap<MergeEntry<T>> {
    fn push(&mut self, item: MergeEntry<T>) {
        self.push(item);
    }

    fn pop(&mut self) -> std::option::Option<MergeEntry<T>> {
        self.pop()
    }

    fn is_empty(&self) -> bool {
        self.is_empty()
    }

    fn len(&self) -> usize {
        self.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_binary_heap_merge_tree() {
        let mut heap: BinaryHeap<MergeEntry<i32>> = BinaryHeap::new();

        // test empty heap
        assert!(heap.is_empty());
        assert_eq!(heap.len(), 0);
        assert!(heap.pop().is_none());

        // test push operations
        heap.push(MergeEntry { item: 3, index: 0 });
        assert_eq!(heap.len(), 1);
        assert!(!heap.is_empty());

        heap.push(MergeEntry { item: 1, index: 1 });
        heap.push(MergeEntry { item: 4, index: 2 });
        heap.push(MergeEntry { item: 2, index: 3 });
        assert_eq!(heap.len(), 4);

        // test pop operations - items shall be sorted
        assert_eq!(heap.pop().unwrap().item, 1);
        assert_eq!(heap.pop().unwrap().item, 2);
        assert_eq!(heap.pop().unwrap().item, 3);
        assert_eq!(heap.pop().unwrap().item, 4);

        // test empty heap - again
        assert!(heap.is_empty());
        assert_eq!(heap.len(), 0);
        assert!(heap.pop().is_none());
    }

    #[test]
    fn test_push_duplicate_values() {
        let mut heap: BinaryHeap<MergeEntry<i32>> = BinaryHeap::new();

        heap.push(MergeEntry { item: 1, index: 0 });
        heap.push(MergeEntry { item: 1, index: 1 });

        assert_eq!(heap.len(), 2);
        assert_eq!(heap.pop().unwrap().index, 0);
        assert_eq!(heap.pop().unwrap().index, 1);
    }

    #[test]
    fn test_mixed_operations() {
        let mut heap: BinaryHeap<MergeEntry<i32>> = BinaryHeap::new();

        heap.push(MergeEntry { item: 3, index: 0 });
        assert_eq!(heap.len(), 1);

        heap.push(MergeEntry { item: 1, index: 1 });
        assert_eq!(heap.len(), 2);

        assert_eq!(heap.pop().unwrap().item, 1);
        assert_eq!(heap.len(), 1);

        heap.push(MergeEntry { item: 2, index: 2 });
        assert_eq!(heap.len(), 2);

        assert_eq!(heap.pop().unwrap().item, 2);
        assert_eq!(heap.pop().unwrap().item, 3);
        assert!(heap.is_empty());
    }
}
