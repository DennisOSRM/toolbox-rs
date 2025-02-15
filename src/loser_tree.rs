use crate::{merge_entry::MergeEntry, merge_tree::MergeTree};

pub struct LoserTree<T>
where
    T: Ord,
{
    /// array of loser indices
    losers: Vec<usize>,
    /// array of leaves
    leaves: Vec<Option<MergeEntry<T>>>,
    /// index of the winner
    winner: usize,
    size: usize,
}

impl<T: Clone + Ord + PartialOrd> LoserTree<T> {
    pub fn with_capacity(capacity: usize) -> Self {
        let size = capacity.next_power_of_two();
        let mut losers = Vec::with_capacity(size - 1);
        let mut leaves = Vec::with_capacity(size);

        losers.resize(size - 1, 0);
        leaves.resize(size, None);
        Self {
            losers,
            leaves,
            winner: 0,
            size: 0,
        }
    }

    /// play a match between two leaves and return the index of the winner
    fn play_match(&mut self, pos1: usize, pos2: usize) -> usize {
        match &self.leaves[pos1] {
            None => pos2,
            Some(v1) => match &self.leaves[pos2] {
                None => pos1,
                Some(v2) => {
                    if v1 > v2 {
                        pos1
                    } else {
                        pos2
                    }
                }
            },
        }
    }

    /// rebuild only the path from leaf i to the root
    fn rebuild_path(&mut self, mut i: usize) {
        let n = self.leaves.len();
        let internal_nodes = n - 1;

        // convert leaf index to internal node index
        i += internal_nodes;

        // walk up the tree till the root
        while i > 0 {
            // find parent node
            let parent = (i - 1) / 2;

            // sibling of the current node either to the left or to the right
            let sibling = if i % 2 == 0 { i - 1 } else { i + 1 };

            // determine the winner of the match
            let winner = self.play_match(
                if i >= internal_nodes {
                    i - internal_nodes
                } else {
                    self.losers[i]
                },
                if sibling >= internal_nodes {
                    sibling - internal_nodes
                } else {
                    self.losers[sibling]
                },
            );

            self.losers[parent] = winner;
            i = parent;
        }

        self.winner = self.losers[0];
    }

    pub fn clear(&mut self) {
        self.size = 0;
        self.winner = 0;
    }

    pub fn capacity(&self) -> usize {
        self.leaves.len()
    }
}

impl<T: Ord + std::clone::Clone> MergeTree<T> for LoserTree<T> {
    fn push(&mut self, item: MergeEntry<T>) {
        debug_assert!(item.index < self.leaves.len(), "index out of bounds");

        let index = item.index;
        self.leaves[index] = Some(item);

        self.rebuild_path(index);
        self.size += 1;
    }

    fn pop(&mut self) -> std::option::Option<MergeEntry<T>> {
        let winner = self.leaves[self.winner].take();
        if winner.is_some() {
            self.size -= 1;
            self.rebuild_path(self.winner); // O(log n) operation
        }
        winner
    }

    fn is_empty(&self) -> bool {
        self.size == 0
    }

    fn len(&self) -> usize {
        self.size
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_loser_tree() {
        let tree: LoserTree<i32> = LoserTree::with_capacity(3);
        assert_eq!(tree.leaves.len(), 4); // next higher power of 2
        assert_eq!(tree.losers.len(), 3); // internal nodes
        assert!(tree.is_empty());
        assert_eq!(tree.len(), 0);
    }

    #[test]
    fn test_push_and_pop() {
        let mut tree = LoserTree::with_capacity(4);

        tree.push(MergeEntry { item: 3, index: 0 });
        tree.push(MergeEntry { item: 1, index: 1 });
        tree.push(MergeEntry { item: 4, index: 2 });
        tree.push(MergeEntry { item: 2, index: 3 });

        assert_eq!(tree.len(), 4);
        assert!(!tree.is_empty());

        // items shall be sorted
        assert_eq!(tree.pop().unwrap().item, 1);
        assert_eq!(tree.pop().unwrap().item, 2);
        assert_eq!(tree.pop().unwrap().item, 3);
        assert_eq!(tree.pop().unwrap().item, 4);

        assert!(tree.is_empty());
        assert_eq!(tree.pop(), None);
    }

    #[test]
    fn test_partial_fill() {
        let mut tree = LoserTree::with_capacity(4);

        tree.push(MergeEntry { item: 2, index: 0 });
        tree.push(MergeEntry { item: 1, index: 1 });

        assert_eq!(tree.len(), 2);

        assert_eq!(tree.pop().unwrap().item, 1);
        assert_eq!(tree.pop().unwrap().item, 2);
        assert!(tree.pop().is_none());
    }

    #[test]
    fn test_rebuild_after_pop() {
        let mut tree = LoserTree::with_capacity(4);

        tree.push(MergeEntry { item: 3, index: 0 });
        tree.push(MergeEntry { item: 1, index: 1 });
        tree.push(MergeEntry { item: 4, index: 2 });

        assert_eq!(tree.pop().unwrap().item, 1);
        tree.push(MergeEntry { item: 2, index: 1 });

        assert_eq!(tree.pop().unwrap().item, 2);
        assert_eq!(tree.pop().unwrap().item, 3);
        assert_eq!(tree.pop().unwrap().item, 4);
    }

    #[test]
    fn test_merge_tree_interface() {
        let mut tree = LoserTree::with_capacity(4);

        // Test empty state
        assert!(tree.is_empty());
        assert_eq!(tree.len(), 0);
        assert!(tree.pop().is_none());

        // Test pushing elements
        tree.push(MergeEntry { item: 3, index: 0 });
        assert_eq!(tree.len(), 1);
        assert!(!tree.is_empty());

        tree.push(MergeEntry { item: 1, index: 1 });
        assert_eq!(tree.len(), 2);

        // Test popping elements in order
        assert_eq!(tree.pop().unwrap().item, 1);
        assert_eq!(tree.len(), 1);

        assert_eq!(tree.pop().unwrap().item, 3);
        assert_eq!(tree.len(), 0);
        assert!(tree.is_empty());
    }

    #[test]
    #[should_panic(expected = "index out of bounds")]
    fn test_push_invalid_index() {
        let mut tree = LoserTree::with_capacity(2);
        tree.push(MergeEntry { item: 1, index: 2 }); // index out of bounds
    }

    #[test]
    fn test_clear() {
        let mut tree = LoserTree::with_capacity(4);
        tree.push(MergeEntry { item: 1, index: 0 });
        tree.clear();
        assert!(tree.is_empty());
        assert_eq!(tree.len(), 0);
    }

    #[test]
    fn test_capacity() {
        let tree = LoserTree::<i32>::with_capacity(3);
        assert_eq!(tree.capacity(), 4); // nächste Zweierpotenz
    }
}
