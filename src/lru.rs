use crate::linked_list::{LinkedList, ListCursor};
use std::{collections::HashMap, fmt::Debug, hash::Hash};

/// A simple LRU implementation for a fixed size data set that avoid reallocation memory
/// Instead of using a standard linked list with stable iterators, it us
pub struct LRU<Key: Copy + Debug + Eq + Hash, Value> {
    lru_list: LinkedList<(Key, Value)>,
    access_map: HashMap<Key, ListCursor<(Key, Value)>>,
    capacity: usize,
}

impl<Key: Copy + Debug + Eq + Hash, Value> LRU<Key, Value> {
    pub fn new_with_capacity(capacity: usize) -> Self {
        debug_assert!(capacity > 0);
        let storage = LinkedList::new();
        let indices = HashMap::with_capacity(capacity);
        LRU {
            lru_list: storage,
            access_map: indices,
            capacity,
        }
    }

    pub fn push(&mut self, key: &Key, value: Value) {
        debug_assert!(self.lru_list.len() <= self.capacity);
        debug_assert!(!self.access_map.contains_key(key));
        if self.access_map.len() == self.capacity {
            // evict an element
            debug_assert!(!self.access_map.is_empty());
            let evicted = self.lru_list.pop_back();
            let evicted_key = &evicted.unwrap().0;
            self.access_map.remove(evicted_key);
        }
        let handle = self.lru_list.push_front((*key, value));
        self.access_map.insert(*key, handle);
    }

    pub fn contains(&self, key: &Key) -> bool {
        self.access_map.contains_key(key)
    }

    pub fn get(&mut self, key: &Key) -> Option<&Value> {
        if let Some(handle) = self.access_map.get(key) {
            self.lru_list.move_to_front(handle);
            return Some(&self.lru_list.get_front().1);
        }
        None
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn len(&self) -> usize {
        assert_eq!(self.lru_list.len(), self.access_map.len());
        self.lru_list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn clear(&mut self) {
        self.lru_list.clear();
        self.access_map.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::LRU;

    struct SomeTestStruct(i32);

    #[test]
    pub fn construct() {
        let mut lru = LRU::new_with_capacity(10);
        assert_eq!(0, lru.len());
        lru.push(&1, SomeTestStruct(1));
        assert_eq!(1, lru.len());
        lru.push(&2, SomeTestStruct(2));
        assert_eq!(2, lru.len());
        lru.push(&3, SomeTestStruct(3));
        assert_eq!(3, lru.len());
        lru.push(&4, SomeTestStruct(4));
        assert_eq!(4, lru.len());
        lru.push(&5, SomeTestStruct(5));
        assert_eq!(5, lru.len());
        lru.push(&6, SomeTestStruct(6));
        assert_eq!(6, lru.len());
        lru.push(&7, SomeTestStruct(7));
        assert_eq!(7, lru.len());
        lru.push(&8, SomeTestStruct(8));
        assert_eq!(8, lru.len());
        lru.push(&9, SomeTestStruct(9));
        assert_eq!(9, lru.len());
        lru.push(&10, SomeTestStruct(10));
        assert_eq!(10, lru.len());
        assert_eq!(10, lru.capacity());

        // access 1, make 2 the oldest element now
        let handle = lru.get(&1).unwrap();
        assert_eq!(1, handle.0);
        // add 11, evict 2
        lru.push(&11, SomeTestStruct(11));
        assert!(!lru.contains(&2));
        assert_eq!(lru.len(), 10);

        // get handle for evicted element and verify it is None
        let handle = lru.get(&2);
        assert!(handle.is_none());

        // assert that all other elements are still cached
        let keys = vec![1, 3, 4, 5, 6, 7, 8, 9, 10, 11];
        keys.into_iter().for_each(|key| {
            assert!(lru.contains(&key));
        });
    }
}
