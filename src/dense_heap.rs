//! A queue that finds a node by looking it up in an array rather than in a map.
//!
//! [`crate::addressable_binary_heap::AddressableHeap`] holds the place of each
//! node in a hash map, which is what lets it take a node of any kind that
//! counts. A search over a graph does not need that: its nodes are the numbers
//! zero upwards, so the place of a node can sit in an array at that number and
//! be read without hashing anything.
//!
//! What an array costs is room for every node of the graph whether the search
//! reaches it or not, and the trouble of emptying it. The nodes a run put on
//! the queue are already written down, and a search over the cells of a
//! partition puts a few thousand of them on out of eighteen million, so
//! emptying is a walk of those rather than of the array.
//!
//! The room is the price: four bytes for every node of the graph, seventy odd
//! megabytes on a continent, held for as long as the search object is rather
//! than per run. A search that runs once over a large graph should use the
//! map; a search that runs a great many times, which is what this is for, pays
//! it once and stops paying per node thereafter.

use crate::{
    graph::NodeID,
    heap_stats::{Counters, HeapStats, Untracked},
};

/// A queue over dense node ids that counts nothing.
pub type DenseQueue = DenseHeap<Untracked>;

/// The same queue, counting what it was asked to do.
pub type TrackedDenseQueue = DenseHeap<Counters>;

#[derive(Clone, Copy)]
struct Held {
    node: NodeID,
    /// where in the heap this node sits, and zero once it has come off for good
    key: usize,
    weight: usize,
    data: NodeID,
}

/// The place of a node that has not been on the queue during this run.
const NOWHERE: u32 = u32::MAX;

pub struct DenseHeap<S: HeapStats<NodeID> = Untracked> {
    /// the binary heap itself, as places into `held`, one based so that the
    /// root has a parent slot to stop at
    heap: Vec<(u32, usize)>,
    held: Vec<Held>,
    /// where each node sits in `held`, and [`NOWHERE`] for one this run has
    /// not held
    places: Vec<u32>,
    stats: S,
}

impl<S: HeapStats<NodeID>> Default for DenseHeap<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: HeapStats<NodeID>> DenseHeap<S> {
    #[must_use]
    pub fn new() -> Self {
        Self {
            heap: vec![(0, 0)],
            held: Vec::new(),
            places: Vec::new(),
            stats: S::default(),
        }
    }

    pub fn stats(&self) -> &S {
        &self.stats
    }

    /// Forgets the run that has just finished.
    ///
    /// Only what that run put on the queue is put back, which is a few
    /// thousand nodes of a graph of millions.
    pub fn clear(&mut self) {
        self.heap.truncate(1);
        for held in &self.held {
            self.places[held.node] = NOWHERE;
        }
        self.held.clear();
        self.stats = S::default();
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.heap.len() <= 1
    }

    /// How many nodes the queue held during this run.
    #[must_use]
    pub fn inserted_len(&self) -> usize {
        self.held.len()
    }

    fn place_of(&self, node: NodeID) -> Option<usize> {
        match self.places.get(node) {
            Some(&NOWHERE) | None => None,
            Some(&place) => Some(place as usize),
        }
    }

    fn remember(&mut self, node: NodeID, place: usize) {
        if node >= self.places.len() {
            self.places.resize(node + 1, NOWHERE);
        }
        self.places[node] = u32::try_from(place).expect("too many nodes on one queue");
    }

    /// Whether the node has been on the queue during this run.
    #[must_use]
    pub fn inserted(&self, node: NodeID) -> bool {
        self.place_of(node).is_some()
    }

    /// Whether the node is on the queue now.
    #[must_use]
    pub fn contains(&self, node: NodeID) -> bool {
        self.place_of(node)
            .is_some_and(|place| self.held[place].key != 0)
    }

    /// What the node is held at, and the largest number there is for one the
    /// queue has never held.
    #[must_use]
    pub fn weight(&self, node: NodeID) -> usize {
        self.place_of(node)
            .map_or(usize::MAX, |place| self.held[place].weight)
    }

    /// What was written down beside the node, which a search uses for the node
    /// it was reached from.
    ///
    /// # Panics
    ///
    /// Panics for a node the queue has not held during this run.
    #[must_use]
    pub fn data(&self, node: NodeID) -> NodeID {
        let place = self.place_of(node).expect("the queue has not held that");
        self.held[place].data
    }

    pub fn insert(&mut self, node: NodeID, weight: usize, data: NodeID) {
        let place = self.held.len();
        let key = self.heap.len();
        self.held.push(Held {
            node,
            key,
            weight,
            data,
        });
        self.remember(node, place);
        self.heap.push((
            u32::try_from(place).expect("too many nodes on one queue"),
            weight,
        ));
        self.stats.inserted(node);
        self.up_heap(key);
    }

    /// Puts a node on the queue, or lowers what it is held at, in one look.
    pub fn insert_or_decrease(&mut self, node: NodeID, weight: usize, data: NodeID) -> bool {
        let Some(place) = self.place_of(node) else {
            self.insert(node, weight, data);
            return true;
        };
        let held = self.held[place];
        if held.key == 0 || held.weight <= weight {
            return false;
        }
        self.held[place].weight = weight;
        self.held[place].data = data;
        self.heap[held.key].1 = weight;
        self.stats.decreased(node);
        self.up_heap(held.key);
        true
    }

