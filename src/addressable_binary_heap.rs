use crate::heap_stats::{Counters, HeapStats, Untracked};
use core::hash::Hash;
use num::{Bounded, Integer};
use rustc_hash::FxHashMap;
use std::fmt::Debug;

struct HeapNode<NodeID: Copy + Integer, Weight: Bounded + Copy + Integer + Debug, Data> {
    node: NodeID,
    key: usize,
    weight: Weight,
    data: Data,
}

impl<NodeID: Copy + Integer, Weight: Bounded + Copy + Integer + Debug, Data>
    HeapNode<NodeID, Weight, Data>
{
    fn new(node: NodeID, key: usize, weight: Weight, data: Data) -> Self {
        Self {
            node,
            key,
            weight,
            data,
        }
    }
}
#[derive(Clone, Copy)]
struct HeapElement<Weight: Bounded + Copy + Integer + Debug> {
    index: usize,
    weight: Weight,
}

impl<Weight: Bounded + Copy + Integer + Debug> Default for HeapElement<Weight> {
    fn default() -> Self {
        HeapElement::new(usize::MAX, Weight::min_value())
    }
}

impl<Weight: Bounded + Copy + Integer + Debug> HeapElement<Weight> {
    fn new(index: usize, weight: Weight) -> Self {
        Self { index, weight }
    }
}

/// An addressable heap that collects nothing, which is what a run whose time
/// is being taken wants.
pub type AddressableHeap<NodeID, Weight, Data> =
    AddressableHeapWithStats<NodeID, Weight, Data, Untracked>;

/// The same heap, counting what it was asked to do.
pub type TrackedAddressableHeap<NodeID, Weight, Data> =
    AddressableHeapWithStats<NodeID, Weight, Data, Counters>;

pub struct AddressableHeapWithStats<
    NodeID: Copy + Integer,
    Weight: Bounded + Copy + Integer + Debug,
    Data,
    S: HeapStats<NodeID>,
> {
    heap: Vec<HeapElement<Weight>>,
    inserted_nodes: Vec<HeapNode<NodeID, Weight, Data>>,
    node_index: FxHashMap<NodeID, usize>,
    /// What the queue is telling whoever is collecting. Untracked holds
    /// nothing and takes up nothing, so a queue that is being timed carries
    /// none of this.
    stats: S,
}

impl<
    NodeID: Copy + Hash + Integer,
    Weight: Bounded + Copy + Integer + Debug,
    Data,
    S: HeapStats<NodeID>,
> Default for AddressableHeapWithStats<NodeID, Weight, Data, S>
{
    fn default() -> Self {
        Self::new()
    }
}

impl<
    NodeID: Copy + Hash + Integer,
    Weight: Bounded + Copy + Integer + Debug,
    Data,
    S: HeapStats<NodeID>,
