use std::num::Wrapping;

use crate::fast_hash_trait::{FastHash, MAX_ELEMENTS};

/// A hash table using tabulation hashing for fast lookups with linear probing
/// for collision resolution.
///
/// # Type Parameters
///
/// * `Key` - Type of keys (must be convertible to u32)
/// * `Value` - Type of stored values
///
/// # Performance
///
/// - O(1) average case for insertions and lookups
/// - O(1) average case for clear operation using timestamp-based invalidation
/// - Space complexity: O(MAX_ELEMENTS)
///
/// A hash cell storing key, value, and timestamp for collision resolution.
///
/// Used internally by TabulationHashTable to store elements and handle
/// linear probing during collision resolution.
#[derive(Debug, Clone)]
pub struct HashCell<Key, Value> {
    /// Timestamp for marking cell validity
    time: u32,
    key: Key,
    value: Value,
}

impl<Key: Default, Value: Default> Default for HashCell<Key, Value> {
    fn default() -> Self {
        Self {
            time: u32::MAX,
            key: Key::default(),
            value: Value::default(),
        }
    }
}

#[derive(Debug)]
pub struct MediumSizeHashTable<Key, Value, Hash: FastHash> {
    positions: Vec<HashCell<Key, Value>>,
    hasher: Hash,
    current_timestamp: Wrapping<u32>,
    length: usize,
}

impl<Key, Value, Hash: FastHash + Default> Default for MediumSizeHashTable<Key, Value, Hash>
where
    Key: Copy + Default + PartialEq + TryInto<u32>,
    Value: Copy + Default,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<Key, Value, Hash: FastHash + Default> MediumSizeHashTable<Key, Value, Hash>
