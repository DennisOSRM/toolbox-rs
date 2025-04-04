/// Next-Fit bin packing algorithm
///
/// # Arguments
/// * `items` - Slice of item sizes
/// * `capacity` - Capacity of each bin
///
/// # Returns
/// Tuple containing the number of bins required and a vector of bin assignments
///
/// # Examples
/// ```
/// use toolbox_rs::bin_pack::bin_pack;
/// let items = vec![2, 5, 4, 7, 1, 3, 8];
/// assert_eq!((3, vec![0, 0, 0, 1, 1, 1, 2]), bin_pack(&items, 11).unwrap());
/// ```
// next-first heuristic for bin packing
// runs in O(N) and yields a 2-approximation
pub fn bin_pack_next_fit(items: &[u32], capacity: u32) -> Result<(u32, Vec<u32>), &'static str> {
    if capacity == 0 {
        return Err("Capacity must be greater than 0");
    }

    if items.is_empty() {
        return Ok((0, Vec::new()));
    }

    if items.iter().any(|&x| x > capacity) {
        return Err("Item exceeds bin capacity");
    }

    let mut current_bin = 0;
    let mut remaining_capacity = capacity;
    let mut assignments = vec![0; items.len()];

    for (i, &item) in items.iter().enumerate() {
        // If item doesn't fit in current bin, create a new one
        if item > remaining_capacity {
            current_bin += 1;
            remaining_capacity = capacity;
        }
        assignments[i] = current_bin;
        remaining_capacity -= item;
    }

    Ok((current_bin + 1, assignments))
}

#[cfg(test)]
mod tests {
    use crate::bin_pack::bin_pack_next_fit;

    #[test]
    fn instance_stack_exchange1() {
        let weight = vec![2, 5, 4, 7, 1, 3, 8];
        assert_eq!(
            Ok((3, vec![0, 0, 0, 1, 1, 1, 2])),
            bin_pack_next_fit(&weight, 11)
        );
    }

    #[test]
    fn instance_stack_exchange2() {
        let weight = vec![2, 5, 4, 7, 1, 3, 8];
        assert_eq!(
            Ok((5, vec![0, 0, 1, 2, 2, 3, 4])),
            bin_pack_next_fit(&weight, 10)
        );
    }

    #[test]
    fn empty_input() {
        assert_eq!(Ok((0, Vec::new())), bin_pack_next_fit(&[], 10));
    }

    #[test]
    fn single_item() {
        assert_eq!(Ok((1, vec![0])), bin_pack_next_fit(&[5], 10));
    }

    #[test]
    fn exact_fit() {
        assert_eq!(Ok((2, vec![0, 1])), bin_pack_next_fit(&[10, 10], 10));
    }

    #[test]
    fn multiple_bins_with_remainder() {
        let items = vec![3, 3, 3, 3, 3]; // Should pack into 2 bins: [3,3,3] and [3,3]
        assert_eq!(Ok((2, vec![0, 0, 0, 1, 1])), bin_pack_next_fit(&items, 10));
    }

    #[test]
    fn invalid_capacity() {
        assert_eq!(
            Err("Capacity must be greater than 0"),
            bin_pack_next_fit(&[1, 2, 3], 0)
        );
    }

    #[test]
    fn sequential_packing() {
        let items = vec![4, 4, 4, 4, 4]; // Should pack into 3 bins: [4,4], [4,4], [4]
        assert_eq!(Ok((3, vec![0, 0, 1, 1, 2])), bin_pack_next_fit(&items, 8));
    }
}
