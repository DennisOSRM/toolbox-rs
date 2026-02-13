use criterion::{BatchSize, BenchmarkId, Criterion, SamplingMode, Throughput, criterion_group};
use rand::{RngExt, distr::StandardUniform};
use toolbox_rs::fenwick::Fenwick;

fn create_scrambled_data(length: usize) -> Vec<i32> {
    let rng = rand::rng();
    rng.sample_iter(StandardUniform).take(length).collect()
}

fn bench_range(c: &mut Criterion) {
    let mut group = c.benchmark_group("Fenwick range queries, brute force");
    group.sampling_mode(SamplingMode::Flat);
    for input_length in [1000, 5_000, 10_000] {
        group.throughput(Throughput::Elements(input_length as u64));
        let fenwick = Fenwick::from_values(&create_scrambled_data(input_length));
        group.bench_function(BenchmarkId::new("Fenwick::range()", input_length), |b| {
            b.iter_batched(
                || fenwick.clone(),
                |fenwick| {
                    for i in 0..input_length {
                        for j in i..input_length {
                            fenwick.range(i, j);
                        }
                    }
                },
                BatchSize::LargeInput,
            )
        });
        group.bench_function(BenchmarkId::new("Fenwick::slow_range", input_length), |b| {
            b.iter_batched(
                || fenwick.clone(),
                |fenwick| {
                    for i in 0..input_length {
                        for j in i..input_length {
                            fenwick.slow_range(i, j);
                        }
                    }
                },
                BatchSize::LargeInput,
            )
        });
    }
}

criterion_group!(all_fenwick, bench_range);
