use std::{
    cmp::max,
    sync::{Arc, atomic::AtomicI32},
};

use crate::{
    edge::{EdgeWithData, InputEdge},
    graph::{EdgeArrayEntry, NodeID},
    static_graph::StaticGraph,
};
use bitvec::vec::BitVec;
use log::debug;

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResidualEdgeData {
    pub capacity: i32,
}

impl ResidualEdgeData {
    pub fn new(capacity: i32) -> ResidualEdgeData {
        ResidualEdgeData { capacity }
    }
}

/// An arc of a residual graph that caches the capacity of its reverse arc. The
/// BFS of a max-flow computation checks the reverse capacity of every arc it
/// relaxes and looking that arc up dominated its run time. Note that caching is
/// free of charge, as the padding of the adjacency array entry is used up.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct ResidualArcData {
    pub capacity: i32,
    pub reverse_capacity: i32,
}

pub trait MaxFlow {
    fn run(&mut self);
    fn run_with_upper_bound(&mut self, bound: Arc<AtomicI32>);
    fn max_flow(&self) -> Result<i32, String>;
    fn assignment(&self, source: NodeID) -> Result<BitVec, String>;
    fn from_edge_list(
        edges: Vec<InputEdge<ResidualEdgeData>>,
        source: NodeID,
        sink: NodeID,
    ) -> Self;
    fn from_generic_edge_list<E: EdgeWithData>(
        input_edges: &[E],
        source: NodeID,
        target: NodeID,
        function: impl Fn(&E) -> ResidualEdgeData,
    ) -> Self
    where
        Self: Sized,
    {
        debug_assert!(!input_edges.is_empty());
        debug!("instantiating max-flow solver");
        let edge_list: Vec<InputEdge<ResidualEdgeData>> = input_edges
            .iter()
            .map(move |edge| InputEdge {
                source: edge.source(),
                target: edge.target(),
                data: function(edge),
            })
            .collect();

        debug!("created {} ff edges", edge_list.len());
        Self::from_edge_list(edge_list, source, target)
    }
}

/// How long an adjacency block may be and still be worth an insertion sort.
const SHORT_BLOCK: usize = 32;

/// Builds the residual graph a max-flow solver runs on.
///
/// The residual graph holds a reverse arc of zero capacity for each input arc.
/// Instead of materializing those and then sorting 2|E| entries, the adjacency
/// array is built directly by a counting sort in O(V + E).
///
/// Each arc caches the capacity of its pair. A backward search checks the
/// reverse capacity of every arc it relaxes, and looking that arc up dominated
/// the run time before it was cached. The cache is free of charge, as the
/// padding of the adjacency array entry is used up.
///
/// Parallel arcs are merged into one carrying the accumulated capacity, which
/// is sound here because what is wanted is a node partition.
///
/// # Panics
///
/// Panics on an empty edge list, or where the arcs do not fit into `u32`.
#[must_use]
pub fn residual_graph_of(
    edge_list: Vec<InputEdge<ResidualEdgeData>>,
) -> StaticGraph<ResidualArcData> {
    debug_assert!(!edge_list.is_empty());
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
    // node_array[u] serves as the write cursor of node u and thus ends up
    // pointing at the end of u's block
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
            target: u32::try_from(edge.target).expect("the graph is too large to hold"),
            data: ResidualArcData {
                capacity: edge.data.capacity,
                reverse_capacity: 0,
            },
        };

        let reverse = node_array[edge.target];
        node_array[edge.target] += 1;
        edge_array[reverse] = EdgeArrayEntry {
            target: u32::try_from(edge.source).expect("the graph is too large to hold"),
            data: ResidualArcData {
                capacity: 0,
                reverse_capacity: edge.data.capacity,
            },
        };
    }
    drop(edge_list);

    // each cursor now points at the end of its block, which is the begin of the
    // next one. Shifting by one restores the adjacency array.
    node_array.rotate_right(1);
    node_array[0] = 0;

    debug!("merging parallel arcs");
    let mut write = 0;
    let mut begin = 0;
    for node in 0..number_of_nodes {
        let end = node_array[node + 1];
        // A node of a road network has a handful of arcs and the residual graph
        // doubles that, so these blocks are five entries long and there are as
        // many of them as there are nodes. A general sort spends most of a block
        // that size working out that it is dealing with a block that size.
        //
        // Not every block though: inertial flow contracts whole ranks of nodes
        // into the source and the sink, whose blocks are a good fraction of the
        // graph, and an insertion sort over those would be quadratic.
        let block = &mut edge_array[begin..end];
        if block.len() <= SHORT_BLOCK {
            for i in 1..block.len() {
                let entry = block[i];
                let mut j = i;
                while j > 0 && block[j - 1].target > entry.target {
                    block[j] = block[j - 1];
                    j -= 1;
                }
                block[j] = entry;
            }
        } else {
            block.sort_unstable_by_key(|entry| entry.target);
        }

        let block_begin = write;
        for read in begin..end {
            if write > block_begin && edge_array[write - 1].target == edge_array[read].target {
                // Checked, since a flow larger than four bytes is one this
                // cannot answer about anyway: max_flow hands back an i32. A cell
                // of a road network carries a capacity of one an arc and comes
                // nowhere near, but this is a builder anything may call.
                let more = edge_array[read].data;
                let held = &mut edge_array[write - 1].data;
                held.capacity = held
                    .capacity
                    .checked_add(more.capacity)
                    .expect("parallel arcs of more capacity than a flow can hold");
                held.reverse_capacity = held
                    .reverse_capacity
                    .checked_add(more.reverse_capacity)
                    .expect("parallel arcs of more capacity than a flow can hold");
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

    // arc ids are held as u32 by the solvers, so a larger graph would silently
    // index the wrong arc
    assert!(write <= u32::MAX as usize, "arc ids have to fit into u32");
    StaticGraph::from_adjacency_array(node_array, edge_array)
}
