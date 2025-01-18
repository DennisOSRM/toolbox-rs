/// Implementation of a Fenwick tree to keep track and rank prefix sums in logarithmic time.
/// cf. Boris Ryabko (1989). "A fast on-line code" (PDF). Soviet Math. Dokl. 39 (3): 533–537
///     Peter M. Fenwick (1994). "A new data structure for cumulative frequency tables". Software: Practice and Experience. 24 (3): 327–336
///
/// Internally the implementation is one-indexed, while externally it is zero-based. This simplifies the implementation.
use num::Integer;

#[derive(Clone, Debug)]
pub struct IndexOutOfRangeError;

#[derive(Clone, Debug)]
pub struct Fenwick<
    T: Integer + std::clone::Clone + Copy + std::ops::AddAssign + std::ops::SubAssign,
> {
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
    pub fn rank(&self, index: usize) -> Option<T> {
        if index >= self.len() {
            return None;
        }
        let mut index = index + 1;
        let mut sum = T::zero();
        while index > 0 {
            sum += self.tree[index];
            index -= Self::largest_power_of_two_divisor(index);
        }
        Some(sum)
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
        while i > j {
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
        self.rank(j).unwrap() - self.rank(i).unwrap()
    }

    /// finds the index whose prefix sum is less or equal to 'value'
    /// returns None in the following case:
    /// value < tree.rank(0), i.e. the prefix sum of the first item is less than the first value
    pub fn select(&self, mut value: T) -> Option<usize> {
        let mut index = 0;
        let mut step = crate::math::prev_power_of_two(self.len());
        while step > 0 {
            if index + step < self.tree.len() && self.tree[index + step] <= value {
                value -= self.tree[index + step];
                index += step;
            }
            step >>= 1;
        }

        // translate internal 1-based indexing to external 0-based indexing
        if index == 0 {
            return None;
        }
        Some(index - 1)
    }
}

#[cfg(test)]
mod tests {
    use super::Fenwick;

    #[test]
    fn piecemeal_construction1() {
        let mut fenwick = Fenwick::with_size(30);
        fenwick.update(0, 10).unwrap();
        assert_eq!(fenwick.rank(0).unwrap(), 10);
        assert_eq!(fenwick.rank(1).unwrap(), 10);
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
            assert_eq!(result[i], fenwick.rank(i).unwrap());
        }
    }

    #[test]
    fn bulk_construction1() {
        let input = [1, 2, 3, 4];
        let result = [1, 3, 6, 10];
        let fenwick = Fenwick::from_values(&input);
        assert_eq!(fenwick.len(), 4);
        for i in 0..input.len() {
            assert_eq!(result[i], fenwick.rank(i).unwrap());
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
            assert_eq!(fenwick1.rank(i), fenwick2.rank(i));
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

    #[test]
    fn rank() {
        struct TestCase {
            value: i32,
            expected_index: Option<usize>,
            expected_value: Option<i32>,
        }

        let input = [19, 3, 27, 28, 263, 3897, 4, 27];
        // gives prefix sums: [19,22,49,77,340,4237,4241,4268]
        let fenwick = Fenwick::from_values(&input);

        let test_cases = [
            TestCase {
                value: 19,
                expected_index: Some(0),
                expected_value: Some(19),
            },
            TestCase {
                value: 18,
                expected_index: None,
                expected_value: None,
            },
            TestCase {
                value: 4233,
                expected_index: Some(4),
                expected_value: Some(340),
            },
            TestCase {
                value: 4237,
                expected_index: Some(5),
                expected_value: Some(4237),
            },
            TestCase {
                value: 4268,
                expected_index: Some(7),
                expected_value: Some(4268),
            },
            TestCase {
                value: 5000,
                expected_index: Some(7),
                expected_value: Some(4268),
            },
        ];

        for test_case in test_cases {
            let index = fenwick.select(test_case.value);
            assert_eq!(test_case.expected_index, index);
            if let Some(index) = index {
                let value = fenwick.rank(index);
                assert_eq!(value, test_case.expected_value);
            } else {
                assert!(test_case.expected_value.is_none());
            }
        }

        // for i in 0..input.len() {
        //     println!("[{i}] = {}", fenwick.rank(i));
        // }

        // println!("select(19)={idx}, value: {value}");
    }
}
