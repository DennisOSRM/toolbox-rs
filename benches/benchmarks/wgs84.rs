use criterion::{BenchmarkId, Criterion, black_box, criterion_group};
use rand::Rng;
use toolbox_rs::wgs84::{FloatLatitude, lat_to_y, lat_to_y_approx};

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

criterion_group!(wgs_benches, lat_to_y_benchmark);