> AddressableHeapWithStats<NodeID, Weight, Data, S>
{
    /// What the queue has done since it was last cleared.
    pub fn stats(&self) -> &S {
        &self.stats
    }

    pub fn new() -> AddressableHeapWithStats<NodeID, Weight, Data, S> {
        AddressableHeapWithStats {
            heap: vec![HeapElement::default()],
            inserted_nodes: Vec::new(),
            node_index: FxHashMap::default(),
            stats: S::default(),
        }
    }

    pub fn clear(&mut self) {
        self.stats = S::default();
        self.heap.clear();
        self.inserted_nodes.clear();
        self.heap.push(HeapElement::default());
        self.node_index.clear();
    }

    pub fn len(&self) -> usize {
        self.heap.len() - 1
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// return the number of inserted elements since the last time queue was
    /// cleared. Note that this is not the number of elements currently in
    /// the heap, nor the number of removed elements.
    pub fn inserted_len(&self) -> usize {
        self.inserted_nodes.len()
    }

    pub fn insert(&mut self, node: NodeID, weight: Weight, data: Data) {
        self.stats.inserted(node);
        let index = self.inserted_nodes.len();
        let element = HeapElement { index, weight };
        let key = self.heap.len();
        self.heap.push(element);
        self.inserted_nodes
            .push(HeapNode::new(node, key, weight, data));
        self.node_index.insert(node, index);
        self.up_heap(key);
    }

    pub fn data(&self, node: NodeID) -> &Data {
        let index = self.node_index.get(&node).unwrap();
        &self.inserted_nodes.get(*index).unwrap().data
    }

    pub fn data_mut(&mut self, node: NodeID) -> &mut Data {
        let index = self.node_index.get(&node).unwrap();
        &mut self.inserted_nodes.get_mut(*index).unwrap().data
    }

    pub fn weight(&self, node: NodeID) -> Weight {
        let index = self.node_index.get(&node);
        if let Some(index) = index {
            self.inserted_nodes.get(*index).unwrap().weight
        } else {
            Weight::max_value()
        }
    }

    pub fn removed(&self, node: NodeID) -> bool {
        let index = self.node_index.get(&node);
        if let Some(index) = index {
            self.inserted_nodes.get(*index).unwrap().key == 0
        } else {
            false
        }
    }

    pub fn contains(&self, node: NodeID) -> bool {
        let index = self.node_index.get(&node);
        if let Some(index) = index {
            self.inserted_nodes.get(*index).unwrap().key != 0
        } else {
            false
        }
    }

    pub fn inserted(&self, node: NodeID) -> bool {
        let index = self.node_index.get(&node);
        if let Some(index) = index {
            debug_assert!(index < &self.inserted_nodes.len());
            self.inserted_nodes.get(*index).unwrap().node == node
        } else {
            false
        }
    }

    /// Returns the node with minimum weight without removing it from the heap.
    /// Panics if the heap is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use toolbox_rs::addressable_binary_heap::AddressableHeap;
    /// let mut heap = AddressableHeap::new();
    /// heap.insert(1, 1, 0);
    /// heap.insert(2, 2, 0);
    /// assert_eq!(heap.min(), 1);
    /// ```
    pub fn min(&self) -> NodeID {
        let index = self.heap[1].index;
        self.inserted_nodes[index].node
    }

    /// Returns the node with minimum weight without removing it, or None if the heap is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use toolbox_rs::addressable_binary_heap::AddressableHeap;
    /// let mut heap = AddressableHeap::new();
    ///
    /// assert_eq!(heap.pop(), None);  // Empty heap
    ///
    /// heap.insert(1, 1, 0);
    /// heap.insert(2, 2, 0);
    /// assert_eq!(heap.pop(), Some(1));  // Returns min without removing
    /// assert_eq!(heap.pop(), Some(1));  // Still returns 1 as it wasn't removed
    /// ```
    #[inline]
    pub fn pop(&mut self) -> Option<NodeID> {
        if self.is_empty() {
            return None;
        }
        Some(self.min())
    }

    pub fn delete_min(&mut self) -> NodeID {
        let removed_index = self.heap[1].index;
        let last_index = self.heap.len() - 1;
        self.heap.swap(1, last_index);

        self.heap.pop();
        if self.heap.len() > 1 {
            self.down_heap(1);
        }
        self.inserted_nodes[removed_index].key = 0;
        let node = self.inserted_nodes[removed_index].node;
        self.stats.deleted(node);
        node
    }

    /// Empties the heap while keeping what it knows about the nodes that went
    /// through it, so their weight and data can still be read afterwards.
    pub fn flush(&mut self) {
        (1..self.heap.len()).for_each(|i| {
            let element = &self.heap[i];
            self.inserted_nodes[element.index].key = 0;
        });
        self.heap.truncate(1);
        // the element at zero is the wall that up_heap rises until, so it has
        // to stay below every weight that can be inserted after this
        self.heap[0].weight = Weight::min_value();
    }

    pub fn decrease_key(&mut self, node: NodeID, weight: Weight) {
        self.stats.decreased(node);
        let index = self.node_index[&node];
        let key = self.inserted_nodes[index].key;
        // not a debug_assert: a key of zero is the sentinel, and lowering that
        // one leaves up_heap with no wall to stop at on the next insert. A
        // release build has to refuse it too, and the branch is nothing next to
        // the lookup above it.
        assert!(key != 0, "the node is not in the heap");

        // the weight is held twice over, once next to the node and once in the
        // heap itself, and the heap is ordered by its own copy. Lowering only
        // the first leaves the element sitting where the old weight put it,
        // and up_heap, which reads the heap, would find nothing to do.
        self.inserted_nodes[index].weight = weight;
        self.heap[key].weight = weight;
        self.up_heap(key);
    }

    pub fn decrease_key_and_update_data(&mut self, node: NodeID, weight: Weight, data: Data) {
        self.decrease_key(node, weight);
        (*self.data_mut(node)) = data;
    }

    fn down_heap(&mut self, mut key: usize) {
        let dropping_index = self.heap[key].index;
        let weight = self.heap[key].weight;

        let mut next_key = key << 1;
        while next_key < self.heap.len() {
            let next_key_sibling = next_key + 1;
            if next_key_sibling < self.heap.len()
                && self.heap[next_key].weight > self.heap[next_key_sibling].weight
            {
                next_key = next_key_sibling;
            }
            if weight <= self.heap[next_key].weight {
                break;
            }
            self.heap[key] = self.heap[next_key];
            self.inserted_nodes[self.heap[key].index].key = key;
            key = next_key;
            next_key <<= 1;
        }
        self.heap[key] = HeapElement {
            index: dropping_index,
            weight,
        };
        self.inserted_nodes[dropping_index].key = key;
    }

    pub fn up_heap(&mut self, mut key: usize) {
        let rising_index = self.heap[key].index;
        let weight = self.heap[key].weight;

        let mut next_key = key >> 1;

        while self.heap[next_key].weight > weight {
            self.heap[key] = self.heap[next_key];
            let index = self.heap[key].index;

            self.inserted_nodes[index].key = key;
            key = next_key;
            next_key >>= 1;
        }
        self.heap[key].index = rising_index;
        self.heap[key].weight = weight;
        self.inserted_nodes[rising_index].key = key;
    }
}

#[cfg(test)]
mod tests {
    use rand::{RngExt, SeedableRng, prelude::StdRng};

    use crate::addressable_binary_heap::AddressableHeap;
    type Heap = AddressableHeap<i32, i32, i32>;

    #[test]
    fn empty() {
        let heap = Heap::new();
        assert!(heap.is_empty());
    }

    #[test]
    fn insert_size() {
        let mut heap = Heap::new();
        heap.insert(20, 1, 2);
        assert_eq!(20, heap.min());
        assert!(!heap.is_empty());
        assert_eq!(heap.len(), 1);
    }

    #[test]
    fn heap_sort() {
        let mut heap = Heap::new();

        let mut input = vec![4, 1, 6, 7, 5];
        for i in &input {
            heap.insert(*i, *i, 0);
        }
        assert_eq!(1, heap.min());
        assert!(!heap.is_empty());

        let mut result = Vec::new();
        while !heap.is_empty() {
            result.push(heap.delete_min());
        }
        assert_eq!(result.len(), 5);
        assert!(heap.is_empty());

        // Sorting unstable is OK. No observable difference on integers.
        input.sort_unstable();
        assert_eq!(result, input);
    }

    #[test]
    #[should_panic]
    fn empty_min_panic() {
        let heap = Heap::new();
        heap.min();
    }

    #[test]
    fn heap_sort_random() {
        let mut heap = Heap::new();
        let mut rng = StdRng::seed_from_u64(0xAAAAAAAA);
        let mut input = Vec::new();

        for _ in 0..1000 {
            let number = rng.random();
            input.push(number);
            heap.insert(number, number, 0);
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

        // Sorting unstable is OK. No observable difference on integers.
        input.sort_unstable();
        assert_eq!(result, input);
    }

    #[test]
    fn clear() {
        let mut heap = Heap::new();
        let input = vec![4, 1, 6, 7, 5];

        for i in &input {
            heap.insert(*i, *i, *i);
        }
        assert_eq!(1, heap.min());
        assert!(!heap.is_empty());
        assert_eq!(5, heap.len());

        heap.clear();
        assert_eq!(0, heap.len());
    }

    #[test]
    fn data() {
        let mut heap = Heap::new();
        let input = vec![4, 1, 6, 7, 5];

        for i in &input {
            heap.insert(*i, *i, *i);
        }
        assert_eq!(1, heap.min());
        assert!(!heap.is_empty());
        assert_eq!(5, heap.len());

        for i in &input {
            assert_eq!(i, heap.data(*i));
        }
    }

    #[test]
    fn data_mut() {
        let mut heap = Heap::new();
        let input = vec![4, 1, 6, 7, 5];

        for i in &input {
            heap.insert(*i, *i, *i);
        }
        assert_eq!(1, heap.min());
        assert!(!heap.is_empty());
        assert_eq!(5, heap.len());

        // double all data entries
        for i in &input {
            let new_value = *heap.data_mut(*i) * 2;
            *heap.data_mut(*i) = new_value;
        }

        for i in &input {
            let new_value = 2 * i;
            assert_eq!(&new_value, heap.data(*i));
        }
    }

    #[test]
    fn flush() {
        let mut heap = Heap::default();
        let input = vec![4, 1, 6, 7, 5];

        for i in &input {
            heap.insert(*i, *i, *i);
        }
        assert_eq!(1, heap.min());
        assert!(!heap.is_empty());
        assert_eq!(5, heap.len());

        heap.flush();
        assert!(heap.is_empty());
        assert_eq!(0, heap.len());
        for i in &input {
            assert!(!heap.contains(*i), "{i} is still held after a flush");
            assert!(heap.removed(*i));
        }

        // and the heap goes on being a heap
        heap.insert(9, 9, 9);
        heap.insert(8, 8, 8);
        assert_eq!(8, heap.min());
    }

    #[test]
    fn removed() {
        let mut heap = Heap::default();
        let input = vec![4, 1, 6, 7, 5];

        for i in &input {
            heap.insert(*i, *i, *i);
        }
        assert_eq!(1, heap.min());
        assert!(!heap.is_empty());
        assert_eq!(5, heap.len());

        assert!(!heap.removed(1));
        assert!(!heap.removed(2));
        assert!(!heap.removed(3));
        assert!(!heap.removed(4));
        assert!(!heap.removed(5));
        assert!(!heap.removed(6));
        assert!(!heap.removed(7));

        while !heap.is_empty() {
            heap.delete_min();
        }

        assert!(heap.removed(1));
        assert!(!heap.removed(2));
        assert!(!heap.removed(3));
        assert!(heap.removed(4));
        assert!(heap.removed(5));
        assert!(heap.removed(6));
        assert!(heap.removed(7));
    }

    #[test]
    fn inserted() {
        let mut heap = Heap::default();
        let input = vec![4, 1, 6, 7, 5];

        for i in &input {
            heap.insert(*i, *i, *i);
        }
        assert_eq!(1, heap.min());
        assert!(!heap.is_empty());
        assert_eq!(5, heap.len());

        while !heap.is_empty() {
            heap.delete_min();
        }

        assert!(heap.inserted(1));
        assert!(!heap.inserted(2));
        assert!(!heap.inserted(3));
        assert!(heap.inserted(4));
        assert!(heap.inserted(5));
        assert!(heap.inserted(6));
        assert!(heap.inserted(7));
    }

    #[test]
    fn weight() {
        let mut heap = Heap::default();
        let input = vec![4, 1, 6, 7, 5];

        for i in &input {
            heap.insert(*i, 2 + *i, *i);
        }
        assert_eq!(1, heap.min());
        assert!(!heap.is_empty());
        assert_eq!(5, heap.len());

        while !heap.is_empty() {
            let node = heap.delete_min();
            assert_eq!(heap.weight(node), 2 + node);
        }

        assert_eq!(heap.weight(1), 2 + 1);
        assert_eq!(heap.weight(2), i32::MAX);
        assert_eq!(heap.weight(3), i32::MAX);
        assert_eq!(heap.weight(4), 2 + 4);
        assert_eq!(heap.weight(5), 2 + 5);
        assert_eq!(heap.weight(6), 2 + 6);
        assert_eq!(heap.weight(7), 2 + 7);
    }

    #[test]
    fn decrease_key() {
        let mut heap = Heap::default();
        let input = vec![4, 1, 6, 7, 5];

        for i in &input {
            heap.insert(*i, 2 + *i, *i);
        }
        assert_eq!(1, heap.min());
        assert!(!heap.is_empty());
        assert_eq!(5, heap.len());

        for i in &input {
            heap.decrease_key(*i, *i);
        }

        assert_eq!(heap.weight(1), 1);
        assert_eq!(heap.weight(2), i32::MAX);
        assert_eq!(heap.weight(3), i32::MAX);
        assert_eq!(heap.weight(4), 4);
        assert_eq!(heap.weight(5), 5);
        assert_eq!(heap.weight(6), 6);
        assert_eq!(heap.weight(7), 7);
    }

    #[test]
    fn decrease_key_with_new_data() {
        let mut heap = Heap::default();
        let input = vec![4, 1, 6, 7, 5];

        for i in &input {
            heap.insert(*i, 2 + *i, *i);
        }
        assert_eq!(heap.inserted_len(), input.len());
        assert_eq!(1, heap.min());
        assert!(!heap.is_empty());
        assert_eq!(5, heap.len());

        for i in &input {
            heap.decrease_key_and_update_data(*i, *i, i + 10);
        }

        assert_eq!(heap.weight(1), 1);
        assert_eq!(*heap.data(1), 11);
        assert_eq!(heap.weight(2), i32::MAX);
        assert_eq!(heap.weight(3), i32::MAX);
        assert_eq!(heap.weight(4), 4);
        assert_eq!(*heap.data(4), 14);
        assert_eq!(heap.weight(5), 5);
        assert_eq!(*heap.data(5), 15);
        assert_eq!(heap.weight(6), 6);
        assert_eq!(*heap.data(6), 16);
        assert_eq!(heap.weight(7), 7);
        assert_eq!(*heap.data(7), 17);
    }

    #[test]
    fn contains() {
        let mut heap = Heap::default();
        let input = vec![4, 1, 6, 7, 5];

        for i in &input {
            heap.insert(*i, *i, *i);
        }

        // never inserted
        assert!(!heap.contains(16));

        // rebind list of input values as mutable
        let mut input = input;
        input.sort();
        // rebind list as unmutable again (for good measure)
        let input = input;

        for i in &input {
            assert!(heap.contains(*i));
            let removed = heap.delete_min();
            assert_eq!(removed, *i);
            assert!(!heap.contains(*i));
        }
    }

    #[test]
    fn test_pop() {
        let mut heap = Heap::new();

        // Test empty heap
        assert_eq!(heap.pop(), None);

        // Test with multiple elements
        heap.insert(3, 3, 0);
        heap.insert(1, 1, 0);
        heap.insert(2, 2, 0);

        assert_eq!(heap.pop(), Some(1)); // Should return min without removing
        assert_eq!(heap.len(), 3); // Length shouldn't change
        assert_eq!(heap.pop(), Some(1)); // Should still return same min

        heap.delete_min(); // Actually remove the minimum
        assert_eq!(heap.pop(), Some(2)); // Should now return new minimum
        assert_eq!(heap.len(), 2); // Length should be reduced
    }

    #[test]
    fn a_lowered_key_comes_out_first() {
        let mut heap = Heap::new();
        heap.insert(1, 10, 0);
        heap.insert(2, 20, 0);
        heap.insert(3, 30, 0);

        heap.decrease_key(3, 1);

        assert_eq!(heap.weight(3), 1);
        assert_eq!(heap.delete_min(), 3, "the lowered key did not rise");
        assert_eq!(heap.delete_min(), 1);
        assert_eq!(heap.delete_min(), 2);
    }

    #[test]
    fn lowering_keys_leaves_the_heap_in_order() {
        // every element is lowered once, in an order that has each of them
        // move a different distance, and the heap has to hand them back
        // smallest first all the same
        let mut heap = Heap::new();
        for node in 0..32_i32 {
            heap.insert(node, node * 7 % 32, 0);
        }
        for node in 0..32_i32 {
            let lowered = heap.weight(node) / 2;
            heap.decrease_key(node, lowered);
            assert_eq!(heap.weight(node), lowered);
        }

        let mut last = 0;
        for _ in 0..32 {
            let node = heap.min();
            let weight = heap.weight(node);
            assert!(weight >= last, "{weight} came out after {last}");
            last = weight;
            heap.delete_min();
        }
        assert!(heap.is_empty());
    }
}
