use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use crate::medium_size_hash_table::{FastHash, MAX_ELEMENTS};
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
}

impl FastHash for TabulationHash {
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
    /// use crate::toolbox_rs::medium_size_hash_table::FastHash;
    /// use toolbox_rs::tabulation_hash::TabulationHash;
    /// let hasher = TabulationHash::new();
    /// let hash = hasher.hash(42);
    /// assert!(hash <= u16::MAX);
    /// ```
    #[inline]
    fn hash(&self, key: u32) -> u16 {
        let lsb = (key & 0xffff) as usize;
        let msb = (key >> 16) as usize;

        self.table1[lsb] ^ self.table2[msb]
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
    fn test_tabulation_hash_default() {
        let default_hasher = TabulationHash::default();
        let new_hasher = TabulationHash::new();

        // Test that default gives same results as new()
        assert_eq!(default_hasher.hash(42), new_hasher.hash(42));
        assert_eq!(default_hasher.hash(100), new_hasher.hash(100));
    }
}
