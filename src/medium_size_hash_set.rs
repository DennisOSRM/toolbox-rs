use std::num::Wrapping;

use crate::fast_hash_trait::{FastHash, MAX_ELEMENTS};

/// A cell in the hash set that stores a key and a timestamp.
///
/// The timestamp is used to determine if the cell is occupied in the current generation
/// of the hash set, avoiding the need to clear all cells when the set is cleared.
#[derive(Debug, Clone)]
pub struct HashCell<Key> {
    time: u32,
    key: Key,
}

impl<Key: Default> Default for HashCell<Key> {
    /// Creates a new `HashCell` with the maximum possible timestamp (empty state)
    /// and the default value for the key type.
    fn default() -> Self {
        Self {
            time: u32::MAX,
            key: Key::default(),
        }
    }
}

/// A hash set implementation optimized for medium-sized collections with fast lookup and insertion.
///
/// This implementation uses open addressing with linear probing for collision resolution
/// and a timestamp-based clearing mechanism for efficient reuse. The set can store up to
/// `MAX_ELEMENTS` items and requires keys to be convertible to `u32`.
///
/// # Type Parameters
///
/// * `Key` - The type of elements stored in the set. Must be `Copy`, `Default`, `PartialEq` and convertible to `u32`
/// * `Hash` - The hash function implementation that must implement `FastHash`
#[derive(Debug)]
pub struct MediumSizeHashSet<Key, Hash: FastHash> {
    positions: Vec<HashCell<Key>>,
    hasher: Hash,
    current_timestamp: Wrapping<u32>,
    length: usize,
}

impl<Key, Hash: FastHash + Default> Default for MediumSizeHashSet<Key, Hash>
where
    Key: Copy + Default + PartialEq + TryInto<u32>,
{
    /// Creates a new empty hash set with default parameters.
    fn default() -> Self {
        Self::new()
    }
}

