pub mod addressable_binary_heap;
pub mod as_bytes;
pub mod bfs;
pub mod bin_pack;
pub mod bit_weight_iterator;
pub mod bitset_subset_iterator;
pub mod bloom_filter;
pub mod bounding_box;
pub mod cell;
pub mod convex_hull;
pub mod count_min_sketch;
pub mod cycle_check;
pub mod ddsg;
pub mod dfs;
pub mod dimacs;
pub mod dinic;
pub mod dynamic_graph;
pub mod edge;
pub mod edmonds_karp;
pub mod enumerative_source_coding;
pub mod fenwick;
pub mod ford_fulkerson;
pub mod geometry;
pub mod graph;
pub mod great_circle;
pub mod huffman_code;
pub mod inertial_flow;
pub mod io;
pub mod k_way_merge_iterator;
pub mod kruskal;
pub mod level_directory;
pub mod linked_list;
pub mod loser_tree;
pub mod lru;
pub mod math;
pub mod max_flow;
pub mod mercator;
pub mod merge_entry;
pub mod merge_tree;
pub mod metis;
pub mod one_iterator;
pub mod one_to_many_dijkstra;
pub mod partition;
pub mod path_based_scc;
pub mod polyline;
pub mod rdx_sort;
pub mod renumbering_table;
pub mod run_iterator;
pub mod single_linked_list;
pub mod space_filling_curve;
pub mod static_graph;
pub mod tabulation_hash;
pub mod tarjan;
pub mod tiny_table;
pub mod top_k;
pub mod unidirectional_dijkstra;
pub mod union_find;
pub mod unsafe_slice;
pub mod vector_tile;
pub mod wgs84;

#[macro_export]
macro_rules! invoke_macro_for_types {
    ($macro:ident, $($args:ident),*) => {
        $( $macro!($args); )*
    }
}
