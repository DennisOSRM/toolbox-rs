use criterion::{black_box, criterion_group, BenchmarkId, Criterion};
use rand::{rng, Rng};
use toolbox_rs::k_way_merge::{MergeEntry, MergeTree};
use toolbox_rs::loser_tree::LoserTree;

/// Creates k sorted sequences of random numbers for benchmarking
fn create_benchmark_data(k: usize, sequence_length: usize) -> Vec<Vec<i32>> {
    let mut rng = rng();
    let mut sequences = Vec::with_capacity(k);

    for _ in 0..k {
        let mut sequence: Vec<i32> = (0..sequence_length)
            .map(|_| rng.random_range(-1000..1000))
            .collect();
        sequence.sort_unstable();
        sequences.push(sequence);
    }

    sequences
}

fn loser_tree_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("loser_tree");

    // Test different numbers of sequences
    for k in [4, 8, 16, 32, 64, 128, 512, 1024] {
        group.bench_with_input(BenchmarkId::new("merge", k), &k, |b, &k| {
            b.iter_with_setup(
                || {
                    // Create k sorted sequences of 1000 elements each
                    let sequences = create_benchmark_data(k, 1000);
                    let mut tree = LoserTree::with_capacity(k);

                    // Initialize with first element from each sequence
                    for (idx, seq) in sequences.iter().enumerate() {
                        if let Some(&first) = seq.first() {
                            tree.push(MergeEntry {
                                item: first,
                                index: idx,
                            });
                        }
                    }

                    (tree, sequences)
                },
                |(mut tree, sequences)| {
                    let mut sequence_positions = vec![1; k];
                    let mut result = Vec::with_capacity(k * 1000);

                    while let Some(entry) = black_box(tree.pop()) {
                        result.push(entry.item);
                        let seq_idx = entry.index;

                        // Push next element from the same sequence
                        if sequence_positions[seq_idx] < sequences[seq_idx].len() {
                            tree.push(MergeEntry {
                                item: sequences[seq_idx][sequence_positions[seq_idx]],
                                index: seq_idx,
                            });
                            sequence_positions[seq_idx] += 1;
                        }
                    }
                    black_box(result)
                },
            )
        });
    }

    group.finish();
}

criterion_group!(loser_tree_benches, loser_tree_benchmark);
