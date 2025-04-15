/// Iterator that yields consecutive runs of equal elements from a slice.
///
/// A run is a sequence of consecutive elements that are equal according to
/// [`PartialEq`]. The iterator yields references to subslices containing these runs.
///
/// # Type Parameters
///
/// * `'a` - Lifetime of the referenced slice
/// * `T` - Type of elements in the slice, must implement [`PartialEq`]
///
/// # Examples
///
/// ```
/// use toolbox_rs::run_iterator::RunIterator;
///
/// let array = [1, 1, 2, 2, 2, 3, 3, 1];
/// let runs: Vec<&[i32]> = RunIterator::new(&array).collect();
/// let expected: Vec<&[i32]> = vec![
///     &[1, 1],    // First run of 1s
///     &[2, 2, 2], // Run of 2s
///     &[3, 3],    // Run of 3s
///     &[1],       // Single 1
/// ];
/// assert_eq!(runs, expected);
/// ```
pub struct RunIterator<'a, T> {
    array: &'a [T],
    pos: usize,
}

impl<'a, T: PartialEq> RunIterator<'a, T> {
    /// Creates a new `RunIterator` from a slice.
    ///
    /// # Arguments
    ///
    /// * `array` - Slice to iterate over
    ///
    /// # Examples
    ///
    /// ```
    /// use toolbox_rs::run_iterator::RunIterator;
    ///
    /// let data = vec!["a", "a", "b", "c", "c"];
    /// let iterator = RunIterator::new(&data);
    /// ```
    pub fn new(array: &'a [T]) -> Self {
        RunIterator { array, pos: 0 }
    }
}

impl<'a, T> RunIterator<'a, T> {
    /// Creates a new `RunIterator` with a custom predicate for determining runs.
    ///
    /// The predicate function determines if two consecutive elements belong to the same run.
    /// Elements are grouped into runs as long as the predicate returns `true` for adjacent pairs.
    ///
    /// # Arguments
    ///
    /// * `array` - Slice to iterate over
    /// * `pred` - Predicate function that takes two references and returns bool
    ///
    /// # Examples
    ///
    /// ```
    /// use toolbox_rs::run_iterator::RunIterator;
    ///
    /// // Group numbers by their difference being less than 2
    /// let input = vec![1, 3, 4, 8, 9, 10, 15];
    /// let result: Vec<&[i32]> = RunIterator::new_by(&input, |a, b| b - a <= 2).collect();
    /// assert_eq!(result, vec![&input[0..3], &input[3..6], &input[6..]]);
    ///
    /// // Group strings by their length
    /// let strings = vec!["a", "b", "cd", "ef", "g"];
    /// let runs: Vec<&[&str]> = RunIterator::new_by(&strings, |a, b| a.len() == b.len()).collect();
    /// assert_eq!(runs, vec![&strings[0..2], &strings[2..4], &[strings[4]]]);
    /// ```
    pub fn new_by<F>(array: &'a [T], pred: F) -> RunIteratorBy<'a, T, F>
    where
        F: Fn(&T, &T) -> bool,
    {
        RunIteratorBy {
            array,
            pos: 0,
            pred,
        }
    }
}

/// Iterator that yields runs of elements based on a custom predicate
pub struct RunIteratorBy<'a, T, F> {
    array: &'a [T],
    pos: usize,
    pred: F,
}

impl<'a, T, F> Iterator for RunIteratorBy<'a, T, F>
where
    F: Fn(&T, &T) -> bool,
{
    type Item = &'a [T];

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.array.len() {
            return None;
        }

        let start = self.pos;
        self.pos += 1;

        while self.pos < self.array.len()
            && (self.pred)(&self.array[self.pos - 1], &self.array[self.pos])
        {
            self.pos += 1;
        }

        Some(&self.array[start..self.pos])
    }
}

impl<'a, T: PartialEq> Iterator for RunIterator<'a, T> {
    type Item = &'a [T];

    /// Returns the next run of equal elements.
    ///
    /// # Returns
    ///
    /// * `Some(&[T])` - Reference to the next run of equal elements
    /// * `None` - When iteration is complete
    ///
    /// # Examples
    ///
    /// ```
    /// use toolbox_rs::run_iterator::RunIterator;
    ///
    /// let data = vec![1, 1, 2, 2, 3];
    /// let mut iter = RunIterator::new(&data);
    ///
    /// assert_eq!(iter.next(), Some(&[1, 1][..]));
    /// assert_eq!(iter.next(), Some(&[2, 2][..]));
    /// assert_eq!(iter.next(), Some(&[3][..]));
    /// assert_eq!(iter.next(), None);
    /// ```
    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.array.len() {
            return None;
        }

        let start = self.pos;
        self.pos += 1;

        self.pos += self.array[start + 1..]
            .iter()
            .position(|x| x != &self.array[start])
            .unwrap_or(self.array.len() - start - 1);

        Some(&self.array[start..self.pos])
    }
}

#[cfg(test)]
mod tests {
    use super::RunIterator;
    #[test]
    fn unsorted_runs_tests() {
        let array = [1, 1, 2, 2, 2, 3, 3, 1];
        let run_iter = RunIterator::new(&array);

        let result: Vec<&[i32]> = run_iter.collect();
        let expected: Vec<&[i32]> = vec![&[1; 2], &[2; 3], &[3; 2], &[1; 1]];
        assert_eq!(expected, result);
    }

    #[test]
    fn object_runs() {
        #[derive(Debug)]
        struct SimplePair {
            key: i32,
            _value: i32,
        }

        impl PartialEq for SimplePair {
            fn eq(&self, other: &Self) -> bool {
                self.key == other.key
            }
        }

        let input = vec![
            SimplePair { key: 1, _value: 2 },
            SimplePair { key: 1, _value: 1 },
            SimplePair { key: 21, _value: 1 },
            SimplePair { key: 1, _value: 1 },
        ];

        let run_iter = RunIterator::new(&input);

        let result: Vec<&[SimplePair]> = run_iter.collect();
        assert_eq!(3, result.len());
        let expected = vec![&input[0..2], &input[2..3], &input[3..]];
        assert_eq!(expected, result);
    }

    #[test]
    fn test_custom_predicate() {
        let input = vec![1, 3, 4, 8, 9, 10, 15];
        let result: Vec<&[i32]> = RunIterator::new_by(&input, |a, b| b - a <= 2).collect();
        // The expected runs are:
        // [1, 3, 4], [8, 9, 10], [15]
        assert_eq!(result, vec![&input[0..3], &input[3..6], &input[6..]]);

        let inputs = vec![19, 21, 23, 20, 25, 18];
        let result: Vec<&[i32]> = RunIterator::new_by(&inputs, |a, b| a % 2 == b % 2).collect();
        // The expected runs are:
        // [19, 21, 23], [20], [25], [18]
        assert_eq!(
            result,
            vec![&inputs[0..3], &inputs[3..4], &inputs[4..5], &inputs[5..]]
        );
    }
}
