use std::iter::Iterator;

use crate::{merge_entry::MergeEntry, merge_tree::MergeTree};

pub struct KWayMergeIterator<'a, T, I: Iterator<Item = T>, M: MergeTree<T>> {
    heap: M,
    list: &'a mut [I],
}

impl<'a, T: std::cmp::Ord, I: Iterator<Item = T>, M: MergeTree<T>> KWayMergeIterator<'a, T, I, M> {
    pub fn new(list: &'a mut [I], mut heap: M) -> Self {
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

impl<T: std::cmp::Ord, I: Iterator<Item = T>, M: MergeTree<T>> Iterator
    for KWayMergeIterator<'_, T, I, M>
{
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

    use crate::k_way_merge_iterator::MergeEntry;

    #[test]
    fn four_way_merge() {
        let mut list = vec![
            vec![1, 3, 5, 7, 9].into_iter(),
            vec![2, 4, 6, 8, 10].into_iter(),
            vec![11, 13, 15, 17, 19].into_iter(),
            vec![12, 14, 16, 18, 20].into_iter(),
        ];
        let heap = BinaryHeap::new();
        let k_way_merge = super::KWayMergeIterator::new(&mut list, heap);
        let result: Vec<_> = k_way_merge.collect();
        assert_eq!(
            result,
            vec![
                1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20
            ]
        );
    }

    #[test]
    fn three_way_merge_of_differently_sized_sequences() {
        let mut list = vec![
            vec![1, 3, 5, 6, 7, 12].into_iter(),
            vec![2, 4, 8, 11].into_iter(),
            vec![9, 10].into_iter(),
        ];
        let heap = BinaryHeap::new();
        let k_way_merge = super::KWayMergeIterator::new(&mut list, heap);
        let result: Vec<_> = k_way_merge.collect();
        assert_eq!(result, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12]);
    }

    #[test]
    fn merge_of_empty_sequences() {
        let mut list: Vec<std::vec::IntoIter<u64>> = vec![vec![].into_iter(), vec![].into_iter()];
        let heap = BinaryHeap::new();
        let k_way_merge = super::KWayMergeIterator::new(&mut list, heap);
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
