use criterion::{criterion_group, BatchSize, BenchmarkId, Criterion, SamplingMode, Throughput};
use rand::{distributions::Standard, Rng};
use toolbox_rs::rdx_sort::radix::Sort;

fn create_scrambled_data(length: usize) -> Vec<i32> {
    let rng = rand::thread_rng();
    rng.sample_iter(Standard).take(length).collect()
}

fn bench_sorts(c: &mut Criterion) {
    let mut group = c.benchmark_group("Sort Algorithms");
    group.sampling_mode(SamplingMode::Flat);
    for i in [
        1, 10, 100, 1000, 10_000, 100_000, 1_000_000, 2_000_000, 5_000_000, 10_000_000,
    ] {
        group.throughput(Throughput::Elements(i as u64));
        let data = create_scrambled_data(i);
        group.bench_function(BenchmarkId::new("std::sort", i), |b| {
            b.iter_batched(
                || data.clone(),
                |mut data| data.sort(),
                BatchSize::LargeInput,
            )
        });
        group.bench_function(BenchmarkId::new("std::sort_unstable", i), |b| {
            b.iter_batched(
                || data.clone(),
                |mut data| data.sort_unstable(),
                BatchSize::LargeInput,
            )
        });
        group.bench_function(BenchmarkId::new("rdx_sort", i), |b| {
            b.iter_batched(
                || data.clone(),
                |mut data| data.rdx_sort(),
                BatchSize::LargeInput,
            )
        });
    }
    group.finish();
}

criterion_group!(all_sorts, bench_sorts,);
