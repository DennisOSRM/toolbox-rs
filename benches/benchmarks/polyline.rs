use criterion::{black_box, criterion_group, BenchmarkId, Criterion};
use toolbox_rs::polyline::{decode, encode, encode_simd};

fn bench_polyline(c: &mut Criterion) {
    let mut group = c.benchmark_group("polyline");

    // Testdaten aus den Unit Tests
    let small_path = vec![[38.5, -120.2], [40.7, -120.95], [43.252, -126.453]];
    let small_encoded = "_p~iF~ps|U_ulLnnqC_mqNvxq`@";

    // Größere Testdaten für realistischere Szenarien
    let large_path: Vec<[f64; 2]> = (0..1000)
        .map(|i| {
            let f = i as f64;
            [f / 100.0, -f / 50.0]
        })
        .collect();
    let large_encoded = encode(&large_path, 5);

    // Encode Benchmarks
    group.bench_function("encode/small", |b| b.iter(|| encode(&small_path, 5)));

    group.bench_function("encode/large", |b| b.iter(|| encode(&large_path, 5)));

    // Decode Benchmarks
    group.bench_function("decode/small", |b| b.iter(|| decode(small_encoded, 5)));

    group.bench_function("decode/large", |b| b.iter(|| decode(&large_encoded, 5)));

    // Precision Benchmarks
    for precision in [0, 5, 10] {
        group.bench_with_input(
            BenchmarkId::new("encode/precision", precision),
            &precision,
            |b, &p| b.iter(|| encode(&small_path, p)),
        );
    }

    group.finish();
}

fn bench_polyline_simd_vs_regular(c: &mut Criterion) {
    let mut group = c.benchmark_group("polyline_encode");

    // Verschiedene Datensatzgrößen für den Vergleich
    let sizes = [10, 100, 1000, 10000];

    for size in sizes {
        // Testdaten generieren
        let points: Vec<[f64; 2]> = (0..size)
            .map(|i| {
                let f = i as f64;
                [f / 100.0, -f / 50.0]
            })
            .collect();

        // Benchmark für normale Implementation
        group.bench_with_input(BenchmarkId::new("standard", size), &points, |b, points| {
            b.iter(|| encode(points, 5))
        });

        // Benchmark für SIMD Implementation
        group.bench_with_input(BenchmarkId::new("simd", size), &points, |b, points| {
            b.iter(|| encode_simd(points, 5))
        });
    }

    group.finish();
}

pub fn bench_encode_simd(c: &mut Criterion) {
    let points: Vec<[f64; 2]> = (0..1000)
        .map(|i| [i as f64 * 0.01, i as f64 * -0.02])
        .collect();

    c.bench_function("encode_simd", |b| {
        b.iter(|| encode_simd(black_box(&points), black_box(5)))
    });
}
criterion_group!(
    name = polyline;
    config = Criterion::default()
    .sample_size(100)
    .measurement_time(std::time::Duration::from_secs(5));
    targets = bench_encode_simd, bench_polyline, bench_polyline_simd_vs_regular
);
