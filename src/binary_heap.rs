// non-addressable priority queue
pub struct BinaryHeap<T: Ord + Copy + Default> {
    h: Vec<T>,
}

impl<T: Ord + Copy + Default> BinaryHeap<T> {
    pub fn build(vec: &[T]) -> BinaryHeap<T> {
        let mut heap = BinaryHeap {
            h: vec.to_vec(),
        };

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
        let last_index = self.h.len() -1;
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
