use criterion::{Criterion, black_box, criterion_group};
use toolbox_rs::great_circle::*;

pub fn haversine_benchmark(c: &mut Criterion) {
    c.bench_function("haversine", |b| {
        b.iter(|| {
            haversine(
                black_box(50.066389),
                black_box(-5.714722),
                black_box(58.643889),
                black_box(-3.070000),
            )
        })
    });
}

pub fn vincenty_benchmark(c: &mut Criterion) {
    c.bench_function("vincenty", |b| {
        b.iter(|| {
            vincenty(
                black_box(50.066389),
                black_box(-5.714722),
                black_box(58.643889),
                black_box(-3.070000),
            )
        })
    });
}

criterion_group!(distances, haversine_benchmark, vincenty_benchmark);
