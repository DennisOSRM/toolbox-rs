use core::mem;

use crate::invoke_macro_for_types;

pub trait RadixType: Clone + Copy + Default {
    // RadixTypes are sortable by rdx_sort(.)
    const IS_SIGNED: bool;
    // signed data requires special handling in the last round
    fn key(&self, round: usize) -> u8;
    // the key is the radix of size u8, i.e. one byte
}

macro_rules! is_signed {
    // convenience macro to derive which built-in number types are signed since
    // they require special case handling in the final sorting round.
    (i8) => {
        true
    };
    (i16) => {
        true
    };
    (i32) => {
        true
    };
    (i64) => {
        true
    };
    (i128) => {
        true
    };
    (f32) => {
        true
    };
    (f64) => {
        true
    };
    ($_t:ty) => {
        false
    };
}

macro_rules! radix_type {
    // short-hand to add a default RadixType implementation for the
    // given input type. Works with built-in types like integers.
    ($a:ident) => {
        // forward to the general implementation below
        radix_type!($a, $a);
    };
    ($a:ident, $b:ident) => {
        impl RadixType for $a {
            const IS_SIGNED: bool = is_signed!($a);
            fn key(&self, round: usize) -> u8 {
                (*self as $b >> (round << 3)) as u8
            }
        }
    };
}

// define built-in number types (integers, floats, bool) as RadixType
invoke_macro_for_types!(
    radix_type, u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize
);

radix_type!(bool, u8);

impl RadixType for f32 {
    const IS_SIGNED: bool = is_signed!(f32);
    fn key(&self, round: usize) -> u8 {
        // Interpret the bits of a float as if they were an integer.
        // This relies on the floats being in IEEE-754 format to work.
        (self.to_bits() >> (round << 3)) as u8
    }
}

impl RadixType for f64 {
    const IS_SIGNED: bool = is_signed!(f64);
    fn key(&self, round: usize) -> u8 {
        // Interpret the bits of a float as if they were an integer.
        // This relies on the floats being in IEEE-754 format to work.
        (self.to_bits() >> (round << 3)) as u8
    }
}
pub trait Sort {
    fn rdx_sort(&mut self);
}

impl<T: 'static + RadixType> Sort for Vec<T> {
    fn rdx_sort(&mut self) {
        // TODO(dl): Add an explanation of how radix sort works
        let mut output = vec![T::default(); self.len()];
        let rounds = mem::size_of::<T>();

        // implementation of Friend's optimization: Compute all frequencies at once
        let mut histogram_table = Vec::<Vec<usize>>::new();
        histogram_table.resize(rounds, Vec::with_capacity(256));
        for histogram in &mut histogram_table {
            histogram.resize(256, 0);
        }
        self.iter().for_each(|num| {
            for k in 0..rounds {
                let radix = num.key(k);
                unsafe {
                    *histogram_table
                        .get_unchecked_mut(k)
                        .get_unchecked_mut(radix as usize) += 1;
                }
            }
        });

        let mut skip_table = Vec::with_capacity(rounds);
        skip_table.resize(rounds, false);

        for k in 0..rounds {
            // TODO: there must be a more elegant way to do this!

            let mut prev = match T::IS_SIGNED && k == rounds - 1 {
                // add offset to non-negative entries making room for negative ones
                true => histogram_table[k].iter().skip(128).sum(),
                false => 0,
            };

            if T::IS_SIGNED && k == rounds - 1 {
                // last round for signed numbers needs to handle negatives
                // note that the sign-bit is in the MSB

                (0..128).for_each(|i| unsafe {
                    skip_table[k] = skip_table[k]
                        || *histogram_table.get_unchecked_mut(k).get_unchecked_mut(i) == self.len();
                    let temp = *histogram_table.get_unchecked_mut(k).get_unchecked_mut(i);
                    *histogram_table.get_unchecked_mut(k).get_unchecked_mut(i) = prev;
                    prev += temp;
                });
                prev = 0;
                for i in (128..256).rev() {
                    // build prefix sums for negative numbers from the right
                    unsafe {
                        skip_table[k] = skip_table[k]
                            || *histogram_table.get_unchecked_mut(k).get_unchecked_mut(i)
                                == self.len();
                        let temp = *histogram_table.get_unchecked_mut(k).get_unchecked_mut(i);
                        *histogram_table.get_unchecked_mut(k).get_unchecked_mut(i) = prev;
                        prev += temp;
                    }
                }
            } else {
                // a round can be skipped if all entries fall into the same bucket
                skip_table[k] = histogram_table[k][0] == self.len();

                // let mut prev = 0;
                (0..256).for_each(|i| unsafe {
                    skip_table[k] = skip_table[k]
                        || *histogram_table.get_unchecked_mut(k).get_unchecked_mut(i) == self.len();
                    let temp = *histogram_table.get_unchecked_mut(k).get_unchecked_mut(i);
                    *histogram_table.get_unchecked_mut(k).get_unchecked_mut(i) = prev;
                    prev += temp;
                });
            }
        }

        // permutation rounds
        for (k, skip_round) in skip_table.iter().enumerate().take(rounds) {
            if *skip_round {
                // skipping round {k} since all of input falls into exactly one bucket
                continue;
            }

            // place values into their slot
            self.iter().for_each(|num| {
                let radix = num.key(k);
                unsafe {
                    // performance optimization
                    let target = *histogram_table
                        .get_unchecked(k)
                        .get_unchecked(radix as usize);
                    *output.get_unchecked_mut(target) = *num;
                    *histogram_table
                        .get_unchecked_mut(k)
                        .get_unchecked_mut(radix as usize) += 1;
                }
            });

            core::mem::swap(self, &mut output);
        }
    }
}