where
    Key: Copy + Default + PartialEq + TryInto<u32>,
    Value: Copy + Default,
{
    /// Creates a new hash table with capacity for MAX_ELEMENTS items.
    ///
    /// # Implementation Details
    ///
    /// - Initializes empty table with default hash cells
    /// - Creates tabulation hasher for index computation
    /// - Sets initial timestamp to 0
    pub fn new() -> Self {
        Self {
            positions: vec![HashCell::default(); MAX_ELEMENTS],
            hasher: Hash::default(),
            current_timestamp: Wrapping(0),
            length: 0,
        }
    }

    /// Gets or creates a mutable reference to a hash cell for the given key.
    ///
    /// # Algorithm
    ///
    /// 1. Computes initial position using tabulation hash
    /// 2. Uses linear probing to handle collisions
    /// 3. Updates timestamp and key on access
    ///
    /// # Arguments
    ///
    /// * `key` - identifier
    ///
    /// # Returns
    ///
    /// Mutable reference to the hash cell
    ///
    /// # Panics
    ///
    /// Panics if `Key` cannot be converted to `u32`
    #[inline]
    pub fn get_mut(&mut self, key: Key) -> &mut Value {
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

        &mut cell.value
    }

    /// Inserts a value into the hash table at the specified key.
    ///
    /// This is a convenience wrapper around `get_mut()` that handles the assignment.
    /// It uses linear probing for collision resolution and automatically handles
    /// timestamp-based cell invalidation.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to insert the value at
    /// * `value` - The value to insert
    ///
    /// # Examples
    ///
    /// Basic insertion:
    /// ```
    /// use toolbox_rs::medium_size_hash_table::MediumSizeHashTable;
    /// use toolbox_rs::tabulation_hash::TabulationHash;
    ///
    /// let mut table = MediumSizeHashTable::<u32, u32, TabulationHash>::new();
    /// table.insert(1, 42);
    /// assert_eq!(table.peek_value(1), Some(&42));
    /// ```
    ///
    /// Updating existing values:
    /// ```
    /// use toolbox_rs::medium_size_hash_table::MediumSizeHashTable;
    /// use toolbox_rs::tabulation_hash::TabulationHash;
    ///
    /// let mut table = MediumSizeHashTable::<u32, u32, TabulationHash>::new();
    /// table.insert(1, 42);
    /// table.insert(1, 43);  // Updates the existing value
    /// assert_eq!(table.peek_value(1), Some(&43));
    /// ```
    ///
    /// Multiple insertions:
    /// ```
    /// use toolbox_rs::medium_size_hash_table::MediumSizeHashTable;
    /// use toolbox_rs::tabulation_hash::TabulationHash;
    ///
    /// let mut table = MediumSizeHashTable::<u32, u32, TabulationHash>::new();
    /// table.insert(1, 10);
    /// table.insert(2, 20);
    /// table.insert(3, 30);
    ///
    /// assert_eq!(table.peek_value(1), Some(&10));
    /// assert_eq!(table.peek_value(2), Some(&20));
    /// assert_eq!(table.peek_value(3), Some(&30));
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if `Key` cannot be converted to `u32`
    #[inline]
    pub fn insert(&mut self, key: Key, value: Value) {
        *self.get_mut(key) = value;
    }

    /// Looks up the value associated with a key without modifying the timestamp.
    ///
    /// # Arguments
    ///
    /// * `key` - key identifier to look up
    ///
    /// # Returns
    ///
    /// * `Some(value)` - If the key exists in the current timestamp
    /// * `None` - If the key doesn't exist or was cleared
    ///
    /// # Panics
    ///
    /// Panics if `Key` cannot be converted to `u32`
    pub fn peek_value(&self, key: Key) -> Option<&Value> {
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
            return Some(&self.positions[position].value);
        }
        None
    }

    /// Checks if a key exists in the hash table.
    ///
    /// This method performs a read-only lookup that doesn't modify the table's state.
    /// It uses the same linear probing strategy as other operations but doesn't update
    /// timestamps or modify any values.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to check for existence
    ///
    /// # Returns
    ///
    /// * `true` if the key exists in the current timestamp
    /// * `false` if the key doesn't exist or was cleared
    ///
    /// # Examples
    ///
    /// Basic usage:
    /// ```
    /// use toolbox_rs::medium_size_hash_table::MediumSizeHashTable;
    /// use toolbox_rs::tabulation_hash::TabulationHash;
    ///
    /// let mut table = MediumSizeHashTable::<u32, u32, TabulationHash>::new();
    /// assert!(!table.contains_key(1), "Empty table should not contain any keys");
    ///
    /// *table.get_mut(1) = 42;
    /// assert!(table.contains_key(1), "Key should exist after insertion");
    /// ```
    ///
    /// Behavior after clear:
    /// ```
    /// use toolbox_rs::medium_size_hash_table::MediumSizeHashTable;
    /// use toolbox_rs::tabulation_hash::TabulationHash;
    ///
    /// let mut table = MediumSizeHashTable::<u32, u32, TabulationHash>::new();
    /// *table.get_mut(1) = 42;
    /// table.clear();
    /// assert!(!table.contains_key(1), "Key should not exist after clear");
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if `Key` cannot be converted to `u32`
    pub fn contains_key(&self, key: Key) -> bool {
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

    /// Clears the hash table by incrementing the timestamp.
    ///
    /// If the timestamp would overflow, reallocates the table instead.
    /// This provides an efficient O(1) clear operation in most cases.
    pub fn clear(&mut self) {
        self.current_timestamp += Wrapping(1);
        self.length = 0;

        if self.current_timestamp.0 == 0 {
            self.positions = vec![HashCell::default(); MAX_ELEMENTS];
        }
    }

    /// Returns the number of elements currently in the hash table.
    ///
    /// This method returns the actual number of key-value pairs stored in the table,
    /// not the capacity. The count is maintained efficiently during insertions and clear
    /// operations.
    ///
    /// # Examples
    ///
    /// ```
    /// use toolbox_rs::medium_size_hash_table::MediumSizeHashTable;
    /// use toolbox_rs::tabulation_hash::TabulationHash;
    ///
    /// let mut table = MediumSizeHashTable::<u32, u32, TabulationHash>::new();
    /// assert_eq!(table.len(), 0);
    ///
    /// table.insert(1, 42);
    /// assert_eq!(table.len(), 1);
    ///
    /// // Update existing key doesn't change length
    /// table.insert(1, 43);
    /// assert_eq!(table.len(), 1);
    ///
    /// table.insert(2, 100);
    /// assert_eq!(table.len(), 2);
    ///
    /// table.clear();
    /// assert_eq!(table.len(), 0);
    /// ```
    #[inline]
    pub fn len(&self) -> usize {
        self.length
    }

    /// Returns true if the hash table contains no elements.
    ///
    /// # Examples
    ///
    /// ```
    /// use toolbox_rs::medium_size_hash_table::MediumSizeHashTable;
    /// use toolbox_rs::tabulation_hash::TabulationHash;
    ///
    /// let mut table = MediumSizeHashTable::<u32, u32, TabulationHash>::new();
    /// assert!(table.is_empty());
    ///
    /// table.insert(1, 42);
    /// assert!(!table.is_empty());
    ///
    /// table.clear();
    /// assert!(table.is_empty());
    /// ```
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.length == 0
    }

    /// Returns the total capacity of the hash table.
    ///
    /// The capacity is fixed at MAX_ELEMENTS (65536) and represents the maximum
    /// number of elements that can be stored in the table. This is different from
    /// `len()` which returns the current number of elements.
    ///
    /// # Examples
    ///
    /// ```
    /// use toolbox_rs::medium_size_hash_table::MediumSizeHashTable;
    /// use toolbox_rs::tabulation_hash::TabulationHash;
    ///
    /// let table = MediumSizeHashTable::<u32, u32, TabulationHash>::new();
    /// assert_eq!(table.capacity(), 65536);
    /// assert!(table.capacity() >= table.len());
    /// ```
    #[inline]
    pub fn capacity(&self) -> usize {
        MAX_ELEMENTS
    }
}

