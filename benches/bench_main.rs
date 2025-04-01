use criterion::criterion_main;

mod benchmarks;

criterion_main!(
    benchmarks::fenwick::all_fenwick,
    benchmarks::radix_sort::all_sorts,
    benchmarks::great_circle::distances,
    benchmarks::tabulation_hash::tables,
    benchmarks::k_way_merge_iterator::k_way_merge,
    benchmarks::loser_tree::loser_tree,
    benchmarks::polyline::polyline,
    benchmarks::mercator::mercator_benches,
);
