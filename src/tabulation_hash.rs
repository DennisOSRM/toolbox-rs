use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::num::Wrapping;

const MAX_ELEMENTS: usize = 65536;

/// Implementation of Tabulation hashing with XOR operations.
///
/// Properties:
/// - Universal hashing properties
/// - Space requirement: 2*2^16 = 256kb (fits in L2 cache)
/// - Efficient evaluation (approximately 10 assembly instructions on x86 and ARM)
/// - Deterministic output for same input values
#[derive(Debug)]
pub struct TabulationHash {
    table1: Vec<u16>,
    table2: Vec<u16>,
}

impl Default for TabulationHash {
    fn default() -> Self {
        Self::new()
    }
}

impl TabulationHash {
    /// Creates a new TabulationHash instance with shuffled tables.
    ///
    /// The tables are initialized with random values from 0..=u16::MAX by
    /// using a seeded random number generator. The seed is set to 0 for
    /// reproducibility. This means that the same input will always produce
    /// the same hash value, making it suitable for applications where
    /// consistent hashing is required.
    ///
    /// # Example
    ///
    /// ```
    /// use toolbox_rs::tabulation_hash::TabulationHash;
    /// let hasher = TabulationHash::new();
    /// ```
    pub fn new() -> Self {
        let mut rng = StdRng::seed_from_u64(0);

        // Initialize tables with random values
        let table1: Vec<u16> = (0..MAX_ELEMENTS)
            .map(|_| rng.random_range(0..=u16::MAX))
            .collect();
        let table2: Vec<u16> = (0..MAX_ELEMENTS)
            .map(|_| rng.random_range(0..=u16::MAX))
            .collect();

        debug_assert_eq!(table1.len(), 65_536);
        debug_assert_eq!(table2.len(), 65_536);

        Self { table1, table2 }
    }

    /// Computes a 16-bit hash value for a 32-bit input using tabulation hashing.
    ///
    /// # Algorithm
    ///
    /// 1. Splits input into 16-bit LSB and MSB parts
    /// 2. Uses parts to index into pre-computed tables
    /// 3. Combines results using XOR operation
    ///
    /// # Arguments
    ///
    /// * `value` - 32-bit unsigned integer to hash
    ///
    /// # Returns
    ///
    /// A 16-bit hash value in range 0..MAX_ELEMENTS
    ///
    /// # Example
    ///
    /// ```
    /// use toolbox_rs::tabulation_hash::TabulationHash;
    /// let hasher = TabulationHash::new();
    /// let hash = hasher.hash(42);
    /// assert!(hash <= u16::MAX);
    /// ```
    #[inline]
    pub fn hash(&self, key: u32) -> u16 {
        let lsb = (key & 0xffff) as usize;
        let msb = (key >> 16) as usize;

        self.table1[lsb] ^ self.table2[msb]
    }
}

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
#[derive(Debug)]
pub struct TabulationHashTable<Key, Value> {
    positions: Vec<HashCell<Key, Value>>,
    hasher: TabulationHash,
    current_timestamp: Wrapping<u32>,
}

impl<Key, Value> Default for TabulationHashTable<Key, Value>
where
    Key: Copy + Default + PartialEq + TryInto<u32>,
    Value: Copy + Default,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<Key, Value> TabulationHashTable<Key, Value>
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
            hasher: TabulationHash::new(),
            current_timestamp: Wrapping(0),
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
        cell.time = self.current_timestamp.0;
        cell.key = key;

        &mut cell.value
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

    /// Clears the hash table by incrementing the timestamp.
    ///
    /// If the timestamp would overflow, reallocates the table instead.
    /// This provides an efficient O(1) clear operation in most cases.
    pub fn clear(&mut self) {
        self.current_timestamp += Wrapping(1);

        if self.current_timestamp.0 == 0 {
            self.positions = vec![HashCell::default(); MAX_ELEMENTS];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_deterministic() {
        let hasher1 = TabulationHash::new();
        let hasher2 = TabulationHash::new();

        assert_eq!(hasher1.hash(42), hasher2.hash(42));
    }

    #[test]
    fn test_hash_different_inputs() {
        let hasher = TabulationHash::new();

        let hash1 = hasher.hash(1);
        let hash2 = hasher.hash(26);
        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_hash_storage_basic() {
        let mut storage = TabulationHashTable::<u32, u32>::new();

        // Insert and retrieve using new API
        *storage.get_mut(1) = 42;
        assert_eq!(storage.peek_value(1), Some(&42));

        // Update existing
        *storage.get_mut(1) = 43;
        assert_eq!(storage.peek_value(1), Some(&43));
    }

    #[test]
    fn test_hash_storage_collision() {
        let mut storage = TabulationHashTable::<u32, u32>::new();

        // Insert two keys that might collide
        *storage.get_mut(1) = 42;
        *storage.get_mut(65537) = 43; // Could hash to same position

        assert_eq!(storage.peek_value(1), Some(&42));
        assert_eq!(storage.peek_value(65537), Some(&43));
    }

    #[test]
    fn test_hash_storage_clear() {
        let mut storage = TabulationHashTable::<u32, u32>::new();

        *storage.get_mut(1) = 42;
        storage.clear();
        assert_eq!(storage.peek_value(1), None); // Default value after clear
    }

    #[test]
    fn test_linear_probing_sequence() {
        let mut storage = TabulationHashTable::<u32, u32>::new();

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
        let mut storage = TabulationHashTable::<u32, u32>::new();

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
        let mut storage = TabulationHashTable::<u32, u32>::new();

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
    fn test_tabulation_hash_default() {
        let default_hasher = TabulationHash::default();
        let new_hasher = TabulationHash::new();

        // Test that default gives same results as new()
        assert_eq!(default_hasher.hash(42), new_hasher.hash(42));
        assert_eq!(default_hasher.hash(100), new_hasher.hash(100));
    }

    #[test]
    fn test_tabulation_hash_table_default() {
        let mut default_table = TabulationHashTable::<u32, u32>::default();
        let mut new_table = TabulationHashTable::<u32, u32>::new();

        // Test that both tables behave the same way
        *default_table.get_mut(1) = 42;
        *new_table.get_mut(1) = 42;

        assert_eq!(default_table.peek_value(1), new_table.peek_value(1));
        assert_eq!(default_table.peek_value(1), Some(&42));
    }
}
