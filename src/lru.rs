use crate::linked_list::{LinkedList, ListCursor};
use std::{collections::HashMap, fmt::Debug, hash::Hash};

/// A simple LRU (Least Recently Used) cache implementation with fixed capacity.
///
/// This implementation uses a combination of a linked list for maintaining access order
/// and a hash map for O(1) lookups. The cache has a fixed capacity and automatically
/// evicts the least recently used items when full.
///
/// # Type Parameters
///
/// * `Key`: The type of keys used in the cache. Must implement `Copy`, `Debug`, `Eq`, and `Hash`.
/// * `Value`: The type of values stored in the cache.
///
/// # Examples
///
/// ```
/// use toolbox_rs::lru::LRU;
///
/// // Create a new LRU cache with capacity 2
/// let mut cache = LRU::new_with_capacity(2);
///
/// // Add some items
/// cache.push(&1, "one");
/// cache.push(&2, "two");
///
/// assert_eq!(cache.get(&1), Some(&"one"));
/// assert_eq!(cache.len(), 2);
///
/// // Adding another item will evict the least recently used item (2)
/// cache.push(&3, "three");
/// assert!(!cache.contains(&2));
/// assert!(cache.contains(&1));
/// assert!(cache.contains(&3));
/// ```
pub struct LRU<Key: Copy + Debug + Eq + Hash, Value> {
    lru_list: LinkedList<(Key, Value)>,
    access_map: HashMap<Key, ListCursor<(Key, Value)>>,
    capacity: usize,
}

impl<Key: Copy + Debug + Eq + Hash, Value> LRU<Key, Value> {
    /// Creates a new LRU cache with the specified capacity.
    ///
    /// # Arguments
    ///
    /// * `capacity` - The maximum number of key-value pairs the cache can hold
    ///
    /// # Panics
    ///
    /// Panics in debug mode if capacity is 0.
    ///
    /// # Examples
    ///
    /// ```
    /// use toolbox_rs::lru::LRU;
    ///
    /// let cache: LRU<i32, &str> = LRU::new_with_capacity(10);
    /// assert_eq!(cache.capacity(), 10);
    /// assert!(cache.is_empty());
    /// ```
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

    /// Inserts a key-value pair into the cache.
    ///
    /// If the cache is at capacity, the least recently used item will be evicted.
    /// If the key already exists, the value will be updated and the entry will
    /// become the most recently used.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to insert
    /// * `value` - The value to associate with the key
    ///
    /// # Examples
    ///
    /// ```
    /// use toolbox_rs::lru::LRU;
    ///
    /// let mut cache = LRU::new_with_capacity(2);
    /// cache.push(&1, "one");
    /// cache.push(&2, "two");
    /// cache.push(&3, "three"); // This will evict key 1
    ///
    /// assert!(!cache.contains(&1));
    /// assert!(cache.contains(&2));
    /// assert!(cache.contains(&3));
    /// ```
    pub fn push(&mut self, key: &Key, value: Value) {
        debug_assert!(self.lru_list.len() <= self.capacity);

        if let Some(handle) = self.access_map.get(key) {
            // Key exists - move to front and update value
            let handle = handle.clone();
            self.lru_list.move_to_front(&handle);
            // Update the value using a mutable reference
            let front = self.lru_list.get_front_mut();
            *front = (*key, value);
            return;
        }

        // Key doesn't exist - handle capacity and insert new entry
        if self.access_map.len() == self.capacity {
            // evict least recently used element
            debug_assert!(!self.access_map.is_empty());
            if let Some((evicted_key, _)) = self.lru_list.pop_back() {
                self.access_map.remove(&evicted_key);
            }
        }

        // Insert new entry
        let handle = self.lru_list.push_front((*key, value));
        self.access_map.insert(*key, handle);
    }