#[cfg(test)]
mod test {
    use std::num::Wrapping;

    use crate::{
        medium_size_hash_table::{FastHash, MAX_ELEMENTS, MediumSizeHashTable},
        tabulation_hash::TabulationHash,
    };

    #[test]
    fn test_hash_storage_basic() {
        let mut storage = MediumSizeHashTable::<u32, u32, TabulationHash>::new();

        // Insert and retrieve using new API
        *storage.get_mut(1) = 42;
        assert_eq!(storage.peek_value(1), Some(&42));

        // Update existing
        *storage.get_mut(1) = 43;
        assert_eq!(storage.peek_value(1), Some(&43));
    }

    #[test]
    fn test_hash_storage_collision() {
        let mut storage = MediumSizeHashTable::<u32, u32, TabulationHash>::new();

        // Insert two keys that might collide
        *storage.get_mut(1) = 42;
        *storage.get_mut(65537) = 43; // Could hash to same position

        assert_eq!(storage.peek_value(1), Some(&42));
        assert_eq!(storage.peek_value(65537), Some(&43));
    }

    #[test]
    fn test_hash_storage_clear() {
        let mut storage = MediumSizeHashTable::<u32, u32, TabulationHash>::new();

        *storage.get_mut(1) = 42;
        storage.clear();
        assert_eq!(storage.peek_value(1), None); // Default value after clear
    }

    #[test]
    fn test_linear_probing_sequence() {
        let mut storage = MediumSizeHashTable::<u32, u32, TabulationHash>::new();

        // Create a sequence of values that will hash to the same position
        let base_key = 42;
        let colliding_keys = [
            base_key,
            base_key + MAX_ELEMENTS as u32,
            base_key + (2 * MAX_ELEMENTS) as u32,
        ];

        // Insert values using new API
        for (i, &key) in colliding_keys.iter().enumerate() {
            *storage.get_mut(key) = i as u32;
        }

        // Verify each value is still accessible and in the correct position
        for (i, &key) in colliding_keys.iter().enumerate() {
            assert_eq!(
                storage.peek_value(key),
                Some(&(i as u32)),
                "Failed to retrieve value {} for key {} after linear probing",
                i,
                key
            );
        }

        // Update middle value and verify chain remains intact
        *storage.get_mut(colliding_keys[1]) = 42;

        assert_eq!(storage.peek_value(colliding_keys[0]), Some(&0));
        assert_eq!(storage.peek_value(colliding_keys[1]), Some(&42));
        assert_eq!(storage.peek_value(colliding_keys[2]), Some(&2));
    }

