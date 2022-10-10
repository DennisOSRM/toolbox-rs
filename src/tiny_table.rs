/// hash table semantics build over an unsorted vector and linear search.
/// This is fast for small data sets with small keys up to dozens of entries.

pub struct TinyTable<K, V> {
    data: Vec<(K, V)>,
}

impl<K: Clone + Copy + PartialEq, V: Clone + Copy> TinyTable<K, V> {
    pub fn contains(&self, k: &K) -> bool {
        self.data.iter().any(|x| -> bool { x.0 == *k })
    }

    pub fn find(&self, k: &K) -> Option<&V> {
        if let Some(entry) = self.data.iter().find(|x| x.0 == *k) {
            return Some(&entry.1);
        }
        None
    }

    pub fn remove(&mut self, k: &K) -> bool {
        if let Some(index) = self.data.iter().position(|value| value.0 == *k) {
            self.data.swap_remove(index);
            return true;
        }
        false
    }

    pub fn find_mut(&self, k: &K) -> Option<&(K, V)> {
        self.data.iter().find(|x| x.0 == *k)
    }

    pub fn insert(&mut self, k: &K, v: &V) -> bool {
        let result = self.remove(k);
        self.data.push((*k, *v));
        result
    }

    pub fn new() -> Self {
        TinyTable { data: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn clear(&mut self) {
        self.data.clear()
    }
}

impl<K: Clone + Copy + PartialEq, V: Clone + Copy> Default for TinyTable<K, V> {
    fn default() -> Self {
        Self::new()
    }
}

// TODO: proper implementation, rather than using unit type that must be explicitly provided
pub type TinySet<K> = TinyTable<K, ()>;

#[cfg(test)]
mod tests {
    use crate::tiny_table::{TinySet, TinyTable};

    #[test]
    fn insert_find_remove_table() {
        let mut table = TinyTable::new();
        assert!(table.is_empty());

        table.insert(&1, &999);
        table.insert(&0, &31337);

        assert_eq!(table.len(), 2);
        assert_eq!(table.find(&1), Some(&999));
        assert_eq!(table.find(&0), Some(&31337));
        assert!(table.contains(&0));
        assert!(table.contains(&1));
        assert!(!table.contains(&2));

        table.remove(&1);
        assert_eq!(table.len(), 1);
        assert_eq!(table.find(&1), None);
        assert_eq!(table.find(&0), Some(&31337));
        assert!(table.contains(&0));
        assert!(!table.contains(&1));
        assert!(!table.contains(&2));

        table.insert(&7, &0xbeef);
        assert_eq!(table.len(), 2);
        assert_eq!(table.find(&1), None);
        assert_eq!(table.find(&7), Some(&0xbeef));
        assert_eq!(table.find(&0), Some(&31337));
        assert!(table.contains(&0));
        assert!(!table.contains(&1));
        assert!(!table.contains(&2));
        assert!(table.contains(&7));

        table.clear();
        assert!(table.is_empty());
    }

    #[test]
    fn insert_find_remove_set() {
        let mut table = TinySet::new();
        assert!(table.is_empty());

        table.insert(&1, &());
        table.insert(&0, &());

        assert_eq!(table.len(), 2);
        assert!(table.find(&1).is_some());
        assert!(table.find(&0).is_some());
        assert!(table.contains(&0));
        assert!(table.contains(&1));
        assert!(!table.contains(&2));

        table.remove(&1);
        assert_eq!(table.len(), 1);
        assert!(table.find(&1).is_none());
        assert!(table.find(&0).is_some());
        assert!(table.contains(&0));
        assert!(!table.contains(&1));
        assert!(!table.contains(&2));

        table.insert(&7, &());
        assert_eq!(table.len(), 2);
        assert!(table.find(&1).is_none());
        assert!(table.find(&7).is_some());
        assert!(table.find(&0).is_some());
        assert!(table.contains(&0));
        assert!(!table.contains(&1));
        assert!(!table.contains(&2));
        assert!(table.contains(&7));

        table.clear();
        assert!(table.is_empty());
    }
}
