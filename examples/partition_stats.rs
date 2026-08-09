//! Reports statistics of a partition file produced by chipper.
//! Usage: partition_stats <partition.bin> <graph.toolbox>
use rustc_hash::FxHashMap;
use toolbox_rs::{io, partition_id::PartitionID};

fn main() {
    let mut args = std::env::args().skip(1);
    let partition_file = args.next().expect("partition file");
    let graph_file = args.next().expect("graph file");

    let ids = io::read_vec_from_file::<PartitionID>(&partition_file);
    let edges = io::read_graph_into_trivial_edges(&graph_file);

    let mut level_histogram = FxHashMap::default();
    for id in &ids {
        *level_histogram.entry(id.level()).or_insert(0usize) += 1;
    }
    let mut cell_sizes = FxHashMap::default();
    for id in &ids {
        *cell_sizes.entry(*id).or_insert(0usize) += 1;
    }

    let cut = edges
        .iter()
        .filter(|edge| ids[edge.source] != ids[edge.target])
        .count();

    let mut sizes = cell_sizes.values().copied().collect::<Vec<_>>();
    sizes.sort_unstable();

    println!("nodes: {}", ids.len());
    println!("edges: {}", edges.len());
    println!("cut edges: {cut}");
    println!("cells: {}", sizes.len());
    println!(
        "cell size min/median/max: {}/{}/{}",
        sizes.first().unwrap(),
        sizes[sizes.len() / 2],
        sizes.last().unwrap()
    );
    let mut levels = level_histogram.into_iter().collect::<Vec<_>>();
    levels.sort_unstable();
    println!("level histogram (level, node count): {levels:?}");

    // checksum that is invariant to nothing: exact partition identity
    let mut hasher = std::hash::DefaultHasher::new();
    use std::hash::{Hash, Hasher};
    ids.hash(&mut hasher);
    println!("exact assignment hash: {:016x}", hasher.finish());
}
