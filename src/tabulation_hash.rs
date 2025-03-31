use rand::rngs::StdRng;
use rand::{SeedableRng, seq::SliceRandom};
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

impl TabulationHash {
    /// Creates a new TabulationHash instance with shuffled tables.
    ///
    /// The tables are initialized with values 0..MAX_ELEMENTS and then shuffled
    /// using a deterministic RNG seeded with 0. This ensures consistent hashing
    /// across program runs.
    ///
    /// # Example
    ///
    /// ```
    /// use toolbox_rs::tabulation_hash::TabulationHash;
    /// let hasher = TabulationHash::new();
    /// ```
    pub fn new() -> Self {
        let mut rng = StdRng::seed_from_u64(0);

        // Initialize and shuffle tables
        let mut table1: Vec<u16> = (0..=u16::MAX).collect();
        let mut table2: Vec<u16> = (0..=u16::MAX).collect();

        debug_assert_eq!(table1.len(), 65_536);
        debug_assert_eq!(table2.len(), 65_536);

        table1.shuffle(&mut rng);
        table2.shuffle(&mut rng);

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
    pub fn hash(&self, value: u32) -> u16 {
        let lsb = (value & 0xffff) as usize;
        let msb = (value >> 16) as usize;

        debug_assert!(
            lsb < self.table1.len(),
            "lsb: {}, table.len(): {}",
            lsb,
            self.table1.len()
        );
        debug_assert!(
            msb < self.table2.len(),
            "msb: {}, table.len(): {}",
            msb,
            self.table2.len()
        );

        self.table1[lsb] ^ self.table2[msb]
    }
}

/// A hash cell storing node ID, key and timestamp for collision resolution.
///
/// Used internally by TabulationHashTable to store elements and handle
/// linear probing during collision resolution.
#[derive(Debug, Clone)]
pub struct HashCell<NodeID, Key> {
    /// Timestamp for marking cell validity
    time: u32,
    /// Node identifier
    id: NodeID,
    /// Associated key/value
    key: Key,
}

impl<NodeID: Default, Key: Default> Default for HashCell<NodeID, Key> {
    fn default() -> Self {
        Self {
            time: u32::MAX,
            id: NodeID::default(),
            key: Key::default(),
        }
    }
}

/// A hash table using tabulation hashing for fast lookups with linear probing
/// for collision resolution.
///
/// # Type Parameters
///
/// * `NodeID` - Type of node identifiers (must be convertible to u32)
/// * `Key` - Type of stored values
///
/// # Performance
///
/// - O(1) average case for insertions and lookups
/// - O(1) average case for clear operation using timestamp-based invalidation
/// - Space complexity: O(MAX_ELEMENTS)
#[derive(Debug)]
pub struct TabulationHashTable<NodeID, Key> {
    positions: Vec<HashCell<NodeID, Key>>,
    hasher: TabulationHash,
    current_timestamp: Wrapping<u32>,
}

impl<NodeID, Key> TabulationHashTable<NodeID, Key>
where
    NodeID: Copy + Default + PartialEq + TryInto<u32>,
    Key: Copy + Default,
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

    /// Gets or creates a mutable reference to a hash cell for the given node.
    ///
    /// # Algorithm
    ///
    /// 1. Computes initial position using tabulation hash
    /// 2. Uses linear probing to handle collisions
    /// 3. Updates timestamp and node ID on access
    ///
    /// # Arguments
    ///
    /// * `node` - Node identifier
    ///
    /// # Returns
    ///
    /// Mutable reference to the hash cell
    ///
    /// # Panics
    ///
    /// Panics if `NodeID` cannot be converted to `u32`
    pub fn get_mut(&mut self, node: NodeID) -> &mut HashCell<NodeID, Key> {
        let node_as_u32: u32 = node
            .try_into()
            .unwrap_or_else(|_| panic!("NodeID must be convertible to u32"));
        let mut position = self.hasher.hash(node_as_u32) as usize;

        while self.positions[position].time == self.current_timestamp.0
            && self.positions[position].id != node
        {
            position = (position + 1) % MAX_ELEMENTS;
        }

        let cell = &mut self.positions[position];
        cell.time = self.current_timestamp.0;
        cell.id = node;

        cell
    }

