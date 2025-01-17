/// Implementation of a Fenwick tree to keep track and query prefix sums in logarithmic time.
/// cf. Boris Ryabko (1989). "A fast on-line code" (PDF). Soviet Math. Dokl. 39 (3): 533–537
///     Peter M. Fenwick (1994). "A new data structure for cumulative frequency tables". Software: Practice and Experience. 24 (3): 327–336
///
/// Internally the implementation is one-indexed, while externally it is zero-based. This simplifies the implementation.
use num::Integer;

#[derive(Clone, Debug)]
pub struct IndexOutOfRangeError;

#[derive(Clone, Debug)]
pub struct Fenwick<T: Integer + std::clone::Clone + Copy + std::ops::AddAssign + std::ops::SubAssign> {
    tree: Vec<T>,
}

impl<T: Integer + std::clone::Clone + Copy + std::ops::AddAssign + std::ops::SubAssign> Fenwick<T> {
    /// construct with fixed size.
    pub fn with_size(n: usize) -> Self {
        Self {
            tree: vec![T::zero(); n + 1],
        }
    }

    // bulk construction in linear time. Useful for initialization over state trees
    // Halim, Steven; Halim, Felix; Effendy, Suhendry (3 December 2018). Competitive Programming 4. Vol. 1. Lulu Press
    pub fn from_values(values: &[T]) -> Self {
        let mut tree = vec![T::zero()];
        tree.extend_from_slice(values);

        for index in 1..tree.len() {
            let parent_index = index + Self::largest_power_of_two_divisor(index);
            if parent_index < tree.len() {
                let parent_value = tree[index];
                tree[parent_index] += parent_value;
            }
        }
        Self { tree }
    }

    /// retrieves the number of elements in the data structure.
    pub fn len(&self) -> usize {
        self.tree.len() - 1
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// retrieve the prefix sum at the given zero-based index
    pub fn query(&self, index: usize) -> T {
        let mut index = index + 1;
        let mut sum = T::zero();
        while index > 0 {
            sum += self.tree[index];
            index -= Self::largest_power_of_two_divisor(index);
        }
        sum
    }

    // update the entry with a new relative value
    pub fn update(&mut self, index: usize, value: T) -> Result<(), IndexOutOfRangeError> {
        if index >= self.len() {
            return Err(IndexOutOfRangeError);
        }
        let mut index = index + 1;
        while index < self.tree.len() {
            self.tree[index] += value;
            index += Self::largest_power_of_two_divisor(index);
        }
        Ok(())
    }

    // internal helper function to compute 2 << lsb(n)
    fn largest_power_of_two_divisor(n: usize) -> usize {
        // TODO: should this go into math.rs if generally useful?
        n & n.wrapping_neg()
    }

    pub fn range(&self, i: usize, j: usize) -> T {
        if i > j {
            return T::zero();
        }

        let mut sum = T::zero();
        let mut j = j + 1;

        while j > i {
            sum += self.tree[j];
            j -= Self::largest_power_of_two_divisor(j);
        }

        let mut i = i + 1;
        while i>j {
            sum -= self.tree[i];
            i -= Self::largest_power_of_two_divisor(i);
        }

        sum
    }

    /// This function is a straight-forward implementation for range.
    /// Should be used in testing, or benchmarking mode only.
    pub fn slow_range(&self, i: usize, j: usize) -> T {
        if i > j {
            return T::zero();
        }
        self.query(j) - self.query(i)
    }
}

#[cfg(test)]
mod tests {
    use super::Fenwick;

    #[test]
    fn piecemeal_construction1() {
        let mut fenwick = Fenwick::with_size(30);
        fenwick.update(0, 10).unwrap();
        assert_eq!(fenwick.query(0), 10);
        assert_eq!(fenwick.query(1), 10);
    }

    #[test]
    fn piecemeal_construction2() {
        let input = [1, 2, 3, 4];
        let result = [1, 3, 6, 10];
        let mut fenwick = Fenwick::with_size(4);
        for i in 0..input.len() {
            assert!(fenwick.update(i, input[i]).is_ok());
        }
        assert_eq!(fenwick.len(), 4);
        for i in 0..input.len() {
            assert_eq!(result[i], fenwick.query(i));
        }
    }

    #[test]
    fn bulk_construction1() {
        let input = [1, 2, 3, 4];
        let result = [1, 3, 6, 10];
        let fenwick = Fenwick::from_values(&input);
        assert_eq!(fenwick.len(), 4);
        for i in 0..input.len() {
            assert_eq!(result[i], fenwick.query(i));
        }
    }

    #[test]
    fn bulk_piecemeal_same() {
        let input = [19, 3, 27, 28, 263, 3897, -4, 27];
        let mut fenwick1 = Fenwick::with_size(input.len());
        for i in 0..input.len() {
            assert!(fenwick1.update(i, input[i]).is_ok());
        }
        let fenwick1 = fenwick1;

        let fenwick2 = Fenwick::from_values(&input);

        assert_eq!(fenwick1.len(), fenwick2.len());
        for i in 0..input.len() {
            assert_eq!(fenwick1.query(i), fenwick2.query(i));
        }
    }

    #[test]
    fn is_empty() {
        let fenwick = Fenwick::<u8>::with_size(0);
        assert!(fenwick.is_empty());

        let fenwick = Fenwick::from_values(&[0, 1, 4]);
        assert!(!fenwick.is_empty());
    }

    #[test]
    fn update_out_of_bounds() {
        let mut fenwick = Fenwick::from_values(&[0, 1, 4]);
        assert!(fenwick.update(0, 100).is_ok());
        assert!(fenwick.update(3, 100).is_err());
    }

    #[test]
    fn range_test() {
        let input = [19, 3, 27, 28, 263, 3897, -4, 27];
        let fenwick = Fenwick::from_values(&input);
        for i in 0..input.len() {
            for j in i..input.len() {
                assert_eq!(fenwick.slow_range(i, j), fenwick.range(i, j));
            }
        }
    }

    #[test]
    fn range_i_larger_than_j() {
        let input = [19, 3, 27, 28, 263, 3897, -4, 27];
        let fenwick = Fenwick::from_values(&input);

        for i in 0..input.len() {
            for j in 0..i {
                assert_eq!(fenwick.slow_range(i, j), fenwick.range(i, j));
            }
        }
    }
}
