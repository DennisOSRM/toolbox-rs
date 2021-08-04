// non-addressable priority queue
pub struct BinaryHeap<T: Ord + Copy + Default> {
    h: Vec<T>,
}

impl<T: Ord + Copy + Default> BinaryHeap<T> {
    pub fn build(vec: &[T]) -> BinaryHeap<T> {
        let mut heap = BinaryHeap { h: vec.to_vec() };

        // set heap property
        for i in 0..heap.len() / 2 {
            heap.sift_down(i);
        }
        heap
    }

    pub fn new() -> BinaryHeap<T> {
        BinaryHeap {
            h: [T::default()].to_vec(),
        }
    }

    pub fn len(&self) -> usize {
        self.h.len() - 1
    }

    pub fn insert(&mut self, value: T) {
        self.h.push(value);
        self.sift_up(self.len());
    }

    pub fn min(&self) -> &T {
        &self.h[1]
    }

    pub fn delete_min(&mut self) -> T {
        let result = self.h[1];
        let last_index = self.h.len() - 1;
        self.h.swap(1, last_index);
        self.h.pop();
        self.sift_down(1);
        result
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&mut self) {
        self.h = vec![T::default()];
    }

    fn sift_up(&mut self, i: usize) {
        if i == 1 || self.h[i / 2] <= self.h[i] {
            return;
        }
        self.h.swap(i / 2, i);
        self.sift_up(i / 2);
    }

    fn sift_down(&mut self, i: usize) {
        if 2 * i > self.len() {
            return;
        }

        let mut m = 2 * i;
        if 2 * i < self.len() && self.h[2 * i] > self.h[2 * i + 1] {
            m += 1;
        }

        if self.h[i] > self.h[m] {
            self.h.swap(i, m);
            self.sift_down(m);
        }
    }
}

impl<T: Ord + Copy + Default> Default for BinaryHeap<T> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use crate::binary_heap::BinaryHeap;
    use rand::{rngs::StdRng, Rng, SeedableRng};
    type Heap = BinaryHeap<i32>;

    #[test]
    fn empty() {
        let heap = Heap::new();

        assert!(heap.is_empty());
    }

    #[test]
    fn insert_size() {
        let mut heap = Heap::new();
        heap.insert(20);
        assert_eq!(20, *heap.min());
        assert!(!heap.is_empty());
    }

    #[test]
    fn heap_sort() {
        let mut heap = Heap::new();

        let mut input = vec![4, 1, 6, 7, 5];
        for i in &input {
            heap.insert(*i);
        }
        assert_eq!(1, *heap.min());
        assert!(!heap.is_empty());

        let mut result = Vec::new();
        while !heap.is_empty() {
            result.push(heap.delete_min());
        }
        assert_eq!(result.len(), 5);
        assert!(heap.is_empty());

        input.sort();
        assert_eq!(result, input);
    }

    #[test]
    fn heap_sort_random() {
        let mut heap = Heap::new();
        let mut rng = StdRng::seed_from_u64(0xAAaaAAaa);
        let mut input = Vec::new();

        for _ in 0..1000 {
            let number = rng.gen();
            input.push(number);
            heap.insert(number);
        }
        assert!(!heap.is_empty());
        assert_eq!(1000, heap.len());
        assert_eq!(1000, input.len());

        let mut result = Vec::new();
        while !heap.is_empty() {
            result.push(heap.delete_min());
        }
        assert_eq!(result.len(), 1000);
        assert!(heap.is_empty());

        input.sort();
        assert_eq!(result, input);
    }

    #[test]
    fn clear() {
        let mut heap = Heap::new();
        let input = vec![4, 1, 6, 7, 5];

        for i in &input {
            heap.insert(*i);
        }
        assert_eq!(1, *heap.min());
        assert!(!heap.is_empty());
        assert_eq!(5, heap.len());

        heap.clear();
        assert_eq!(0, heap.len());
    }

    #[test]
    #[should_panic]
    fn empty_min_panic() {
        let heap = Heap::new();
        heap.min();
    }
}
