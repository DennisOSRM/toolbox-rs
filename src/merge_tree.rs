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
