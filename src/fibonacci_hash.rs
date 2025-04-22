use crate::fast_hash_trait::FastHash;

/// A hash function implementation using the Fibonacci hashing technique.
///
/// Fibonacci hashing is a multiplication-based hashing method that uses the golden ratio
/// to achieve a good distribution of hash values. It works particularly well for integer keys
/// and provides good performance characteristics due to its simplicity.
///
/// # Examples
///
/// Basic usage:
/// ```
/// use toolbox_rs::fibonacci_hash::FibonacciHash;
/// use toolbox_rs::fast_hash_trait::FastHash;
///
/// let hasher = FibonacciHash::new();
/// let hash = hasher.hash(42);
/// assert!(hash <= u16::MAX);
/// ```
///
/// Different inputs produce different hashes:
/// ```
/// use toolbox_rs::fibonacci_hash::FibonacciHash;
/// use toolbox_rs::fast_hash_trait::FastHash;
///
/// let hasher = FibonacciHash::new();
/// let hash1 = hasher.hash(1);
/// let hash2 = hasher.hash(2);
/// assert_ne!(hash1, hash2);
/// ```
#[derive(Default)]
pub struct FibonacciHash;

impl FibonacciHash {
    /// Creates a new FibonacciHash instance with the specified shift amount.
    ///
    /// The shift amount determines how many bits to shift during the hashing process.
    /// A typical value is 16, which produces good distribution for most use cases.
    ///
    /// # Examples
    ///
    /// ```
    /// use toolbox_rs::fibonacci_hash::FibonacciHash;
    ///
    /// let hasher = FibonacciHash::new();
    /// ```
    pub fn new() -> Self {
        Self {}
    }
}

impl FastHash for FibonacciHash {
    /// Computes a 16-bit hash value for a 32-bit input using Fibonacci hashing.
    ///
    /// # Algorithm
    ///
    /// 1. XORs the input with a shifted version of itself
    /// 2. Multiplies by the golden ratio constant (≈ (√5-1)/2 * 2⁶⁴)
    /// 3. Shifts the result right to get the final hash
    ///
    /// # Arguments
    ///
    /// * `key` - 32-bit unsigned integer to hash
    ///
    /// # Returns
    ///
    /// A 16-bit hash value
    ///
    /// # Examples
    ///
    /// ```
    /// use toolbox_rs::fibonacci_hash::FibonacciHash;
    /// use toolbox_rs::fast_hash_trait::FastHash;
    ///
    /// let hasher = FibonacciHash::new();
    ///
    /// // Same input produces same hash
    /// assert_eq!(hasher.hash(42), hasher.hash(42));
    ///
    /// // Values are within u16 range
    /// for i in 0..10 {
    ///     assert!(hasher.hash(i) <= u16::MAX);
    /// }
    /// ```
    fn hash(&self, key: u32) -> u16 {
        // First XOR the high bits with the low bits
        let mut hash = key as u64;
        hash ^= hash >> 16;

        // Multiply by the golden ratio constant (approximately (√5-1)/2 * 2⁶⁴)
        const GOLDEN_RATIO: u64 = 11400714819323198485;
        let result = (GOLDEN_RATIO.wrapping_mul(hash)) >> 16;

        result as u16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fibonacci_hash_deterministic() {
        let hasher = FibonacciHash::new();
        let hash1 = hasher.hash(42);
        let hash2 = hasher.hash(42);
        assert_eq!(hash1, hash2, "Same input should produce same hash");
    }

    #[test]
    fn test_different_inputs() {
        let hasher = FibonacciHash::new();
        let hash1 = hasher.hash(1);
        let hash2 = hasher.hash(2);
        assert_ne!(
            hash1, hash2,
            "Different inputs should produce different hashes"
        );
    }

    #[test]
    fn test_hash_distribution() {
        let hasher = FibonacciHash::new();
        let mut seen = std::collections::HashSet::new();

        // Test distribution over a reasonable range
        for i in 0..1000 {
            seen.insert(hasher.hash(i));
        }

        // We expect a good hash function to have reasonable distribution
        // For 1000 inputs hashed to u16, we expect a decent number of unique values
        assert!(
            seen.len() > 900,
            "Hash function should provide good distribution"
        );
    }
}