#[cfg(test)]
mod tests {
    use rand::RngExt;

    use super::Sort;

    #[test]
    fn tenknumbers() {
        let mut rng = rand::rng();
        let mut list = Vec::new();
        (0..10_000).for_each(|_| {
            list.push(rng.random::<u64>());
        });
        list.rdx_sort();
        list.windows(2).for_each(|i| {
            // now verify numbers are in ascending order
            assert!(i[0] <= i[1]);
        });
    }

    #[test]
    fn invertedrun() {
        let mut list: Vec<i32> = (0..10_000).rev().collect();
        list.windows(2).for_each(|i| {
            // verify numbers are in descending order
            assert!(i[0] > i[1]);
        });

        list.rdx_sort();
        // Note: is_sorted has not been stabilized at the time of writing.
        // assert!(list.is_sorted());

        list.windows(2).for_each(|i| {
            // now verify numbers are in ascending order
            assert!(i[0] < i[1]);
        });
    }

    #[test]
    fn sort_bools() {
        // assumes false < true
        let mut bits: Vec<bool> = vec![false, false, false, false, false, true, false, true];
        bits.rdx_sort();
        assert_eq!(
            bits,
            vec![false, false, false, false, false, false, true, true]
        )
    }

    #[test]
    fn sort_f32() {
        let mut bits: Vec<f32> = vec![1.0, 4.0, 3.2415, 0.0, 26.6, 14.32, 1.23, 0.12];
        bits.rdx_sort();
        assert_eq!(bits, vec![0.0, 0.12, 1.0, 1.23, 3.2415, 4.0, 14.32, 26.6])
    }

    #[test]
    fn sort_f64() {
        let mut bits = vec![1.0, 4.0, 3.2415, 0.0, 26.6, 14.32, 1.23, 0.12];
        bits.rdx_sort();
        assert_eq!(bits, vec![0.0, 0.12, 1.0, 1.23, 3.2415, 4.0, 14.32, 26.6])
    }

    #[test]
    fn sort_i32() {
        let mut input = vec![0, 128, -1, 170, 45, 75, 90, -127, 280, -4, 24, 1, 2, 66];
        input.rdx_sort();
        assert_eq!(
            input,
            vec![-127, -4, -1, 0, 1, 2, 24, 45, 66, 75, 90, 128, 170, 280]
        );
    }

    #[test]
    fn sort_i64() {
        let mut input: Vec<i64> = vec![0, 128, -1, 170, 45, 75, 90, -127, 280, -4, 24, 1, 2, 66];
        input.rdx_sort();
        assert_eq!(
            input,
            vec![-127, -4, -1, 0, 1, 2, 24, 45, 66, 75, 90, 128, 170, 280]
        );
    }
}