    #[test]
    fn test_linear_probing_loop() {
        let mut storage = MediumSizeHashTable::<u32, u32, TabulationHash>::new();

        // First, find two keys that hash to the same position
        let base_key = 0u32;
        let mut colliding_key = 1u32;

        while storage.hasher.hash(base_key) != storage.hasher.hash(colliding_key) {
            colliding_key += 1;
        }

        // Now we have two keys that will definitely collide
        *storage.get_mut(base_key) = 100;
        *storage.get_mut(colliding_key) = 200;

        // Find a third key that hashes to the same position
        let mut third_key = colliding_key + 1;
        while storage.hasher.hash(third_key) != storage.hasher.hash(base_key) {
            third_key += 1;
        }
        *storage.get_mut(third_key) = 300;

        // Verify the probing sequence
        assert_eq!(storage.peek_value(base_key), Some(&100));
        assert_eq!(storage.peek_value(colliding_key), Some(&200));
        assert_eq!(storage.peek_value(third_key), Some(&300));

        // Update middle key to ensure probing still works
        *storage.get_mut(colliding_key) = 250;

        // Verify entire chain is intact
        assert_eq!(storage.peek_value(base_key), Some(&100));
        assert_eq!(storage.peek_value(colliding_key), Some(&250));
        assert_eq!(storage.peek_value(third_key), Some(&300));
    }

    #[test]
    fn test_clear_timestamp_overflow() {
        let mut storage = MediumSizeHashTable::<u32, u32, TabulationHash>::new();

        // Set timestamp to MAX - 1
        storage.current_timestamp = Wrapping(u32::MAX - 1);

        // Insert some data
        *storage.get_mut(1) = 42;
        assert_eq!(storage.peek_value(1), Some(&42));

        // First clear increments to MAX
        storage.clear();

        assert_eq!(storage.current_timestamp.0, u32::MAX);
        // Old data should still be accessible since we're still at a valid timestamp
        assert_eq!(storage.peek_value(1), None);

        // Second clear should trigger overflow handling and reset
        storage.clear();
        assert_eq!(storage.current_timestamp.0, 0);
        // After reset, old data should be inaccessible
        assert_eq!(storage.peek_value(1), None);

        // Verify we can insert new data
        *storage.get_mut(2) = 43;
        assert_eq!(storage.peek_value(2), Some(&43));
    }

    #[test]
    fn test_tabulation_hash_table_default() {
        let mut default_table = MediumSizeHashTable::<u32, u32, TabulationHash>::default();
        let mut new_table = MediumSizeHashTable::<u32, u32, TabulationHash>::new();

        // Test that both tables behave the same way
        *default_table.get_mut(1) = 42;
        *new_table.get_mut(1) = 42;

        assert_eq!(default_table.peek_value(1), new_table.peek_value(1));
        assert_eq!(default_table.peek_value(1), Some(&42));
    }

    #[test]
    fn test_contains_key_basic() {
        let mut storage = MediumSizeHashTable::<u32, u32, TabulationHash>::new();

        assert!(
            !storage.contains_key(1),
            "Empty table should not contain any keys"
        );

        *storage.get_mut(1) = 42;
        assert!(storage.contains_key(1), "Key should exist after insertion");
        assert!(
            !storage.contains_key(2),
            "Non-existent key should return false"
        );

        storage.clear();
        assert!(!storage.contains_key(1), "Key should not exist after clear");
    }

    #[test]
    fn test_contains_key_collisions() {
        let mut storage = MediumSizeHashTable::<u32, u32, TabulationHash>::new();

        // Insert two keys that will definitely collide
        let base_key = 42;
        let colliding_key = base_key + MAX_ELEMENTS as u32;

        *storage.get_mut(base_key) = 100;
        *storage.get_mut(colliding_key) = 200;

        assert!(storage.contains_key(base_key), "First key should exist");
        assert!(
            storage.contains_key(colliding_key),
            "Colliding key should exist"
        );
        assert!(
            !storage.contains_key(base_key + 1),
            "Non-existent key should not exist"
        );
    }

