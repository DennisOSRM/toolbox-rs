use std::iter::Iterator;
use std::{cmp::Reverse, collections::BinaryHeap};

pub struct KWayMergeIterator<'a, T: Iterator<Item = i32>> {
    heap: BinaryHeap<Reverse<(i32, usize)>>,
    list: &'a mut [T],
}

impl<'a, T: Iterator<Item = i32>> KWayMergeIterator<'a, T> {
    pub fn new(list: &'a mut [T]) -> Self {
        let mut heap = BinaryHeap::new();
        for (i, iterator) in list.iter_mut().enumerate() {
            if let Some(first) = iterator.next() {
                heap.push(Reverse((first, i)));
            }
        }
        Self { heap, list }
    }

    pub fn has_next(&self) -> bool {
        !self.heap.is_empty()
    }
}

impl<T: Iterator<Item = i32>> Iterator for KWayMergeIterator<'_, T> {
    type Item = i32;

    fn next(&mut self) -> Option<Self::Item> {
        let Reverse((value, list)) = self.heap.pop()?;
        if let Some(next) = self.list[list].next() {
            self.heap.push(Reverse((next, list)));
        }
        Some(value)
    }
}

#[cfg(test)]
mod test {
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
}
