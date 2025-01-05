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

#[cfg(test)]
mod tests {
    use crate::math::choose;

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
}
