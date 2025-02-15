use std::cmp::Ordering;

pub struct UnionFind {
    number_of_sets: usize,
    parent: Vec<usize>,
    rank: Vec<usize>,
}

impl UnionFind {
    pub fn new(max: usize) -> Self {
        Self {
            number_of_sets: max,
            parent: (0..max).collect(),
            rank: vec![0; max],
        }
    }

    pub fn len(&self) -> usize {
        self.parent.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    // union of two sets represented by x and y.
    pub fn union(&mut self, x: usize, y: usize) {
        let x_set = self.find(x);
        let y_set = self.find(y);

        if x_set == y_set {
            return;
        }

        // merge lower ranked set with higher ranked set
        match self.rank[x_set].cmp(&self.rank[y_set]) {
            Ordering::Less => {
                self.parent[x_set] = y_set;
            }
            Ordering::Greater => {
                self.parent[y_set] = x_set;
            }
            Ordering::Equal => {
                self.parent[y_set] = x_set;
                self.rank[x_set] += 1;
            }
        }

        self.number_of_sets -= 1;
    }

    // find the representative of the set that x is an element of
    pub fn find(&mut self, x: usize) -> usize {
        let mut p = x;
        while self.parent[p] != p {
            // lazy path compression, set every node to it's parent
            self.parent[p] = self.parent[self.parent[p]];
            p = self.parent[p];
        }
        p
    }

    pub fn number_of_sets(&self) -> usize {
        self.number_of_sets
    }
}

#[cfg(test)]
mod tests {

    use crate::union_find::UnionFind;

    #[test]
    fn default_all_self_parent() {
        let mut uf = UnionFind::new(10);
        assert!(!uf.is_empty());
        assert_eq!(10, uf.len());
        assert_eq!(10, uf.number_of_sets);
        for i in 0..10_usize {
            assert_eq!(i, uf.find(i));
        }
        assert_eq!(10, uf.number_of_sets);
    }

    #[test]
    fn unions_in_a_row() {
        let mut uf = UnionFind::new(10);
        assert!(!uf.is_empty());
        assert_eq!(10, uf.len());
        assert_eq!(10, uf.number_of_sets);

        for i in 0..10_usize {
            uf.union(3, i);
        }

        for i in 0..10_usize {
            // all elements are merged into the representative
            assert_eq!(3, uf.find(i));
        }

        // check that all paths are compressed
        assert_eq!(uf.parent, vec![3, 3, 3, 3, 3, 3, 3, 3, 3, 3]);

        // check that all ranks are 0 except for item '3' it's 1
        assert_eq!(uf.rank, vec![0, 0, 0, 1, 0, 0, 0, 0, 0, 0]);

        // check that all sets have been merged into a single one
        assert_eq!(1, uf.number_of_sets);
    }

    #[test]
    fn test_number_of_sets() {
        let mut uf = UnionFind::new(6);

        // Initial state: 6 separate sets
        assert_eq!(6, uf.number_of_sets());

        // Unite elements 0 and 1 -> 5 sets
        uf.union(0, 1);
        assert_eq!(5, uf.number_of_sets());

        // Unite elements 2 and 3 -> 4 sets
        uf.union(2, 3);
        assert_eq!(4, uf.number_of_sets());

        // Unite elements 0 and 2 (merging two existing sets) -> 3 sets
        uf.union(0, 2);
        assert_eq!(3, uf.number_of_sets());

        // Unite elements already in same set -> still 3 sets
        uf.union(1, 3);
        assert_eq!(3, uf.number_of_sets());

        // Unite remaining elements
        uf.union(4, 5);
        assert_eq!(2, uf.number_of_sets());

        uf.union(0, 4);
        assert_eq!(1, uf.number_of_sets());
    }
}
