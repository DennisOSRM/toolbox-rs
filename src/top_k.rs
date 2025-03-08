use crate::invoke_macro_for_types;

pub trait ComparisonValue {
    type Integral: Default;
    fn value(&self) -> Self::Integral;
}

macro_rules! cv {
    // short-hand to add a default ComparisonValue implementation for the
    // given input type. Works with built-in types like integers.
    ($a:ident) => {
        impl ComparisonValue for $a {
            type Integral = $a;

            fn value(&self) -> Self::Integral {
                *self
            }
        }
    };
}

invoke_macro_for_types!(
    cv, u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize
);

pub fn top_k<T: ComparisonValue + Copy + std::cmp::Ord>(
    input: impl IntoIterator<Item = T>,
    k: usize,
) -> Vec<T>
where
    <T as ComparisonValue>::Integral: PartialOrd,
{
    if k == 0 {
        return Vec::new();
    }

    let mut top_k = Vec::with_capacity(2 * k);
    let mut threshold: Option<T::Integral> = None;
    for item in input {
        if let Some(ref t) = threshold {
            if item.value() >= *t {
                continue;
            }
        }
        top_k.push(item);
        if top_k.len() == 2 * k {
            let (_, median, _) = top_k.select_nth_unstable(k - 1);
            threshold = Some(median.value());
            top_k.truncate(k);
        }
    }

    // TODO: consider running select_nth + truncate before sorting (benchmark using criterion)
    top_k.sort_unstable();
    top_k.truncate(k);

    top_k
}

#[cfg(test)]
mod test {
    use super::top_k;
    use crate::top_k::ComparisonValue;

    #[test]
    fn top_3_5_hit() {
        #[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
        pub struct Hit {
            pub score: u64,
        }

        impl ComparisonValue for Hit {
            type Integral = u64;

            fn value(&self) -> Self::Integral {
                self.score
            }
        }

        let input = [
            Hit { score: 8 },
            Hit { score: 12 },
            Hit { score: 5 },
            Hit { score: 1 },
            Hit { score: 20 },
        ];
        let output = top_k(input, 3);
        let expected = [Hit { score: 1 }, Hit { score: 5 }, Hit { score: 8 }];

        output.iter().zip(expected.iter()).for_each(|(a, b)| {
            assert_eq!(a.value(), b.value());
        });
    }

    #[test]
    fn top_3_5_i32() {
        let input = [8, 12, 5, 1, 20];
        let output = top_k(input, 3);
        assert_eq!(output, vec![1, 5, 8,]);
    }

    #[test]
    fn top_3_15_i32() {
        let input = [8, 12, 5, 1, 20, 7, 2, 6, 3, 4, 9, 21, 26, 27, 8];
        let output = top_k(input, 3);
        assert_eq!(output, vec![1, 2, 3,]);
    }

    #[test]
    fn top_0_15_i32() {
        let input = [8, 12, 5, 1, 20, 7, 2, 6, 3, 4, 9, 21, 26, 27, 8];
        let output = top_k(input, 0);
        assert_eq!(output, Vec::<i32>::new());
    }
}
