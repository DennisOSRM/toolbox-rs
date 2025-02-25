use num::{Integer, traits::WrappingSub};

/// Iterate all bitset subsets of a given bitset
/// Implements what is known as Carry-Rippler trick
///
pub struct BitsetSubsetIterator<T: Integer> {
    subset: T,
    set: T,
    done: bool,
}

impl<T: Integer> BitsetSubsetIterator<T> {
    pub fn from_bitset(set: T) -> Self {
        Self {
            subset: T::zero(),
            set,
            done: false,
        }
    }
}

impl<T: Copy + Integer + WrappingSub + std::ops::BitAnd<Output = T>> Iterator
    for BitsetSubsetIterator<T>
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        if self.done {
            return None;
        }

        let temp = self.subset;
        self.subset = self.subset.wrapping_sub(&self.set) & self.set;
        self.done = self.subset == T::zero();
        Some(temp)
    }
}

#[cfg(test)]
mod tests {
    use super::BitsetSubsetIterator;

    #[test]
    fn x55_bitmask() {
        let result: Vec<i32> = BitsetSubsetIterator::from_bitset(0x55).collect();
        let expected = [0, 1, 4, 5, 16, 17, 20, 21, 64, 65, 68, 69, 80, 81, 84, 85];
        assert_eq!(result.len(), 16);
        assert_eq!(result, expected);
    }
}
