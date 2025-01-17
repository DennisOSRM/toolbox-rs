use criterion::criterion_main;

mod benchmarks;

criterion_main!(
    benchmarks::fenwick::all_fenwick,
    benchmarks::great_circle::distances,
    benchmarks::radix_sort::all_sorts
);
