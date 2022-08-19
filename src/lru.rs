use std::{collections::HashMap, hash::Hash};

use crate::linked_list::{LinkedList, ListNode};

/// A simple LRU implementation for a fixed size data set that avoid reallocation memory
/// Instead of using a standard linked list with stable iterators, it us

pub struct LRU<Key: Eq + Hash, Value, const N: usize> {
    storage: LinkedList<Value>,
    indices: HashMap<Key, ListNode<Value>>,
}

impl<Key: Eq + Hash, Value, const N: usize> LRU<Key, Value, N> {
    pub fn new() -> Self {
        let storage = LinkedList::new();
        let indices = HashMap::new();
        LRU { storage, indices }
    }

    pub fn push(&mut self, key: &Key, value: &Value) {
        debug_assert!(self.storage.len() <= N);
        // self.storage.push(data);
        if self.storage.len() == N {
            // evict an element
        } else {

        }
    }

    pub fn contains(&self, key: &Key) -> bool {
        self.indices.contains_key(key)
    }

    // pub fn get(&self, key: &Key) -> &Value {

    // }

    pub fn capacity(&self) -> usize {
        N
    }

    pub fn len(&self) -> usize {
        assert_eq!(self.storage.len(), self.indices.len());
        self.storage.len()
    }
}

#[cfg(test)]
mod tests {
    use super::LRU;

    struct SomeTestStruct {}

    #[test]
    pub fn construct() {
        let mut lru: LRU<_, _, 10> = LRU::new();
        assert_eq!(0, lru.len());
        lru.push(&123, &SomeTestStruct{});
        assert_eq!(10, lru.capacity());
    }
}
