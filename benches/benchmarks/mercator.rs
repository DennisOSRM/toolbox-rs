use criterion::{BenchmarkId, Criterion, criterion_group};
use rand::RngExt;
use std::hint::black_box;
use toolbox_rs::{
    mercator::{lat_to_y, lat_to_y_approx, lon_to_x},
    wgs84::{FloatLatitude, FloatLongitude},
};

pub fn lat_to_y_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("lat_to_y_sizes");
    let sizes = [10, 100, 1000, 10000];
    let mut rng = rand::rng();

    for size in sizes {
        let latitudes: Vec<FloatLatitude> = (0..size)
            .map(|_| FloatLatitude(rng.random_range(-70.0..70.0)))
            .collect();

        group.bench_with_input(BenchmarkId::new("exact", size), &latitudes, |b, lats| {
            b.iter(|| {
                for &lat in lats.iter() {
                    black_box(lat_to_y(lat));
                }
            })
        });

        group.bench_with_input(BenchmarkId::new("approx", size), &latitudes, |b, lats| {
            b.iter(|| {
                for &lat in lats.iter() {
                    black_box(lat_to_y_approx(lat));
                }
            })
        });
    }
    group.finish();
}

pub fn lon_to_x_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("lon_to_x_sizes");
    let sizes = [10, 100, 1000, 10000];
    let mut rng = rand::rng();

    for size in sizes {
        let longitudes: Vec<FloatLongitude> = (0..size)
            .map(|_| FloatLongitude(rng.random_range(-70.0..70.0)))
            .collect();

        group.bench_with_input(BenchmarkId::new("exact", size), &longitudes, |b, lons| {
            b.iter(|| {
                for &lon in lons.iter() {
                    black_box(lon_to_x(lon));
                }
            })
        });
    }
    group.finish();
}
criterion_group!(mercator_benches, lat_to_y_benchmark, lon_to_x_benchmark);
