pub trait ComparisonValue {
    type Integral: Default;
    fn value(&self) -> Self::Integral;
}

macro_rules! comparison_value {
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

// define built-in number types (integers, floats, bool)
comparison_value!(u8);
comparison_value!(u16);
comparison_value!(u32);
comparison_value!(u64);
comparison_value!(u128);
comparison_value!(usize);

comparison_value!(i8);
comparison_value!(i16);
comparison_value!(i32);
comparison_value!(i64);
comparison_value!(i128);
comparison_value!(isize);

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
    let mut threshold = T::Integral::default();
    for item in input {
        if item.value() <= threshold {
            continue;
        }
        top_k.push(item);
        if top_k.len() == 2 * k {
            let (_, median, _) = top_k.select_nth_unstable(k - 1);
            threshold = median.value();
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
        assert_eq!(
            output,
            vec![Hit { score: 1 }, Hit { score: 5 }, Hit { score: 8 },]
        );
    }

    #[test]
    fn top_3_5_i32() {
        let input = [8, 12, 5, 1, 20];
        let output = top_k(input, 3);
        assert_eq!(output, vec![1, 5, 8,]);
    }
}
