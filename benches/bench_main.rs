use criterion::criterion_main;

mod benchmarks;

criterion_main!(
    benchmarks::great_circle::distances,
    benchmarks::radix_sort::all_sorts
);
