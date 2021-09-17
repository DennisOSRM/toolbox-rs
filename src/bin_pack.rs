// next-first heuristic for bin packing
// runs in O(N) and yields a 2-approximation
pub fn bin_pack(items: &[u32], capacity: u32) -> u32 {
    let mut res = 0;
    let mut remaining_capacity = capacity;

    // TODO: return assignment

    for item in items {
        // If this item can't fit in current bin
        if *item > remaining_capacity {
            res += 1;
            remaining_capacity = capacity - item;
        } else {
            remaining_capacity -= item;
        }
    }
    res
}

#[cfg(test)]
mod tests {
    use crate::bin_pack::bin_pack;

    #[test]
    fn instance_g4g() {
        let weight = vec![2, 5, 4, 7, 1, 3, 8];
        assert_eq!(4, bin_pack(&weight, 10));
    }
}
