use crate::{enumerative_source_coding::decode_u64, math::choose};

/// Implement an iterator of all 64 bit integers with fixed weight
pub struct U64BitWeightIterator {
    weight: u64,
    ordinal: u64,
    max: u64,
}

impl U64BitWeightIterator {
    pub fn with_weight(weight: u64) -> Self {
        U64BitWeightIterator {
            weight,
            ordinal: 0,
            max: choose(64u64, weight),
        }
    }
}

impl Iterator for U64BitWeightIterator {
    type Item = u64;

    fn next(&mut self) -> Option<Self::Item> {
        if self.ordinal < self.max {
            self.ordinal += 1;
            return Some(decode_u64(self.weight, self.ordinal - 1));
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::U64BitWeightIterator;

    #[test]
    fn trivial_iterator_of_weight_one() {
        let result: Vec<u64> = U64BitWeightIterator::with_weight(1).collect();
        assert_eq!(result.len(), 64);
        let expected: [u64; 64] = core::array::from_fn(|i| 1 << i);
        assert_eq!(result, expected);
    }

    #[test]
    fn trivial_iterator_of_weight_63() {
        let result: Vec<u64> = U64BitWeightIterator::with_weight(63).collect();
        assert_eq!(result.len(), 64);
        let expected: [u64; 64] =
            core::array::from_fn(|i| 0xFFFF_FFFF_FFFF_FFFF ^ (1u64 << (63 - i)));
        assert_eq!(result, expected);
    }
}
