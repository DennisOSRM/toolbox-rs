use criterion::{black_box, criterion_group, Criterion};
use rand::{seq::SliceRandom, Rng};
use toolbox_rs::k_way_merge::KWayMergeIterator;

fn create_random_runs(s: usize, k: usize) -> Vec<impl Iterator<Item = i32>> {
    let mut rng = rand::rng();
    let mut numbers: Vec<i32> = (0..s as i32).collect();
    numbers.shuffle(&mut rng);

    let mut runs = Vec::new();
    let mut start = 0;
    for _ in 0..k {
        let end = start + rng.random_range(1..=(s - start) / (k - runs.len()));
        let mut run: Vec<i32> = numbers[start..end].to_vec();
        run.sort();
        runs.push(run.into_iter());
        start = end;
    }

    runs
}

fn k_way_merge_benchmark(c: &mut Criterion) {
    let mut list = create_random_runs(1_000_000, 100);

    c.bench_function("k_way_merge", |b| {
        b.iter(|| {
            let k_way_merge = KWayMergeIterator::new(black_box(&mut list));
            let result: Vec<_> = k_way_merge.collect();
            black_box(result);
        })
    });
}
criterion_group!(k_way_merge, k_way_merge_benchmark,);