    /// Returns true if the cache contains the specified key.
    ///
    /// This operation does not affect the order of items in the cache.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to look up
    ///
    /// # Examples
    ///
    /// ```
    /// use toolbox_rs::lru::LRU;
    ///
    /// let mut cache = LRU::new_with_capacity(1);
    /// cache.push(&1, "one");
    /// assert!(cache.contains(&1));
    /// assert!(!cache.contains(&2));
    /// ```
    pub fn contains(&self, key: &Key) -> bool {
        self.access_map.contains_key(key)
    }

    /// Gets the value associated with the key and marks it as most recently used.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to look up
    ///
    /// # Returns
    ///
    /// Returns `Some(&Value)` if the key exists, or `None` if it doesn't.
    ///
    /// # Examples
    ///
    /// ```
    /// use toolbox_rs::lru::LRU;
    ///
    /// let mut cache = LRU::new_with_capacity(2);
    /// cache.push(&1, "one");
    /// cache.push(&2, "two");
    ///
    /// assert_eq!(cache.get(&1), Some(&"one"));
    /// cache.push(&3, "three"); // This will evict key 2, not 1
    /// assert!(cache.contains(&1)); // 1 was most recently used
    /// assert!(!cache.contains(&2)); // 2 was evicted
    /// ```
    pub fn get(&mut self, key: &Key) -> Option<&Value> {
        if let Some(handle) = self.access_map.get(key) {
            self.lru_list.move_to_front(handle);
            return Some(&self.lru_list.get_front().1);
        }
        None
    }

    /// Returns a reference to the most recently used (front) item in the cache.
    ///
    /// # Returns
    ///
    /// Returns `Option<(&Key, &Value)>` - `Some` with references to the key and value if the cache
    /// is not empty, or `None` if the cache is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use toolbox_rs::lru::LRU;
    ///
    /// let mut cache = LRU::new_with_capacity(2);
    /// assert_eq!(cache.get_front(), None);
    ///
    /// cache.push(&1, "one");
    /// cache.push(&2, "two");
    /// assert_eq!(cache.get_front(), Some((&2, &"two")));
    /// ```
    pub fn get_front(&self) -> Option<(&Key, &Value)> {
        if self.is_empty() {
            None
        } else {
            let front = self.lru_list.get_front();
            Some((&front.0, &front.1))
        }
    }

    /// Returns a mutable reference to the most recently used (front) item in the cache.
    ///
    /// # Returns
    ///
    /// Returns `Option<(&Key, &mut Value)>` - `Some` with a reference to the key and mutable reference
    /// to the value if the cache is not empty, or `None` if the cache is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use toolbox_rs::lru::LRU;
    ///
    /// let mut cache = LRU::new_with_capacity(2);
    /// cache.push(&1, String::from("one"));
    /// cache.push(&2, String::from("two"));
    ///
    /// if let Some((_, value)) = cache.get_front_mut() {
    ///     *value = String::from("TWO");
    /// }
    /// assert_eq!(cache.get(&2), Some(&String::from("TWO")));
    /// ```
    pub fn get_front_mut(&mut self) -> Option<(&Key, &mut Value)> {
        if self.is_empty() {
            None
        } else {
            let front = self.lru_list.get_front_mut();
            Some((&front.0, &mut front.1))
        }
    }

    /// Returns the maximum number of items the cache can hold.
    ///
    /// # Examples
    ///
    /// ```
    /// use toolbox_rs::lru::LRU;
    ///
    /// let cache: LRU<i32, &str> = LRU::new_with_capacity(5);
    /// assert_eq!(cache.capacity(), 5);
    /// ```
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Returns the current number of items in the cache.
    ///
    /// # Examples
    ///
    /// ```
    /// use toolbox_rs::lru::LRU;
    ///
    /// let mut cache = LRU::new_with_capacity(2);
    /// assert_eq!(cache.len(), 0);
    ///
    /// cache.push(&1, "one");
    /// assert_eq!(cache.len(), 1);
    /// ```
    pub fn len(&self) -> usize {
        assert_eq!(self.lru_list.len(), self.access_map.len());
        self.lru_list.len()
    }

