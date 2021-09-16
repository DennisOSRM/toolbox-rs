use std::cmp::Ordering;

#[derive(Debug)]
pub struct UnionFind {
    parent: Vec<u32>,
    rank: Vec<u32>,
}

impl UnionFind {
    pub fn new(max: u32) -> Self {
        Self {
            parent: (0..max).collect(),
            rank: vec![0; max as usize],
        }
    }

    pub fn len(&self) -> usize {
        self.parent.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    // union of two sets represented by x and y.
    pub fn union(&mut self, x: u32, y: u32) {
        let x_set = self.find(x);
        let y_set = self.find(y);

        if x_set == y_set {
            return;
        }

        // merge lower ranked set with higher ranked set
        match self.rank[x_set as usize].cmp(&self.rank[y_set as usize]) {
            Ordering::Less => {
                self.parent[x_set as usize] = y_set;
            }
            Ordering::Greater => {
                self.parent[y_set as usize] = x_set;
            }
            Ordering::Equal => {
                self.parent[y_set as usize] = x_set;
                self.rank[x_set as usize] += 1;
            }
        }
    }

    // find the representative of the set that x is an element of
    pub fn find(&mut self, x: u32) -> u32 {
        let mut p = x;
        while self.parent[p as usize] != p {
            // lazy path compression, set every node to it's parent
            self.parent[p as usize] = self.parent[self.parent[p as usize] as usize];
            p = self.parent[p as usize];
        }
        p
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

        for i in 0..10_u32 {
            assert_eq!(i, uf.find(i));
        }
    }

    #[test]
    fn unions_in_a_row() {
        let mut uf = UnionFind::new(10);
        assert!(!uf.is_empty());
        assert_eq!(10, uf.len());

        for i in 0..10_u32 {
            uf.union(3, i);
        }

        for i in 0..10_u32 {
            // all elements are merged into the representative
            assert_eq!(3, uf.find(i));
        }

        // check that all paths are compressed
        assert_eq!(uf.parent, vec![3, 3, 3, 3, 3, 3, 3, 3, 3, 3]);

        // check that all ranks are 0 except for item '3' it's 1
        assert_eq!(uf.rank, vec![0, 0, 0, 1, 0, 0, 0, 0, 0, 0]);
    }
}
