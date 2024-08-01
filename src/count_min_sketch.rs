use xxhash_rust::xxh3::xxh3_128_with_seed;

use crate::as_bytes::AsBytes;

fn optimal_k(delta: f64) -> usize {
    (1. / delta).ln().ceil() as usize
}

fn optimal_m(epsilon: f64) -> usize {
    (std::f64::consts::E / epsilon).ceil() as usize
}

fn get_seed() -> u64 {
    rand::Rng::gen::<u64>(&mut rand::thread_rng())
}

pub struct CountMinSketch {
    seed: u64,
    counter: Vec<Vec<u32>>,
    m: usize,
    k: usize,
    len: usize,
}

impl CountMinSketch {
    pub fn new(delta: f64, epsilon: f64) -> Self {
        let seed = get_seed();

        let m = optimal_m(delta);
        let k = optimal_k(epsilon);

        let counter = vec![vec![0u32; m]; k];

        Self {
            seed,
            counter,
            m,
            k,
            len: 0,
        }
    }
}

impl CountMinSketch {
    fn hash_pair<K: Eq + AsBytes>(&self, key: &K) -> (u64, u64) {
        let hash = xxh3_128_with_seed(key.as_bytes(), self.seed);
        (hash as u64, (hash >> 64) as u64)
    }

    fn get_buckets<K: Eq + AsBytes>(&self, key: &K) -> Vec<usize> {
        let (hash1, hash2) = self.hash_pair(key);
        let mut bucket_indices = Vec::with_capacity(self.k);
        if self.k == 1 {
            let index = hash1 % self.m as u64;
            bucket_indices.push(index as usize);
        } else {
            (0..self.k as u64).for_each(|i| {
                let hash = hash1.wrapping_add(i.wrapping_mul(hash2));
                let index = hash % self.m as u64;
                bucket_indices.push(index as usize);
            });
        }
        bucket_indices
    }

    pub fn insert<K: Eq + AsBytes>(&mut self, key: &K) {
        let indices = self.get_buckets(key);
        indices.iter().enumerate().for_each(|(k, &b)| {
            self.counter[k][b] = self.counter[k][b].saturating_add(1);
        });
        self.len += 1;
    }

    pub fn estimate<K: Eq + AsBytes>(&self, key: &K) -> u32 {
        let indices = self.get_buckets(key);
        indices
            .iter()
            .enumerate()
            .map(|(k, b)| self.counter[k][*b])
            .fold(u32::MAX, |a, b| a.min(b))
    }
}

#[cfg(test)]
mod tests {
    use super::CountMinSketch;

    #[test]
    fn insert_check_1m() {
        let mut sketch = CountMinSketch::new(0.01, 0.2);

        for _ in 0..1_000_000 {
            sketch.insert(&"key");
        }

        assert_eq!(sketch.estimate(&"key"), 1_000_000);
        assert_eq!(sketch.estimate(&"blah"), 0);
    }
}