impl<Key, Hash: FastHash + Default> MediumSizeHashSet<Key, Hash>
where
    Key: Copy + Default + PartialEq + TryInto<u32>,
{
    /// Creates a new empty hash set.
    pub fn new() -> Self {
        Self {
            positions: vec![HashCell::default(); MAX_ELEMENTS],
            hasher: Hash::default(),
            current_timestamp: Wrapping(0),
            length: 0,
        }
    }

    /// Inserts a key into the hash set.
    ///
    /// If the key already exists in the set, it will be overwritten without changing the size.
    /// Uses linear probing to resolve collisions.
    ///
    /// # Panics
    ///
    /// Panics if the key cannot be converted to u32.
    #[inline]
    pub fn push(&mut self, key: Key) {
        let key_as_u32: u32 = key
            .try_into()
            .unwrap_or_else(|_| panic!("Key must be convertible to u32"));
        let mut position = self.hasher.hash(key_as_u32) as usize;

        while self.positions[position].time == self.current_timestamp.0
            && self.positions[position].key != key
        {
            position = (position + 1) % MAX_ELEMENTS;
        }

        let cell = &mut self.positions[position];
        if cell.time != self.current_timestamp.0 {
            // New cell, increment length
            self.length += 1;
        }
        // Update timestamp and key
        cell.time = self.current_timestamp.0;
        cell.key = key;
    }

    /// Returns true if the set contains the specified key.
    ///
    /// # Panics
    ///
    /// Panics if the key cannot be converted to u32.
    pub fn contains(&self, key: Key) -> bool {
        let key_as_u32: u32 = key
            .try_into()
            .unwrap_or_else(|_| panic!("Key must be convertible to u32"));
        let mut position = self.hasher.hash(key_as_u32) as usize;

        while self.positions[position].time == self.current_timestamp.0
            && self.positions[position].key != key
        {
            position = (position + 1) % MAX_ELEMENTS;
        }

        if self.positions[position].time == self.current_timestamp.0 {
            return true;
        }
        false
    }

    /// Clears the set, removing all elements.
    ///
    /// This operation is O(1) as it simply increments an internal timestamp.
    /// When the timestamp wraps around to 0, the entire set is reallocated.
    pub fn clear(&mut self) {
        if self.length == 0 {
            return;
        }
        // Increment the timestamp to mark all current elements as "old"
        // and reset the length to 0.
        // This allows for efficient reuse of the hash table without needing to clear all cells.
        // When the timestamp wraps around, we reallocate the positions vector.
        self.current_timestamp += Wrapping(1);
        self.length = 0;

        if self.current_timestamp.0 == 0 {
            self.positions = vec![HashCell::default(); MAX_ELEMENTS];
        }
    }

    /// Returns the number of elements in the set.
    #[inline]
    pub fn len(&self) -> usize {
        self.length
    }

    /// Returns true if the set contains no elements.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    /// Returns the maximum capacity of the set.
    #[inline]
    pub fn capacity(&self) -> usize {
        MAX_ELEMENTS
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fibonacci_hash::FibonacciHash;

    #[test]
    fn test_new_set() {
        let set: MediumSizeHashSet<u32, FibonacciHash> = MediumSizeHashSet::new();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
        assert_eq!(set.capacity(), MAX_ELEMENTS);
    }

    #[test]
    fn test_push_and_contains() {
        let mut set = MediumSizeHashSet::<u32, FibonacciHash>::new();

        // Test single insertion
        set.push(42);
        assert!(set.contains(42));
        assert!(!set.contains(43));
        assert_eq!(set.len(), 1);

        // Test multiple insertions
        set.push(100);
        set.push(200);
        assert!(set.contains(100));
        assert!(set.contains(200));
        assert_eq!(set.len(), 3);

        // Test duplicate insertion
        set.push(42);
        assert_eq!(set.len(), 3); // Length shouldn't change for duplicates
    }

    #[test]
    fn test_clear() {
        let mut set = MediumSizeHashSet::<u32, FibonacciHash>::new();

        set.push(1);
        set.push(2);
        set.push(3);
        assert_eq!(set.len(), 3);

        set.clear();
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
        assert!(!set.contains(1));
        assert!(!set.contains(2));
        assert!(!set.contains(3));
    }

    #[test]
    fn test_timestamp_wraparound() {
        let mut set = MediumSizeHashSet::<u32, FibonacciHash>::new();

        // Force timestamp to near maximum
        set.current_timestamp = Wrapping(u32::MAX - 1);

        set.push(1);
        assert!(set.contains(1));

        // This should handle wraparound by reallocating
        set.clear();
        set.push(3);
        assert!(set.contains(3));
        set.clear();

        assert!(!set.contains(1));
        assert!(!set.contains(3));
        assert_eq!(set.current_timestamp.0, 0);
    }

    #[test]
    fn test_linear_probing() {
        let mut set = MediumSizeHashSet::<u32, FibonacciHash>::new();

        // Insert enough elements to force some collisions
        for i in 0..100 {
            set.push(i);
            assert!(set.contains(i));
        }

        // Verify all elements are still accessible
        for i in 0..100 {
            assert!(set.contains(i));
        }
    }

    #[test]
    #[should_panic(expected = "Key must be convertible to u32")]
    fn test_invalid_key_conversion() {
        let mut set = MediumSizeHashSet::<i32, FibonacciHash>::new();
        // This should panic when trying to convert to u32
        set.push(i32::MIN);
    }

    #[test]
    fn test_empty_operations() {
        let set = MediumSizeHashSet::<u32, FibonacciHash>::new();
        assert!(set.is_empty());
        assert!(!set.contains(0));
        assert_eq!(set.len(), 0);
    }

    #[test]
    fn test_contains_after_clear() {
        let mut set = MediumSizeHashSet::<u32, FibonacciHash>::new();

        set.push(42);
        assert!(set.contains(42));

        set.clear();
        assert!(!set.contains(42));

        // Add new element after clear
        set.push(43);
        assert!(set.contains(43));
        assert!(!set.contains(42));
    }

    #[test]
    fn test_hash_cell_default() {
        let cell: HashCell<u32> = HashCell::default();
        assert_eq!(cell.time, u32::MAX);
        assert_eq!(cell.key, 0); // u32's default is 0
    }

    #[test]
    fn test_medium_size_hash_set_default() {
        let set: MediumSizeHashSet<u32, FibonacciHash> = MediumSizeHashSet::default();

        // Should match behavior of new()
        assert!(set.is_empty());
        assert_eq!(set.len(), 0);
        assert_eq!(set.capacity(), MAX_ELEMENTS);

        // Test internal state
        assert_eq!(set.current_timestamp.0, 0);
        assert_eq!(set.positions.len(), MAX_ELEMENTS);

        // Verify positions are initialized with default HashCells
        assert!(set.positions.iter().all(|cell| cell.time == u32::MAX));
        assert!(set.positions.iter().all(|cell| cell.key == 0));
    }

    #[test]
    fn test_default_vs_new_equivalence() {
        let default_set: MediumSizeHashSet<u32, FibonacciHash> = MediumSizeHashSet::default();
        let new_set: MediumSizeHashSet<u32, FibonacciHash> = MediumSizeHashSet::new();

        // Test basic properties are equivalent
        assert_eq!(default_set.len(), new_set.len());
        assert_eq!(default_set.capacity(), new_set.capacity());
        assert_eq!(default_set.current_timestamp.0, new_set.current_timestamp.0);

        // Insert same elements in both sets
        let mut default_set = default_set;
        let mut new_set = new_set;

        default_set.push(1);
        new_set.push(1);

        assert_eq!(default_set.contains(1), new_set.contains(1));
        assert_eq!(default_set.len(), new_set.len());
    }

    #[test]
    fn test_contains_early_return() {
        let mut set = MediumSizeHashSet::<u32, FibonacciHash>::new();

        // Insert a value first
        set.push(42);

        // Now insert a value that will cause a collision with 42
        // We know FibonacciHash uses the golden ratio and shifts by 16,
        // so we can create a collision by using values that will generate
        // the same hash after the multiplication and shifting
        set.push(42 + MAX_ELEMENTS as u32);

        // This will hash to the same initial position as 42,
        // but since 42 is already there, it will probe to the next position
        assert!(set.contains(42));
        assert!(set.contains(42 + MAX_ELEMENTS as u32));

        // Verify we have 2 elements (proving they didn't overwrite each other)
        assert_eq!(set.len(), 2);
    }
}
