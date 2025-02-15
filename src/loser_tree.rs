use crate::k_way_merge::{MergeEntry, MergeTree};

pub struct LoserTree<T>
where
    MergeEntry<T>: Clone,
{
    /// array of loser indices
    losers: Vec<usize>,
    /// array of leaves
    leaves: Vec<Option<MergeEntry<T>>>,
    /// index of the winner
    winner: usize,
}

impl<T: Clone + Ord + PartialOrd> LoserTree<T> {
    pub fn new(capacity: usize) -> Self {
        let size = capacity.next_power_of_two();
        Self {
            losers: Vec::with_capacity(size - 1),
            leaves: Vec::with_capacity(size),
            winner: 0,
        }
    }

    /// Play a match between two leaves and return the index of the winner
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

    /// Rebuild the full tree from the leaves to the root
    // pub fn rebuild_full_tree(&mut self) {
    //     let n = self.leaves.len();
    //     let internal_nodes = n - 1;

    //     // play all matches bottom-up
    //     for pos in (0..internal_nodes).rev() {
    //         let left = 2 * pos + 1;
    //         let right = 2 * pos + 2;

    //         let winner = self.play_match(
    //             if left >= internal_nodes {
    //                 left - internal_nodes
    //             } else {
    //                 self.losers[left]
    //             },
    //             if right >= internal_nodes {
    //                 right - internal_nodes
    //             } else {
    //                 self.losers[right]
    //             },
    //         );
    //         self.losers[pos] = winner;
    //     }

    //     self.winner = self.losers[0];
    // }

    /// rebuild only the path from leaf i to the root
    fn rebuild_path(&mut self, mut i: usize) {
        let n = self.leaves.len();
        let internal_nodes = n - 1;

        // convert leaf index to internal node index
        i = i + internal_nodes;

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
}

impl<T: Ord + std::clone::Clone> MergeTree<T> for LoserTree<T> {
    fn push(&mut self, item: MergeEntry<T>) {
        let index = item.index;
        self.leaves[index].replace(item);

        self.rebuild_path(index);
    }

    fn pop(&mut self) -> std::option::Option<MergeEntry<T>> {
        let winner = self.leaves[self.winner].take();
        if winner.is_some() {
            self.rebuild_path(self.winner); // O(log n) operation
        }
        winner
    }

    fn is_empty(&self) -> bool {
        self.leaves.iter().all(|x| x.is_none())
    }

    fn len(&self) -> usize {
        self.leaves.iter().filter(|x| x.is_some()).count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_loser_tree() {
        let tree: LoserTree<i32> = LoserTree::new(3);
        assert_eq!(tree.leaves.len(), 4); // next higher power of 2
        assert_eq!(tree.losers.len(), 3); // internal nodes
        assert!(tree.is_empty());
        assert_eq!(tree.len(), 0);
    }

    #[test]
    fn test_push_and_pop() {
        let mut tree = LoserTree::new(4);

        // Test Push
        tree.push(MergeEntry { item: 3, index: 0 });
        tree.push(MergeEntry { item: 1, index: 1 });
        tree.push(MergeEntry { item: 4, index: 2 });
        tree.push(MergeEntry { item: 2, index: 3 });

        assert_eq!(tree.len(), 4);
        assert!(!tree.is_empty());

        // Items shall be sorted
        assert_eq!(tree.pop().unwrap().item, 1);
        assert_eq!(tree.pop().unwrap().item, 2);
        assert_eq!(tree.pop().unwrap().item, 3);
        assert_eq!(tree.pop().unwrap().item, 4);

        assert!(tree.is_empty());
        assert_eq!(tree.pop(), None);
    }

    #[test]
    fn test_partial_fill() {
        let mut tree = LoserTree::new(4);

        tree.push(MergeEntry { item: 2, index: 0 });
        tree.push(MergeEntry { item: 1, index: 1 });

        assert_eq!(tree.len(), 2);

        assert_eq!(tree.pop().unwrap().item, 1);
        assert_eq!(tree.pop().unwrap().item, 2);
        assert!(tree.pop().is_none());
    }

    #[test]
    fn test_rebuild_after_pop() {
        let mut tree = LoserTree::new(4);

        tree.push(MergeEntry { item: 3, index: 0 });
        tree.push(MergeEntry { item: 1, index: 1 });
        tree.push(MergeEntry { item: 4, index: 2 });

        assert_eq!(tree.pop().unwrap().item, 1);
        tree.push(MergeEntry { item: 2, index: 1 });

        assert_eq!(tree.pop().unwrap().item, 2);
        assert_eq!(tree.pop().unwrap().item, 3);
        assert_eq!(tree.pop().unwrap().item, 4);
    }
}