    /// Returns true if the cache contains no items.
    ///
    /// # Examples
    ///
    /// ```
    /// use toolbox_rs::lru::LRU;
    ///
    /// let mut cache = LRU::new_with_capacity(2);
    /// assert!(cache.is_empty());
    ///
    /// cache.push(&1, "one");
    /// assert!(!cache.is_empty());
    ///
    /// cache.clear();
    /// assert!(cache.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Removes all items from the cache.
    ///
    /// # Examples
    ///
    /// ```
    /// use toolbox_rs::lru::LRU;
    ///
    /// let mut cache = LRU::new_with_capacity(2);
    /// cache.push(&1, "one");
    /// cache.push(&2, "two");
    ///
    /// cache.clear();
    /// assert!(cache.is_empty());
    /// assert_eq!(cache.len(), 0);
    /// ```
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
    fn construct() {
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

    #[test]
    fn clear_is_empty() {
        let mut lru = LRU::new_with_capacity(10);
        for i in [0, 1, 2, 3] {
            lru.push(&i, 2 * i);
        }
        assert!(!lru.is_empty());
        lru.clear();
        assert!(lru.is_empty());
    }

    #[test]
    fn test_update_existing_key() {
        let mut lru = LRU::new_with_capacity(2);
        lru.push(&1, "one");
        lru.push(&2, "two");

        // Update existing key
        lru.push(&1, "ONE");
        assert_eq!(lru.get(&1), Some(&"ONE"));
        assert_eq!(lru.len(), 2);
    }

    #[test]
    fn test_get_updates_order() {
        let mut lru = LRU::new_with_capacity(3);
        lru.push(&1, "one");
        lru.push(&2, "two");
        lru.push(&3, "three");

        // Access 1, making it most recently used
        assert_eq!(lru.get(&1), Some(&"one"));

        // Add new item - should evict 2, not 1
        lru.push(&4, "four");
        assert!(lru.contains(&1));
        assert!(!lru.contains(&2));
        assert!(lru.contains(&3));
        assert!(lru.contains(&4));
    }

    #[test]
    fn test_capacity_bounds() {
        let mut lru = LRU::new_with_capacity(1);
        lru.push(&1, "one");
        assert_eq!(lru.len(), 1);
        assert!(lru.contains(&1));

        lru.push(&2, "two");
        assert_eq!(lru.len(), 1);
        assert!(!lru.contains(&1));
        assert!(lru.contains(&2));
    }

    #[test]
    fn test_clear_retains_capacity() {
        let mut lru = LRU::new_with_capacity(5);
        for i in 0..5 {
            lru.push(&i, i.to_string());
        }
        assert_eq!(lru.len(), 5);

        lru.clear();
        assert_eq!(lru.len(), 0);
        assert_eq!(lru.capacity(), 5);

        // Can still add items up to capacity
        for i in 0..5 {
            lru.push(&i, i.to_string());
        }
        assert_eq!(lru.len(), 5);
    }

    #[test]
    fn test_get_front_mut() {
        let mut lru = LRU::new_with_capacity(3);
        assert_eq!(lru.get_front_mut(), None);

        lru.push(&1, String::from("one"));
        lru.push(&2, String::from("two"));

        // Test mutating the front element
        if let Some((key, value)) = lru.get_front_mut() {
            assert_eq!(key, &2);
            assert_eq!(value, "two");
            *value = String::from("TWO");
        }

        // Verify the change was applied
        assert_eq!(lru.get(&2), Some(&String::from("TWO")));

        // Add another element and verify front changes
        lru.push(&3, String::from("three"));
        if let Some((key, value)) = lru.get_front_mut() {
            assert_eq!(key, &3);
            assert_eq!(value, "three");
            *value = String::from("THREE");
        }

        // Verify both modified values are still correct
        assert_eq!(lru.get(&2), Some(&String::from("TWO")));
        assert_eq!(lru.get(&3), Some(&String::from("THREE")));
    }
}
