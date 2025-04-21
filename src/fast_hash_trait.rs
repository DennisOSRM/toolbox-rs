/// Maximum number of distinct elements that can be hashed.
/// This is limited to 2^16 (65536) since the hash function returns a u16.
pub const MAX_ELEMENTS: usize = 65536;

/// A trait for fast hashing implementations that map 32-bit keys to 16-bit hash values.
///
/// This trait is designed for scenarios where:
/// - You need very fast hashing operations
/// - A perfect hash function is not required
/// - The hash space can be limited to 16 bits (0-65535)
/// - Collisions are acceptable but should be minimized
///
/// # Examples
///
/// ```
/// use toolbox_rs::fast_hash_trait::FastHash;
///
/// struct SimpleHash;
///
/// impl FastHash for SimpleHash {
///     fn hash(&self, key: u32) -> u16 {
///         // Simple example hash: take the lower 16 bits
///         (key & 0xFFFF) as u16
///     }
/// }
///
/// let hasher = SimpleHash;
/// let hash1 = hasher.hash(123456);
/// let hash2 = hasher.hash(123456);
/// assert_eq!(hash1, hash2); // Same input produces same hash
/// ```
///
/// # Notes
///
/// - The hash function must be deterministic: the same input must always produce the same output
/// - The hash function should aim to distribute values uniformly across the u16 range
/// - The implementation should be as fast as possible, avoiding complex operations
///
/// # Safety
///
/// The hash function must never panic for any input value.
pub trait FastHash {
    /// Computes a 16-bit hash value for the given 32-bit key.
    ///
    /// # Arguments
    ///
    /// * `key` - The 32-bit unsigned integer to hash
    ///
    /// # Returns
    ///
    /// A 16-bit hash value in the range [0, 65535]
    fn hash(&self, key: u32) -> u16;
}
