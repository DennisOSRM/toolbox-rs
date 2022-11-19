use fxhash::FxHashMap;

enum Implementation {
    Vec(Vec<usize>),
    Map(FxHashMap<usize, usize>),
}

pub struct RenumberingTable {
    table: Implementation,
}

impl RenumberingTable {
    pub fn new_with_size_hint(universe_size: usize, usage_bound: usize) -> Self {
        debug_assert!(universe_size >= usage_bound);

        let factor = universe_size / usage_bound;
        if factor > 8 {
            // the table will filled with at most 12.5% of the number of elements
            return Self {
                table: Implementation::Map(FxHashMap::default()),
            };
        }

        let mut vector = Vec::new();
        vector.resize(universe_size, usize::MAX);
        Self {
            table: Implementation::Vec(vector),
        }
    }

    pub fn set(&mut self, key: usize, value: usize) {
        match &mut self.table {
            Implementation::Vec(vector) => vector[key] = value,
            Implementation::Map(map) => {
                map.insert(key, value);
            }
        }
    }

    pub fn get(&self, key: usize) -> usize {
        match &self.table {
            Implementation::Vec(vector) => vector[key],
            Implementation::Map(map) => *map.get(&key).unwrap(),
        }
    }

    pub fn contains_key(&self, key: usize) -> bool {
        match &self.table {
            Implementation::Vec(vector) => vector[key] != usize::MAX,
            Implementation::Map(map) => map.contains_key(&key),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RenumberingTable;

    #[test]
    pub fn full_universe() {
        let mut table = RenumberingTable::new_with_size_hint(10, 10);
        for i in 0..10 {
            table.set(i, 10 - i);
        }
        for i in 0..10 {
            assert_eq!(10 - i, table.get(i));
        }
    }

    #[test]
    pub fn sparse_universe() {
        let mut table = RenumberingTable::new_with_size_hint(10000, 10);
        for i in 0..10 {
            table.set(1234 + i, i);
        }
        for i in 0..10 {
            assert_eq!(i, table.get(1234 + i));
        }
        for i in 0..1234 {
            assert!(!table.contains_key(i));
        }
        for i in 1234..1244 {
            assert!(table.contains_key(i));
        }
        for i in 1244..10000 {
            assert!(!table.contains_key(i));
        }
    }
}
