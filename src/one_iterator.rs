// iterate the indices of the ones in the binary representation of a u32 integer
pub struct OneIterator {
    value: u32,
}

impl Iterator for OneIterator {
    type Item = u32;

    fn next(&mut self) -> Option<Self::Item> {
        if self.value == 0 {
            return None;
        }
        let first_bit = 31 - self.value.leading_zeros();
        self.value ^= 1 << first_bit;
        Some(first_bit)
    }
}

impl From<u32> for OneIterator {
    fn from(value: u32) -> Self {
        OneIterator { value }
    }
}

pub trait OneIter {
    /// Instantiate a OneIterator for the underlying object
    fn one_iter(&self) -> OneIterator;
}

impl OneIter for u32 {
    fn one_iter(&self) -> OneIterator {
        OneIterator::from(*self)
    }
}

#[cfg(test)]
mod tests {
    use crate::one_iterator::{OneIter, OneIterator};

    #[test]
    fn iterate_ones_from() {
        let count = OneIterator::from(0xFFFFFFFF).count();
        assert_eq!(32, count);
        let indices: Vec<u32> = OneIterator::from(0xFFFFFFFF).collect();
        assert_eq!(
            indices,
            vec![
                31, 30, 29, 28, 27, 26, 25, 24, 23, 22, 21, 20, 19, 18, 17, 16, 15, 14, 13, 12, 11,
                10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0
            ]
        );
        let indices: Vec<u32> = OneIterator::from(0x44104085).collect();
        assert_eq!(indices, vec![30, 26, 20, 14, 7, 2, 0]);
    }

    #[test]
    fn iterate_ones_one_iter() {
        let count = 0xFFFFFFFF.one_iter().count();
        assert_eq!(32, count);
        let indices: Vec<u32> = 0xFFFFFFFF.one_iter().collect();
        assert_eq!(
            indices,
            vec![
                31, 30, 29, 28, 27, 26, 25, 24, 23, 22, 21, 20, 19, 18, 17, 16, 15, 14, 13, 12, 11,
                10, 9, 8, 7, 6, 5, 4, 3, 2, 1, 0
            ]
        );
        let indices: Vec<u32> = 0x44104085.one_iter().collect();
        assert_eq!(indices, vec![30, 26, 20, 14, 7, 2, 0]);
    }

    #[test]
    fn iterate_zero() {
        let mut iter = 0.one_iter();
        assert_eq!(None, iter.next());
    }
}
