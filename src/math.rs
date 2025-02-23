use num::{Integer, PrimInt};

pub fn choose(n: u64, k: u64) -> u64 {
    if k > n {
        return 0;
    }

    if k == 0 || k == n {
        return 1;
    }

    let k = if k > n - k { n - k } else { k };

    let mut result = 1;
    for i in 1..=k {
        result = result * (n - i + 1) / i;
    }

    result
}

/// calculate the largest power of 2 less or equal to n
pub fn prev_power_of_two<T: PrimInt>(n: T) -> T {
    if n == T::zero() {
        return T::zero();
    }
    let leading_zeros = n.leading_zeros() as usize;
    let sizeof = 8 * std::mem::size_of::<T>();
    T::one() << (sizeof - leading_zeros - 1)
}

/// computes the least-significant bit set.
pub fn lsb_index<T: Integer + PrimInt>(n: T) -> Option<u32> {
    if n == T::zero() {
        return None;
    }

    Some(n.trailing_zeros())
}

/// computes the least-significant bit set. Doesn't return the correct answer if the input is zero
pub fn non_zero_lsb_index<T: Integer + PrimInt>(n: T) -> u32 {
    if n == T::zero() {
        return 0;
    }

    n.trailing_zeros()
}

pub fn horner(x: f64, coefficients: &[f64]) -> f64 {
    coefficients.iter().fold(0.0, |acc, &coeff| acc * x + coeff)
}

#[cfg(test)]
mod tests {
    use crate::math::{choose, horner, lsb_index, non_zero_lsb_index, prev_power_of_two};

    #[test]
    fn some_well_known_n_choose_k_values() {
        let test_cases = [
            ((64u64, 1u64), 64u64),
            ((64, 63), 64),
            ((9, 4), 126),
            ((10, 5), 252),
            ((50, 2), 1_225),
            ((5, 2), 10),
            ((10, 4), 210),
            ((37, 17), 15905368710),
            ((52, 5), 2598960),
        ];

        test_cases.into_iter().for_each(|((n, k), expected)| {
            assert_eq!(choose(n, k), expected);
        });
    }

    #[test]
    fn lsb_well_known_values() {
        assert_eq!(lsb_index(0), None);
        assert_eq!(lsb_index(10), Some(1));
        assert_eq!(lsb_index(16), Some(4));
        assert_eq!(lsb_index(255), Some(0));
        assert_eq!(lsb_index(1024), Some(10));
        assert_eq!(lsb_index(72057594037927936_i64), Some(56));
    }

    #[test]
    fn lsb_index_well_known_values() {
        assert_eq!(non_zero_lsb_index(0), 0);
        assert_eq!(non_zero_lsb_index(10), 1);
        assert_eq!(non_zero_lsb_index(16), 4);
        assert_eq!(non_zero_lsb_index(255), 0);
        assert_eq!(non_zero_lsb_index(1024), 10);
        assert_eq!(non_zero_lsb_index(72057594037927936_i64), 56);
    }

    #[test]
    fn largest_power_of_two_less_or_equal() {
        assert_eq!(prev_power_of_two(16_u8), 16);
        assert_eq!(prev_power_of_two(17_i32), 16);
        assert_eq!(prev_power_of_two(0x5555555555555_u64), 0x4000000000000)
    }

    #[test]
    fn test_horner() {
        // Test of polynom: 2x² + 3x + 1
        let coefficients = [1.0, 3.0, 2.0];
        assert_eq!(horner(0.0, &coefficients), 1.0);
        assert_eq!(horner(1.0, &coefficients), 6.0);
        assert_eq!(horner(2.0, &coefficients), 15.0);

        // Test of polynom: x³ - 2x² + 3x - 4
        let coefficients = [-4.0, 3.0, -2.0, 1.0];
        assert!((horner(0.0, &coefficients) - (-4.0)).abs() < 1e-10);
        assert!((horner(1.0, &coefficients) - (-2.0)).abs() < 1e-10);
        assert!((horner(2.0, &coefficients) - 2.0).abs() < 1e-10);

        // Test of empty polynom
        assert_eq!(horner(1.0, &[]), 0.0);

        // Test of constant polynom
        assert_eq!(horner(42.0, &[5.0]), 5.0);
    }
}
