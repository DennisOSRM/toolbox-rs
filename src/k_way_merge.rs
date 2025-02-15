use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::iter::Iterator;

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

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct MergeEntry<T> {
    pub item: T,
    pub index: usize,
}

impl<T: Ord> PartialOrd for MergeEntry<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T: Ord> Ord for MergeEntry<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        // reverse ordering for a min heap
        other.item.cmp(&self.item)
    }
}

pub struct KWayMergeIterator<'a, T, I: Iterator<Item = T>> {
    heap: BinaryHeap<MergeEntry<T>>,
    list: &'a mut [I],
}

impl<'a, T: std::cmp::Ord, I: Iterator<Item = T>> KWayMergeIterator<'a, T, I> {
    pub fn new(list: &'a mut [I]) -> Self {
        let mut heap = BinaryHeap::new();
        for (i, iterator) in list.iter_mut().enumerate() {
            if let Some(first) = iterator.next() {
                heap.push(MergeEntry {
                    item: first,
                    index: i,
                });
            }
        }
        Self { heap, list }
    }
}

impl<T: std::cmp::Ord, I: Iterator<Item = T>> Iterator for KWayMergeIterator<'_, T, I> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        let MergeEntry {
            item: value,
            index: list,
        } = self.heap.pop()?;
        if let Some(next) = self.list[list].next() {
            self.heap.push(MergeEntry {
                item: next,
                index: list,
            });
        }
        Some(value)
    }
}

#[cfg(test)]
mod test {
    use std::collections::BinaryHeap;

    use crate::k_way_merge::MergeEntry;

    #[test]
    fn four_way_merge() {
        let mut list = vec![
            vec![1, 3, 5, 7, 9].into_iter(),
            vec![2, 4, 6, 8, 10].into_iter(),
            vec![11, 13, 15, 17, 19].into_iter(),
            vec![12, 14, 16, 18, 20].into_iter(),
        ];
        let k_way_merge = super::KWayMergeIterator::new(&mut list);
        let result: Vec<_> = k_way_merge.collect();
        assert_eq!(
            result,
            vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20]
        );
    }

    #[test]
    fn three_way_merge_of_differently_sized_sequences() {
        let mut list = vec![
            vec![1, 3, 5, 6, 7, 12].into_iter(),
            vec![2, 4, 8, 11].into_iter(),
            vec![9, 10].into_iter(),
        ];
        let k_way_merge = super::KWayMergeIterator::new(&mut list);
        let result: Vec<_> = k_way_merge.collect();
        assert_eq!(result, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
    }

    #[test]
    fn merge_of_empty_sequences() {
        let mut list: Vec<std::vec::IntoIter<u64>> = vec![vec![].into_iter(), vec![].into_iter()];
        let k_way_merge = super::KWayMergeIterator::new(&mut list);
        let result: Vec<_> = k_way_merge.collect();
        assert_eq!(result, Vec::<u64>::new());
    }

    #[test]
    fn test_merge_entry_ordering() {
        let entry1 = MergeEntry { item: 2, index: 0 };
        let entry2 = MergeEntry { item: 1, index: 1 };
        let entry3 = MergeEntry { item: 3, index: 1 };

        // check ascending order
        assert!(entry1 < entry2); // Note the reverse order
        assert!(entry1 > entry3); // Note the reverse order
        assert!(entry2 > entry3); // Note the reverse order

        let mut heap = BinaryHeap::new();
        heap.push(entry1);
        heap.push(entry2);
        heap.push(entry3);

        // output: 1, 2, 3
        assert_eq!(heap.pop().unwrap().item, 1);
        assert_eq!(heap.pop().unwrap().item, 2);
        assert_eq!(heap.pop().unwrap().item, 3);
    }
}
