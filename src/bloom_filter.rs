use bitvec::prelude::*;
use xxhash_rust::xxh3::xxh3_64_with_seed;

/// Straight-forward implementation of a bloom filter on u8 slices. It applies
/// the result of Kirsch and Mitzenmacher [1] of using a simple linear
/// combination of two hash functions without any loss in the asymptotic false
/// positive rate.
/// [1] Kirsch, Mitzenmacher. "Less Hashing, Same Performance: Building a Better Bloom Filter".
pub struct BloomFilter {
    bit_vector: BitVec,
    number_of_functions: usize,
}

/// Result type for the contains(.) operation
#[derive(Debug, Eq, PartialEq)]
pub enum BloomResult {
    No,
    YesWhp,
}

pub trait AsBytes {
    fn as_bytes(&self) -> &[u8];
}

impl AsBytes for &str {
    fn as_bytes(&self) -> &[u8] {
        str::as_bytes(self)
    }
}

impl BloomFilter {
    // first hash function
    fn fn1(&self, t: &[u8]) -> usize {
        // hash operations are based on xxhash3 which is fast
        xxh3_64_with_seed(t, 0xdeadbeef) as usize
    }

    // second hash function
    fn fn2(&self, t: &[u8]) -> usize {
        // hash operations are based on xxhash3 which is fast
        xxh3_64_with_seed(t, 123) as usize
    }

    // constructs an empty filter with optimal length of bit vector and number
    // of simulated hash functions.
    pub fn new_from_size_and_probabilty(expected_set_size: usize, probability: f64) -> Self {
        assert!(probability > 0.);
        assert!(probability < 1.);

        // calculate optimal values on size and function count
        let ln_pfp = probability.log(std::f64::consts::E);
        let ln_2 = 1. / std::f64::consts::LOG2_E;

        let optimal_vector_legth =
            (-(expected_set_size as f64 * ln_pfp / ln_2.powi(2)).ceil()) as usize;
        assert!(optimal_vector_legth > 0);

        let bit_vector: BitVec = (0..optimal_vector_legth).map(|_a| false).collect();
        let number_of_functions = (-(ln_pfp / ln_2).ceil()) as usize;
        assert!(number_of_functions > 0);

        BloomFilter {
            bit_vector,
            number_of_functions,
        }
    }

    // constructs a filter from a list of inputs
    pub fn new_from_list<T: AsBytes>(list: &[T], probability: f64) -> Self {
        let mut filter = BloomFilter::new_from_size_and_probabilty(list.len(), probability);
        for i in list {
            // add all the items to the filter
            filter.add(i);
        }
        filter
    }

    // checks wether the given value is contained with high probability.
    pub fn contains(&self, value: &[u8]) -> BloomResult {
        let len = self.bit_vector.len();
        let fn1_value = self.fn1(value) % len;

        let all_hash_fns_matched = (0..self.number_of_functions).all(|i| {
            let fn2_value = self.fn2(value) % len;
            let index = (fn1_value + (i * fn2_value)) % len;
            self.bit_vector[index]
        });
        if all_hash_fns_matched {
            return BloomResult::YesWhp;
        }
        BloomResult::No
    }

    // adds a value via its byte representation to the filter.
    pub fn add_bytes(&mut self, value: &[u8]) {
        let len = self.bit_vector.len();
        let fn1_value = self.fn1(value) % len;

        (0..self.number_of_functions).for_each(|i| {
            let fn2_value = self.fn2(value) % len;
            let index = (fn1_value + (i * fn2_value)) % len;
            self.bit_vector.set(index, true);
        });
    }

    // add a value to the filter
    pub fn add<T: AsBytes>(&mut self, value: &T) {
        self.add_bytes(value.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use crate::bloom_filter::BloomResult;

    use super::BloomFilter;

    #[test]
    fn one_sentence() {
        let sentence1 = "this is just a string of words with little meaning.";
        let sentence2 = "and this is another one with equally little meaning.";

        let mut filter = BloomFilter::new_from_size_and_probabilty(1, 0.001);
        // assert!(filter.size > 10);
        filter.add(&sentence1);
        assert_eq!(filter.contains(sentence1.as_bytes()), BloomResult::YesWhp);
        assert_eq!(filter.contains(sentence2.as_bytes()), BloomResult::No);
    }

    #[test]
    fn from_list() {
        let sentence1 = "this is just a string of words with little meaning.";
        let sentence2 = "and this is another one with equally little meaning.";
        let list = vec![sentence1, sentence2];

        let sentence3 = "I am running out of meaningless examples to write.";

        let mut filter = BloomFilter::new_from_list(&list, 0.001);
        // assert!(filter.size > 10);
        filter.add(&sentence1);
        assert_eq!(filter.contains(sentence1.as_bytes()), BloomResult::YesWhp);
        assert_eq!(filter.contains(sentence2.as_bytes()), BloomResult::YesWhp);
        assert_eq!(filter.contains(sentence3.as_bytes()), BloomResult::No);
    }
}
