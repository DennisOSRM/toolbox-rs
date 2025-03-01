use criterion::{Criterion, black_box, criterion_group};
use rand::Rng;
use toolbox_rs::wgs84::{FloatLatitude, lat_to_y, lat_to_y_approx};

pub fn lat_to_y_benchmark_small(c: &mut Criterion) {
    let latitudes = [
        FloatLatitude(0.0),
        FloatLatitude(45.0),
        FloatLatitude(-45.0),
        FloatLatitude(85.0),
        FloatLatitude(-85.0),
        FloatLatitude(70.0),
        FloatLatitude(-70.0),
    ];

    c.bench_function("lat_to_y", |b| {
        b.iter(|| {
            for &lat in latitudes.iter() {
                black_box(lat_to_y(lat));
            }
        })
    });

    c.bench_function("lat_to_y_approx", |b| {
        b.iter(|| {
            for &lat in latitudes.iter() {
                black_box(lat_to_y_approx(lat));
            }
        })
    });
}

pub fn lat_to_y_benchmark_large(c: &mut Criterion) {
    let mut rng = rand::rng();
    let latitudes: Vec<FloatLatitude> = (0..10_000)
        .map(|_| FloatLatitude(rng.random_range(-90.0..90.0)))
        .collect();

    c.bench_function("lat_to_y", |b| {
        b.iter(|| {
            for &lat in latitudes.iter() {
                black_box(lat_to_y(lat));
            }
        })
    });

    c.bench_function("lat_to_y_approx", |b| {
        b.iter(|| {
            for &lat in latitudes.iter() {
                black_box(lat_to_y_approx(lat));
            }
        })
    });
}

criterion_group!(
    wgs_benches,
    lat_to_y_benchmark_small,
    lat_to_y_benchmark_large
);