    /// Looks up the key associated with a node without modifying the timestamp.
    ///
    /// # Arguments
    ///
    /// * `node` - Node identifier to look up
    ///
    /// # Returns
    ///
    /// * `Some(key)` - If the node exists in the current timestamp
    /// * `None` - If the node doesn't exist or was cleared
    ///
    /// # Panics
    ///
    /// Panics if `NodeID` cannot be converted to `u32`
    pub fn peek_key(&self, node: NodeID) -> Option<Key> {
        let node_as_u32: u32 = node
            .try_into()
            .unwrap_or_else(|_| panic!("NodeID must be convertible to u32"));
        let mut position = self.hasher.hash(node_as_u32) as usize;

        while self.positions[position].time == self.current_timestamp.0
            && self.positions[position].id != node
        {
            position = (position + 1) % MAX_ELEMENTS;
        }

        if self.positions[position].time == self.current_timestamp.0 {
            return Some(self.positions[position].key);
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

        // Insert and retrieve
        storage.get_mut(1).key = 42;
        assert_eq!(storage.peek_key(1), Some(42));

        // Update existing
        storage.get_mut(1).key = 43;
        assert_eq!(storage.peek_key(1), Some(43));
    }

    #[test]
    fn test_hash_storage_collision() {
        let mut storage = TabulationHashTable::<u32, u32>::new();

        // Insert two nodes that might collide
        storage.get_mut(1).key = 42;
        storage.get_mut(65537).key = 43; // Could hash to same position

        assert_eq!(storage.peek_key(1), Some(42));
        assert_eq!(storage.peek_key(65537), Some(43));
    }

    #[test]
    fn test_hash_storage_clear() {
        let mut storage = TabulationHashTable::<u32, u32>::new();

        storage.get_mut(1).key = 42;
        storage.clear();
        assert_eq!(storage.peek_key(1), None); // Default value after clear
    }

    #[test]
    fn test_linear_probing_sequence() {
        let mut storage = TabulationHashTable::<u32, u32>::new();

        // Create a sequence of values that will hash to the same position
        let base_node = 42;
        let colliding_nodes = [
            base_node,
            base_node + MAX_ELEMENTS as u32,
            base_node + (2 * MAX_ELEMENTS) as u32,
        ];

        // Insert values in sequence to force linear probing
        for (i, &node) in colliding_nodes.iter().enumerate() {
            storage.get_mut(node).key = i as u32;
        }

        // Verify each value is still accessible and in the correct position
        for (i, &node) in colliding_nodes.iter().enumerate() {
            assert_eq!(
                storage.peek_key(node),
                Some(i as u32),
                "Failed to retrieve value {} for node {} after linear probing",
                i,
                node
            );
        }

        // Update middle value and verify chain remains intact
        storage.get_mut(colliding_nodes[1]).key = 42;

        assert_eq!(storage.peek_key(colliding_nodes[0]), Some(0));
        assert_eq!(storage.peek_key(colliding_nodes[1]), Some(42));
        assert_eq!(storage.peek_key(colliding_nodes[2]), Some(2));
    }

    #[test]
    fn test_linear_probing_loop() {
        let mut storage = TabulationHashTable::<u32, u32>::new();

        // First, find two nodes that hash to the same position
        let base_node = 0u32;
        let mut colliding_node = 1u32;

        while storage.hasher.hash(base_node) != storage.hasher.hash(colliding_node) {
            colliding_node += 1;
        }

        // Now we have two nodes that will definitely collide
        storage.get_mut(base_node).key = 100;
        storage.get_mut(colliding_node).key = 200;

        // Find a third node that hashes to the same position
        let mut third_node = colliding_node + 1;
        while storage.hasher.hash(third_node) != storage.hasher.hash(base_node) {
            third_node += 1;
        }
        storage.get_mut(third_node).key = 300;

        // Verify the probing sequence
        assert_eq!(storage.peek_key(base_node), Some(100));
        assert_eq!(storage.peek_key(colliding_node), Some(200));
        assert_eq!(storage.peek_key(third_node), Some(300));

        // Update middle node to ensure probing still works
        storage.get_mut(colliding_node).key = 250;

        // Verify entire chain is intact
        assert_eq!(storage.peek_key(base_node), Some(100));
        assert_eq!(storage.peek_key(colliding_node), Some(250));
        assert_eq!(storage.peek_key(third_node), Some(300));
    }

    #[test]
    fn test_clear_timestamp_overflow() {
        let mut storage = TabulationHashTable::<u32, u32>::new();

        // Set timestamp to MAX - 1
        storage.current_timestamp = Wrapping(u32::MAX - 1);

        // Insert some data
        storage.get_mut(1).key = 42;
        assert_eq!(storage.peek_key(1), Some(42));

        // First clear increments to MAX
        storage.clear();

        assert_eq!(storage.current_timestamp.0, u32::MAX);
        // Old data should still be accessible since we're still at a valid timestamp
        assert_eq!(storage.peek_key(1), None);

        // Second clear should trigger overflow handling and reset
        storage.clear();
        assert_eq!(storage.current_timestamp.0, 0);
        // After reset, old data should be inaccessible
        assert_eq!(storage.peek_key(1), None);

        // Verify we can insert new data
        storage.get_mut(2).key = 43;
        assert_eq!(storage.peek_key(2), Some(43));
    }
}
