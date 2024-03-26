#[derive(Debug)]
pub struct FenwickTree {
    tree: Vec<usize>,
}

fn lsb(n: usize) -> usize {
    // let two_complement = (n as isize * -1isize) as usize;
    // n & two_complement
    n & (1 << n.trailing_zeros())
}

impl FenwickTree {
    pub fn new_with_size(size: usize) -> Self {
        Self {
            tree: vec![0; size + 1],
        }
    }

    pub fn new_with_values(input: &[usize]) -> Self {
        let mut tree = vec![0; input.len() + 1];
        tree[1..].copy_from_slice(input);

        for index in 1..tree.len() {
            let value = tree[index];
            let parent_index = index + lsb(index);
            if parent_index < tree.len() {
                tree[parent_index] += value;
            }
        }

        Self { tree }
    }

    pub fn update(&mut self, index: usize, increment: usize) {
        let mut index = index + 1;
        while index <= self.tree.len() {
            self.tree[index] += increment;
            index += lsb(index);
        }
    }

    pub fn query(&self, index: usize) -> usize {
        let mut sum = 0;
        let mut index = index + 1;

        while index > 0 {
            sum += self.tree[index];
            index -= lsb(index);
        }

        sum
    }

    pub fn len(&self) -> usize {
        self.tree.len() - 1
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[cfg(test)]
mod tests {
    use rand::prelude::SliceRandom;
    use rand::thread_rng;

    use super::FenwickTree;

    const COUNTS: [usize; 12] = [2, 1, 1, 3, 2, 3, 4, 5, 6, 7, 8, 9];
    #[test]
    fn compute_update_prefix() {
        let mut input = COUNTS.clone();
        let mut fwt = FenwickTree::new_with_size(input.len());
        assert_eq!(fwt.len(), input.len());
        input.iter().enumerate().for_each(|(idx, c)| {
            fwt.update(idx, *c);
        });
        assert_eq!(12, fwt.query(5));

        // process an update
        fwt.update(3, 6);
        input[3] += 6;
        assert_eq!(18, fwt.query(5));

        // check that all prefix sums are ok after update
        let prefix_sums: Vec<usize> = input
            .iter()
            .scan(0, |sum, i| {
                *sum += i;
                Some(*sum)
            })
            .collect();
        prefix_sums.iter().enumerate().for_each(|(index, value)| {
            assert_eq!(*value, fwt.query(index));
        });
    }

    #[test]
    fn construction_correctness() {
        let mut fwt1 = FenwickTree::new_with_size(COUNTS.len());
        COUNTS.into_iter().enumerate().for_each(|(idx, c)| {
            // incrementally construct the tree
            fwt1.update(idx, c);
        });
        assert_eq!(fwt1.len(), COUNTS.len());
        let fwt2 = FenwickTree::new_with_values(&COUNTS);
        assert_eq!(fwt2.len(), COUNTS.len());

        let prefix_sums = COUNTS
            .iter()
            .scan(0, |sum, i| {
                *sum += i;
                Some(*sum)
            })
            .collect::<Vec<_>>();

        prefix_sums.iter().enumerate().for_each(|(index, value)| {
            // assert that the linear time construction yields the same result than the incremental construction
            assert_eq!(*value, fwt1.query(index));
            assert_eq!(*value, fwt2.query(index));
        });
    }

    #[test]
    fn randomized_incremental_construction_correct() {
        let mut indices_vector: Vec<_> = COUNTS
            .iter()
            .enumerate()
            .map(|(index, count)| vec![index; *count])
            .flatten()
            .collect();
        indices_vector.shuffle(&mut thread_rng());

        let mut fwt1 = FenwickTree::new_with_size(COUNTS.len());
        assert_eq!(fwt1.len(), COUNTS.len());
        indices_vector.iter().for_each(|index| {
            // incrementally construct the tree on by one
            fwt1.update(*index, 1);
        });
        let fwt2 = FenwickTree::new_with_values(&COUNTS);
        assert_eq!(fwt2.len(), COUNTS.len());

        let prefix_sums = COUNTS
            .iter()
            .scan(0, |sum, i| {
                *sum += i;
                Some(*sum)
            })
            .collect::<Vec<_>>();

        prefix_sums.iter().enumerate().for_each(|(index, value)| {
            // assert that the linear time construction yields the same result than the incremental construction
            assert_eq!(*value, fwt1.query(index));
            assert_eq!(*value, fwt2.query(index));
        });
    }

    #[test]
    fn randomized_incremental_chunk_construction_correct() {
        let mut indices_vector: Vec<_> = (0..COUNTS.len()).collect();
        indices_vector.shuffle(&mut thread_rng());

        let mut fwt1 = FenwickTree::new_with_size(COUNTS.len());
        assert_eq!(fwt1.len(), COUNTS.len());
        indices_vector.iter().for_each(|index| {
            // incrementally construct the tree by chunk
            fwt1.update(indices_vector[*index], COUNTS[indices_vector[*index]]);
        });
        let fwt2 = FenwickTree::new_with_values(&COUNTS);
        assert_eq!(fwt2.len(), COUNTS.len());

        let prefix_sums = COUNTS
            .iter()
            .scan(0, |sum, i| {
                *sum += i;
                Some(*sum)
            })
            .collect::<Vec<_>>();

        prefix_sums.iter().enumerate().for_each(|(index, value)| {
            // assert that the linear time construction yields the same result than the incremental construction
            assert_eq!(*value, fwt1.query(index));
            assert_eq!(*value, fwt2.query(index));
        });
    }

    #[test]
    fn empty_tree_from_empty_slice() {
        let fwt = FenwickTree::new_with_values(&[]);
        assert!(fwt.is_empty());
        assert_eq!(0, fwt.len());
    }
}
