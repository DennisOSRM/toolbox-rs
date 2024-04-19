pub mod addressable_binary_heap;
pub mod bfs;
pub mod bin_pack;
pub mod bloom_filter;
pub mod bounding_box;
pub mod cell;
pub mod convex_hull;
pub mod cycle_check;
pub mod ddsg;
pub mod dfs;
pub mod dimacs;
pub mod dinic;
pub mod dynamic_graph;
pub mod edge;
pub mod edmonds_karp;
pub mod ford_fulkerson;
pub mod geometry;
pub mod graph;
pub mod great_circle;
pub mod huffman_code;
pub mod inertial_flow;
pub mod io;
pub mod kruskal;
pub mod level_directory;
pub mod linked_list;
pub mod lru;
pub mod max_flow;
pub mod metis;
pub mod one_iterator;
pub mod one_to_many_dijkstra;
pub mod partition;
pub mod projection;
pub mod rdx_sort;
pub mod renumbering_table;
pub mod search_space;
pub mod space_filling_curve;
pub mod static_graph;
pub mod tarjan;
pub mod tiny_table;
pub mod top_k;
pub mod unidirectional_dijkstra;
pub mod union_find;
pub mod unsafe_slice;
pub mod wgs84;

#[macro_export]
macro_rules! invoke_macro_for_types {
    ($macro:ident, $($args:ident),*) => {
        $( $macro!($args); )*
    }
}
