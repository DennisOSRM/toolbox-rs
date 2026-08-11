//! Construction of the residual graph used by the max-flow solvers.
use crate::edge::InputEdge;
use crate::graph::EdgeArrayEntry;
use crate::max_flow::{ResidualArcData, ResidualEdgeData};
use crate::static_graph::StaticGraph;
use core::cmp::max;
use log::debug;

/// Builds the residual graph shared by the max-flow solvers.
///
/// Both solvers have to see the same graph down to the arc order, or a
/// comparison between them measures the construction as much as the algorithm.
pub fn build_residual_graph(
    edge_list: Vec<InputEdge<ResidualEdgeData>>,
) -> StaticGraph<ResidualArcData> {
    debug_assert!(!edge_list.is_empty());
    // The residual graph holds a reverse arc of zero capacity for each input
    // arc. Instead of materializing those and then sorting 2|E| entries, the
    // adjacency array is built directly by a counting sort in O(V + E).
    let number_of_nodes = 1 + edge_list
        .iter()
        .map(|edge| max(edge.source, edge.target))
        .max()
        .expect("edge list is empty");
    debug!("counting degrees of {number_of_nodes} nodes");

    // count the residual degree of each node in node_array[node + 1], then
    // turn the counts into the offsets of the adjacency blocks
    let mut node_array = vec![0_usize; number_of_nodes + 1];
    for edge in &edge_list {
        node_array[edge.source + 1] += 1;
        node_array[edge.target + 1] += 1;
    }
    for i in 1..node_array.len() {
        node_array[i] += node_array[i - 1];
    }

    debug!("scattering {} arcs", 2 * edge_list.len());
    // Scatter the arcs into their blocks. node_array[u] serves as the write
    // cursor of node u and thus ends up pointing at the end of u's block.
    // Each input arc contributes its capacity to the forward arc and to the
    // cached reverse capacity of the reverse arc, which is why the cache
    // comes for free.
    let mut edge_array = vec![
        EdgeArrayEntry {
            target: 0,
            data: ResidualArcData::default()
        };
        2 * edge_list.len()
    ];
    for edge in &edge_list {
        let forward = node_array[edge.source];
        node_array[edge.source] += 1;
        edge_array[forward] = EdgeArrayEntry {
            target: edge.target,
            data: ResidualArcData {
                capacity: edge.data.capacity,
                reverse_capacity: 0,
            },
        };

        let reverse = node_array[edge.target];
        node_array[edge.target] += 1;
        edge_array[reverse] = EdgeArrayEntry {
            target: edge.source,
            data: ResidualArcData {
                capacity: 0,
                reverse_capacity: edge.data.capacity,
            },
        };
    }
    drop(edge_list);

    // each cursor now points at the end of its block, which is the begin of
    // the next one. Shifting by one restores the adjacency array.
    node_array.rotate_right(1);
    node_array[0] = 0;

    debug!("merging parallel arcs");
    // sort each adjacency block by target and merge parallel arcs into a
    // single one that carries the accumulated capacity. Note that this is
    // fine, as we are looking to compute a node partition. Blocks are short,
    // hence sorting them is cheap.
    let mut write = 0;
    let mut begin = 0;
    for node in 0..number_of_nodes {
        let end = node_array[node + 1];
        edge_array[begin..end].sort_unstable_by_key(|entry| entry.target);

        let block_begin = write;
        for read in begin..end {
            if write > block_begin && edge_array[write - 1].target == edge_array[read].target {
                edge_array[write - 1].data.capacity += edge_array[read].data.capacity;
                edge_array[write - 1].data.reverse_capacity +=
                    edge_array[read].data.reverse_capacity;
            } else {
                edge_array[write] = edge_array[read];
                write += 1;
            }
        }
        begin = end;
        node_array[node + 1] = write;
    }
    edge_array.truncate(write);
    edge_array.shrink_to_fit();
    debug!("residual graph has {write} arcs");

    // the DFS stores arc ids in parent_edge as u32, so a larger graph would
    // silently index the wrong arc
    assert!(write <= u32::MAX as usize, "arc ids have to fit into u32");
    StaticGraph::from_adjacency_array(node_array, edge_array)
}
