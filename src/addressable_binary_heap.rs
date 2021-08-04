use core::hash::Hash;
use std::{collections::HashMap, fmt::Debug, usize};

use num::{Bounded, Integer};

struct HeapNode<NodeID: Copy + Integer, Weight: Bounded + Copy + Integer + Debug, Data> {
    node: NodeID,
    key: usize,
    weight: Weight,
    data: Data,
}

impl<NodeID: Copy + Integer, Weight: Bounded + Copy + Integer + Debug, Data> HeapNode<NodeID, Weight, Data> {
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

pub struct AddressableHeap<NodeID: Copy + Integer, Weight: Bounded + Copy + Integer + Debug, Data> {
    heap: Vec<HeapElement<Weight>>,
    inserted_nodes: Vec<HeapNode<NodeID, Weight, Data>>,
    node_index: HashMap<NodeID, usize>,
}

impl<NodeID: Copy + Hash + Integer, Weight: Bounded + Copy + Integer + Debug, Data>
    AddressableHeap<NodeID, Weight, Data>
{
    pub fn new() -> AddressableHeap<NodeID, Weight, Data> {
        AddressableHeap {
            heap: vec![HeapElement::default()].to_vec(),
            inserted_nodes: Vec::new(),
            node_index: HashMap::new(),
        }
    }

    pub fn clear(&mut self) {
        let def = HeapElement::default();
        self.heap.resize(1, def);
        self.inserted_nodes.clear();
        self.heap[1].weight = Weight::max_value();
    }

    pub fn len(&self) -> usize {
        self.heap.len() - 1
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn insert(&mut self, node: NodeID, weight: Weight, data: Data) {
        println!("insert weight {:?}", weight);
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

    pub fn key(&self, node: NodeID) -> Weight {
        let index = self.node_index.get(&node).unwrap();
        self.inserted_nodes.get(*index).unwrap().weight
    }

    pub fn removed(&self, node: NodeID) -> bool {
        let index = self.node_index.get(&node).unwrap();
        self.inserted_nodes.get(*index).unwrap().key == 0
    }

    pub fn inserted(&self, node: NodeID) -> bool {
        let index = *self.node_index.get(&node).unwrap();
        if index >= self.inserted_nodes.len() {
            return false;
        }
        self.inserted_nodes.get(index).unwrap().node == node
    }

    pub fn min(&self) -> NodeID {
        let index = self.heap[1].index;
        self.inserted_nodes[index].node
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
        self.inserted_nodes[removed_index].node
    }

    pub fn flush(&mut self) {
        for i in (self.heap.len() - 1)..1 {
            let element = &self.heap[i];
            self.inserted_nodes[element.index].key = 0;
        }
        self.heap.resize(1, HeapElement::default());
        self.heap[0].weight = Weight::max_value();
    }

    pub fn decrease_key(&mut self, node: NodeID, weight: Weight) {
        let index = self.node_index[&node];
        let key = self.inserted_nodes[index].key;

        self.inserted_nodes[index].weight = weight;
        self.up_heap(key);
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
        println!("upheap(key: {})", key);
        let rising_index = self.heap[key].index;
        println!("rising index: {}", rising_index);
        let weight = self.heap[key].weight;
        println!("weight: {:?}", weight);

        let mut next_key = key >> 1;

        while self.heap[next_key].weight > weight {
            println!("next_key: {}", next_key);
            self.heap[key] = self.heap[next_key];
            let index = self.heap[key].index;
            println!("index: {}", index);

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
    use rand::{prelude::StdRng, Rng, SeedableRng};

    use crate::addressable_binary_heap::AddressableHeap;
    type Heap = AddressableHeap<i32, i32, i32>;

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
            println!("insert {} {} {}", *i, *i, 0);
            heap.insert(*i, *i, 0);
        }
        assert_eq!(1, heap.min());
        assert!(!heap.is_empty());

        let mut result = Vec::new();
        while !heap.is_empty() {
            result.push(heap.delete_min());
        }
        println!("{:?}", result);
        assert_eq!(result.len(), 5);
        assert!(heap.is_empty());

        input.sort();
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
        let mut rng = StdRng::seed_from_u64(0xAAaaAAaa);
        let mut input = Vec::new();

        for _ in 0..1000 {
            let number = rng.gen();
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

        input.sort();
        assert_eq!(result, input);
    }
}
