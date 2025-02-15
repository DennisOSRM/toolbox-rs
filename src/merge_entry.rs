use std::cmp::Ordering;

#[derive(PartialEq, Eq, Clone, Debug)]
pub struct MergeEntry<T> {
    pub item: T,
    pub index: usize,
}

impl<T: Ord> PartialOrd for MergeEntry<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<T: Ord> Ord for MergeEntry<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        // reverse ordering for a min heap
        other.item.cmp(&self.item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ordering() {
        let e1 = MergeEntry { item: 1, index: 0 };
        let e2 = MergeEntry { item: 2, index: 1 };
        assert!(e1 > e2); // Umgekehrte Ordnung für Min-Heap
    }
}
