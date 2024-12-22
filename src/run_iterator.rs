pub struct RunIterator<'a, T> {
    array: &'a [T],
    pos: usize,
}

impl<'a, T: PartialEq> RunIterator<'a, T> {
    pub fn new(array: &'a [T]) -> Self {
        RunIterator { array, pos: 0 }
    }
}

impl<'a, T: PartialEq> Iterator for RunIterator<'a, T> {
    type Item = &'a [T];

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

        let runs = vec![
            SimplePair { key: 1, _value: 2 },
            SimplePair { key: 1, _value: 1 },
            SimplePair { key: 21, _value: 1 },
            SimplePair { key: 1, _value: 1 },
        ];

        let run_iter = RunIterator::new(&runs);

        let result: Vec<&[SimplePair]> = run_iter.collect();
        assert_eq!(3, result.len());
        let expected = vec![&runs[0..2], &runs[2..3], &runs[3..]];
        assert_eq!(expected, result);
    }
}
