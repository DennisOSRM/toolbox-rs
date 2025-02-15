use criterion::{black_box, criterion_group, BenchmarkId, Criterion};
use rand::{seq::SliceRandom, Rng};
use toolbox_rs::k_way_merge_iterator::KWayMergeIterator;

/// Create a list of random runs of numbers.
///
/// # Panics
/// Panics if k == 0 or s == 0
fn create_random_runs(s: usize, k: usize) -> Vec<impl Iterator<Item = i32>> {
    assert!(k > 0, "k must be greater than 0");
    assert!(s > 0, "s must be greater than 0");

    let mut rng = rand::rng();
    let mut numbers: Vec<i32> = (0..s as i32).collect();
    numbers.shuffle(&mut rng);

    let mut runs = Vec::with_capacity(k);
    let mut start = 0;
    (0..k).for_each(|_| {
        let end = start + rng.random_range(1..=(s - start) / (k - runs.len()));
        let mut run: Vec<i32> = numbers[start..end].to_vec();
        run.sort();
        runs.push(run.into_iter());
        start = end;
    });

    runs
}

fn k_way_merge_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("k_way_merge");

    for k in [10, 100, 1000] {
        group.bench_with_input(BenchmarkId::from_parameter(k), &k, |b, &k| {
            b.iter_with_setup(
                || create_random_runs(1_000_000, k),
                |mut list| {
                    let heap = std::collections::BinaryHeap::new();
                    let k_way_merge = KWayMergeIterator::new(black_box(&mut list), heap);
                    black_box(k_way_merge.collect::<Vec<_>>())
                },
            )
        });
    }

    group.finish();
}
criterion_group!(k_way_merge, k_way_merge_benchmark,);