    #[test]
    fn test_insert_basic() {
        let mut storage = MediumSizeHashTable::<u32, u32, TabulationHash>::new();

        storage.insert(1, 42);
        assert_eq!(
            storage.peek_value(1),
            Some(&42),
            "Value should be inserted correctly"
        );

        // Update existing key
        storage.insert(1, 43);
        assert_eq!(
            storage.peek_value(1),
            Some(&43),
            "Value should be updated correctly"
        );

        // Multiple inserts
        storage.insert(2, 100);
        storage.insert(3, 200);
        assert_eq!(
            storage.peek_value(2),
            Some(&100),
            "Second insert should work"
        );
        assert_eq!(
            storage.peek_value(3),
            Some(&200),
            "Third insert should work"
        );
    }

    #[test]
    fn test_insert_collisions() {
        let mut storage = MediumSizeHashTable::<u32, u32, TabulationHash>::new();

        // Create keys that will collide
        let base_key = 42;
        let keys = [
            base_key,
            base_key + MAX_ELEMENTS as u32,
            base_key + (2 * MAX_ELEMENTS) as u32,
        ];

        // Insert colliding values
        for (i, &key) in keys.iter().enumerate() {
            storage.insert(key, i as u32);
        }

        // Verify all values are stored correctly
        for (i, &key) in keys.iter().enumerate() {
            assert_eq!(
                storage.peek_value(key),
                Some(&(i as u32)),
                "Value for key {} should be stored correctly despite collisions",
                key
            );
        }

        // Update middle value
        storage.insert(keys[1], 99);

        // Verify chain remains intact
        assert_eq!(storage.peek_value(keys[0]), Some(&0));
        assert_eq!(storage.peek_value(keys[1]), Some(&99));
        assert_eq!(storage.peek_value(keys[2]), Some(&2));
    }

    #[test]
    fn test_len() {
        let mut storage = MediumSizeHashTable::<u32, u32, TabulationHash>::new();
        assert_eq!(storage.len(), 0, "New table should have length 0");

        *storage.get_mut(1) = 42;
        assert_eq!(storage.len(), 1, "Length should be 1 after first insert");

        // Update existing key - shouldn't change length
        *storage.get_mut(1) = 43;
        assert_eq!(
            storage.len(),
            1,
            "Length shouldn't change when updating existing key"
        );

        *storage.get_mut(2) = 100;
        assert_eq!(storage.len(), 2, "Length should increase with new key");

        storage.clear();
        assert_eq!(storage.len(), 0, "Length should be 0 after clear");
    }

    #[test]
    fn test_is_empty() {
        let mut storage = MediumSizeHashTable::<u32, u32, TabulationHash>::new();
        assert!(storage.is_empty(), "New table should be empty");

        *storage.get_mut(1) = 42;
        assert!(
            !storage.is_empty(),
            "Table should not be empty after insert"
        );

        storage.clear();
        assert!(storage.is_empty(), "Table should be empty after clear");

        // Test empty after timestamp overflow
        storage.current_timestamp = Wrapping(u32::MAX);
        *storage.get_mut(1) = 42;
        storage.clear(); // This will trigger timestamp overflow handling
        assert!(
            storage.is_empty(),
            "Table should be empty after timestamp overflow clear"
        );
    }

    #[test]
    fn test_capacity() {
        let storage = MediumSizeHashTable::<u32, u32, TabulationHash>::new();
        assert_eq!(
            storage.capacity(),
            MAX_ELEMENTS,
            "Capacity should be MAX_ELEMENTS"
        );

        let mut storage = MediumSizeHashTable::<u32, u32, TabulationHash>::new();
        for i in 0..100 {
            *storage.get_mut(i) = i;
        }
        assert_eq!(
            storage.capacity(),
            MAX_ELEMENTS,
            "Capacity should remain constant regardless of content"
        );
        assert!(
            storage.len() <= storage.capacity(),
            "Length should never exceed capacity"
        );
    }
}