    /// Takes the lightest node off the queue.
    ///
    /// # Panics
    ///
    /// Panics on an empty queue.
    pub fn delete_min(&mut self) -> NodeID {
        assert!(!self.is_empty(), "the queue is empty");
        let place = self.heap[1].0 as usize;
        let last = self.heap.len() - 1;
        self.heap.swap(1, last);
        self.heap.pop();
        if self.heap.len() > 1 {
            self.held[self.heap[1].0 as usize].key = 1;
            self.down_heap(1);
        }
        self.held[place].key = 0;
        let node = self.held[place].node;
        self.stats.deleted(node);
        node
    }

    fn up_heap(&mut self, mut key: usize) {
        let rising = self.heap[key];
        while key > 1 {
            let parent = key >> 1;
            if self.heap[parent].1 <= rising.1 {
                break;
            }
            self.heap[key] = self.heap[parent];
            self.held[self.heap[key].0 as usize].key = key;
            key = parent;
        }
        self.heap[key] = rising;
        self.held[rising.0 as usize].key = key;
    }

    fn down_heap(&mut self, mut key: usize) {
        let sinking = self.heap[key];
        loop {
            let mut child = key << 1;
            if child >= self.heap.len() {
                break;
            }
            if child + 1 < self.heap.len() && self.heap[child + 1].1 < self.heap[child].1 {
                child += 1;
            }
            if self.heap[child].1 >= sinking.1 {
                break;
            }
            self.heap[key] = self.heap[child];
            self.held[self.heap[key].0 as usize].key = key;
            key = child;
        }
        self.heap[key] = sinking;
        self.held[sinking.0 as usize].key = key;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_lightest_comes_off_first() {
        let mut queue = DenseQueue::new();
        for (node, weight) in [(3, 30), (1, 10), (2, 20)] {
            queue.insert(node, weight, node);
        }
        assert_eq!(queue.delete_min(), 1);
        assert_eq!(queue.delete_min(), 2);
        assert_eq!(queue.delete_min(), 3);
        assert!(queue.is_empty());
    }

    #[test]
    fn a_node_lowered_comes_off_sooner() {
        let mut queue = DenseQueue::new();
        queue.insert(1, 10, 1);
        queue.insert(2, 20, 2);
        assert!(queue.insert_or_decrease(2, 5, 9));
        assert_eq!(queue.delete_min(), 2);
        assert_eq!(queue.weight(2), 5);
        assert_eq!(queue.data(2), 9);
    }

    #[test]
    fn a_node_is_not_lowered_to_something_larger() {
        let mut queue = DenseQueue::new();
        queue.insert(1, 10, 1);
        assert!(!queue.insert_or_decrease(1, 20, 7));
        assert_eq!(queue.weight(1), 10);
        assert_eq!(queue.data(1), 1);
    }

    /// A node that has come off for good stays off, whatever is offered for it
    /// afterwards.
    #[test]
    fn a_node_that_has_come_off_does_not_go_back_on() {
        let mut queue = DenseQueue::new();
        queue.insert(1, 10, 1);
        assert_eq!(queue.delete_min(), 1);
        assert!(!queue.insert_or_decrease(1, 1, 1));
        assert!(queue.inserted(1));
        assert!(!queue.contains(1));
        assert_eq!(queue.weight(1), 10);
    }

    /// What one run held is not what the next one sees, and the forgetting is
    /// a count rather than a walk.
    #[test]
    fn a_run_does_not_see_what_the_one_before_it_held() {
        let mut queue = DenseQueue::new();
        queue.insert(5, 50, 5);
        assert!(queue.inserted(5));

        queue.clear();
        assert!(!queue.inserted(5));
        assert!(!queue.contains(5));
        assert_eq!(queue.weight(5), usize::MAX);
        assert_eq!(queue.inserted_len(), 0);
    }

    /// A run puts back what it held and nothing else, so a node of a graph
    /// the run never reached is untouched by it.
    #[test]
    fn a_run_puts_back_only_what_it_held() {
        let mut queue = DenseQueue::new();
        queue.insert(9, 90, 9);
        queue.insert(2, 20, 2);
        queue.clear();

        assert!(!queue.inserted(9));
        assert!(!queue.inserted(2));
        assert!(!queue.inserted(5), "a node it never held");
        queue.insert(9, 1, 9);
        assert_eq!(queue.weight(9), 1);
    }

    #[test]
    fn a_heap_of_many_comes_out_in_order() {
        use rand::{RngExt, SeedableRng, prelude::StdRng};

        let mut rng = StdRng::seed_from_u64(0x_DE95);
        for round in 0..20 {
            let mut queue = DenseQueue::new();
            let count = 50 + round * 7;
            let mut weights = Vec::new();
            for node in 0..count {
                let weight = rng.random_range(0..1000_usize);
                weights.push(weight);
                queue.insert(node, weight, node);
            }
            // and half of them lowered afterwards
            for (node, weight) in weights.iter_mut().enumerate() {
                if rng.random_range(0..2) == 0 {
                    let lower = rng.random_range(0..(*weight).max(1));
                    if queue.insert_or_decrease(node, lower, node) {
                        *weight = lower;
                    }
                }
            }

            weights.sort_unstable();
            let mut came_out = Vec::new();
            while !queue.is_empty() {
                let node = queue.delete_min();
                came_out.push(queue.weight(node));
            }
            assert_eq!(came_out, weights, "round {round}");
        }
    }
}
