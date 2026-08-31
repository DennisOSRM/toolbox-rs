use std::{
    cmp::max,
    sync::{Arc, atomic::AtomicI32},
};

use itertools::Itertools;
use log::debug;

use crate::{
    edge::{InputEdge, TrivialEdge},
    geometry::FPCoordinate,
    graph::NodeID,
    max_flow::{MaxFlow, ResidualEdgeData},
    renumbering_table::RenumberingTable,
};

const ROTATED_COMPARATORS: [fn(i32, i32) -> i32; 4] = [
    |lat, _ln| -> i32 { lat },
    |_lt, lon| -> i32 { lon },
    |lat, lon| -> i32 { lon + lat },
    |lat, lon| -> i32 { -lon + lat },
];

#[derive(Debug)]
pub enum FlowError {
    AxisOutOfBounds,
    /// the cell does not induce any edge, thus it cannot be cut
    EmptyGraph,
    /// the graph has more nodes than the node ids of a cell can address
    GraphTooLarge,
    String(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Flow {
    pub flow: i32,
    pub balance: f64,
    pub left_ids: Vec<usize>,
    pub right_ids: Vec<usize>,
}

pub fn flow_cmp(a: &Flow, b: &Flow) -> std::cmp::Ordering {
    if a.flow == b.flow {
        // note that a and b are inverted here on purpose:
        // balance is at most 0.5 and the closer the value the more balanced the partitions
        return b.balance.partial_cmp(&a.balance).unwrap();
    }
    a.flow.cmp(&b.flow)
}

/// Computes the inertial flow cut for a given orientation and balance
///
/// # Arguments
///
/// * `index` - which of the (0..4) substeps to execute
/// * `edges` - a list of edges that represents the input graph
/// * `node_id_list` - list of node ids
/// * `coordinates` - immutable slice of coordinates of the graphs nodes
/// * `balance_factor` - balance factor, i.e. how many nodes get contracted
/// * `upper_bound` - a global upperbound to the best inertial flow cut
pub fn sub_step<M: MaxFlow>(
    input_edges: &[TrivialEdge],
    node_id_list: &[usize],
    coordinates: &[FPCoordinate],
    axis: usize,
    balance_factor: f64,
    upper_bound: Arc<AtomicI32>,
) -> Result<Flow, FlowError> {
    debug_assert!(axis < 4);
    debug_assert!(balance_factor > 0.);
    debug_assert!(balance_factor < 0.5);
    debug_assert!(coordinates.len() > 2);

    if axis >= 4 {
        return Err(FlowError::AxisOutOfBounds);
    }
    if input_edges.is_empty() {
        // a cell without edges has no cut. Bailing out here keeps the max-flow
        // solver from being handed an empty graph.
        return Err(FlowError::EmptyGraph);
    }
    if coordinates.len() > u32::MAX as usize {
        // Node ids are narrowed to u32 while sorting the nodes of the cell.
        // Every id addresses a coordinate, so checking the coordinates covers
        // all of them at once.
        return Err(FlowError::GraphTooLarge);
    }

    let comparator = ROTATED_COMPARATORS[axis];
    debug!("[{axis}] sorting cooefficient: {comparator:?}");
    // The iteration proxy list to be sorted. The coordinates vector itself is
    // not touched. Sorting the ids by a key that is looked up in the coordinate
    // list costs a cache miss on every comparison, thus the ids are decorated
    // with their projection and the pairs are sorted instead.
    let mut node_id_list = node_id_list
        .iter()
        .map(|id| {
            let projection = comparator(coordinates[*id].lat, coordinates[*id].lon);
            // the check on the number of coordinates above rules this out
            let id: u32 = (*id).try_into().expect("node id does not fit into u32");
            (projection, id)
        })
        .collect_vec();
    node_id_list.sort_unstable();

    let size_of_contraction = max(1, (node_id_list.len() as f64 * balance_factor) as usize);
    let sources = &node_id_list[0..size_of_contraction];
    let targets = &node_id_list[node_id_list.len() - size_of_contraction..];

    debug_assert!(!sources.is_empty());
    debug_assert!(!targets.is_empty());

    debug!("[{axis}] renumbering of inertial flow graph");
    // let mut renumbering_table = vec![usize::MAX; coordinates.len()];
    let mut renumbering_table =
        RenumberingTable::new_with_size_hint(coordinates.len(), node_id_list.len());
    // nodes in the in the graph have to be numbered consecutively.
    // the mapping is input id -> dinic id

    for (_projection, s) in sources {
        renumbering_table.set(*s as usize, 0);
    }
    for (_projection, t) in targets {
        renumbering_table.set(*t as usize, 1);
    }

    // each thread holds their own copy of the edge set
    let mut edges = input_edges
        .iter()
        .map(|edge| -> InputEdge<ResidualEdgeData> {
            InputEdge::<ResidualEdgeData> {
                source: edge.source as NodeID,
                target: edge.target as NodeID,
                data: ResidualEdgeData::new(1),
            }
        })
        .collect_vec();
    let mut current_id = 2;

    for e in &mut edges {
        // nodes in the in the graph have to be numbered consecutively
        if !renumbering_table.contains_key(e.source) {
            renumbering_table.set(e.source, current_id);
            current_id += 1;
        }
        if !renumbering_table.contains_key(e.target) {
            renumbering_table.set(e.target, current_id);
            current_id += 1;
        }
        e.source = renumbering_table.get(e.source) as NodeID;
        e.target = renumbering_table.get(e.target) as NodeID;
    }
    debug!("[{axis}] instantiating min-cut solver, epsilon 0.25");

    // remove eigenloops especially from contracted regions
    let edge_count_before = edges.len();
    edges.retain(|edge| edge.source != edge.target);
    debug!(
        "[{axis}] eigenloop removal - edge count before {edge_count_before}, after {}",
        edges.len()
    );
    edges.shrink_to_fit();

    debug!("[{axis}] instantiating min-cut solver, epsilon {balance_factor}");
    let mut max_flow_solver = M::from_edge_list(edges, 0, 1);
    debug!("[{axis}] instantiated min-cut solver");
    max_flow_solver.run_with_upper_bound(upper_bound);

    let max_flow = max_flow_solver.max_flow();

    if let Err(message) = max_flow {
        // Error is returned in case the search is aborted early
        return Err(FlowError::String(message));
    }
    let flow = max_flow.expect("max flow computation did not run");

    debug!("[{axis}] computed max flow: {flow}");
    let intermediate_assignment = max_flow_solver
        .assignment(0)
        .expect("max flow computation did not run");

    // TODO: don't copy, but partition in place
    let mut left_ids = Vec::new();
    let mut right_ids = Vec::new();
    // nodes that no edge of the cell is incident to are not part of the flow
    // graph and thus have no assignment
    let mut isolated_ids = Vec::new();
    for (_projection, id) in node_id_list {
        let id = id as usize;
        if !renumbering_table.contains_key(id) {
            isolated_ids.push(id);
        } else if intermediate_assignment[renumbering_table.get(id)] {
            left_ids.push(id);
        } else {
            right_ids.push(id);
        }
    }
    // Isolated nodes are cut off either way, so they join the smaller side.
    // Dropping them here would take them out of the recursion and leave them
    // behind on a level of their own.
    if !isolated_ids.is_empty() {
        debug!("[{axis}] {} isolated nodes in cell", isolated_ids.len());
        if left_ids.len() <= right_ids.len() {
            left_ids.append(&mut isolated_ids);
        } else {
            right_ids.append(&mut isolated_ids);
        }
    }

    debug_assert!(!left_ids.is_empty());
    debug_assert!(!right_ids.is_empty());

    let balance = std::cmp::min(left_ids.len(), right_ids.len()) as f64
        / (left_ids.len() + right_ids.len()) as f64;
    debug!("[{axis}] balance: {balance}");

    Ok(Flow {
        flow,
        balance,
        left_ids,
        right_ids,
    })
}

#[cfg(test)]
mod tests {
    use itertools::Itertools;
    use std::sync::{Arc, atomic::AtomicI32};

    use crate::{
        dinic::Dinic,
        geometry::FPCoordinate,
        inertial_flow::{Flow, TrivialEdge, flow_cmp, sub_step},
    };

    static EDGES: [TrivialEdge; 14] = [
        TrivialEdge {
            source: 0,
            target: 1,
        },
        TrivialEdge {
            source: 1,
            target: 0,
        },
        TrivialEdge {
            source: 0,
            target: 2,
        },
        TrivialEdge {
            source: 2,
            target: 0,
        },
        TrivialEdge {
            source: 1,
            target: 2,
        },
        TrivialEdge {
            source: 2,
            target: 1,
        },
        TrivialEdge {
            source: 2,
            target: 4,
        },
        TrivialEdge {
            source: 4,
            target: 2,
        },
        TrivialEdge {
            source: 3,
            target: 5,
        },
        TrivialEdge {
            source: 5,
            target: 3,
        },
        TrivialEdge {
            source: 4,
            target: 3,
        },
        TrivialEdge {
            source: 3,
            target: 4,
        },
        TrivialEdge {
            source: 4,
            target: 5,
        },
        TrivialEdge {
            source: 5,
            target: 4,
        },
    ];

    static COORDINATES: [FPCoordinate; 6] = [
        FPCoordinate::new(1, 0),
        FPCoordinate::new(2, 1),
        FPCoordinate::new(0, 1),
        FPCoordinate::new(2, 2),
        FPCoordinate::new(0, 2),
        FPCoordinate::new(1, 3),
    ];
    static NODE_ID_LIST: [usize; 6] = [0, 1, 2, 3, 4, 5];

    #[test]
    fn inertial_flow() {
        let upper_bound = Arc::new(AtomicI32::new(6));
        let result = sub_step::<Dinic>(&EDGES, &NODE_ID_LIST, &COORDINATES, 3, 0.25, upper_bound)
            .expect("error should not happen");
        assert_eq!(result.flow, 1);
        assert_eq!(result.balance, 0.5);
        assert_eq!(result.left_ids.len(), 3);
        assert_eq!(result.left_ids, vec![4, 5, 3]);
        assert_eq!(result.right_ids.len(), 3);
        assert_eq!(result.right_ids, vec![2, 0, 1]);
    }

    #[test]
    fn isolated_nodes_stay_in_the_recursion() {
        // node 6 carries a coordinate, but no edge is incident to it
        static COORDINATES_WITH_ISOLATED: [FPCoordinate; 7] = [
            FPCoordinate::new(1, 0),
            FPCoordinate::new(2, 1),
            FPCoordinate::new(0, 1),
            FPCoordinate::new(2, 2),
            FPCoordinate::new(0, 2),
            FPCoordinate::new(1, 3),
            FPCoordinate::new(3, 3),
        ];
        static NODE_ID_LIST_WITH_ISOLATED: [usize; 7] = [0, 1, 2, 3, 4, 5, 6];

        let upper_bound = Arc::new(AtomicI32::new(7));
        let result = sub_step::<Dinic>(
            &EDGES,
            &NODE_ID_LIST_WITH_ISOLATED,
            &COORDINATES_WITH_ISOLATED,
            3,
            0.25,
            upper_bound,
        )
        .expect("error should not happen");

        // no node may be dropped, or it would be left behind on its level
        assert_eq!(result.left_ids.len() + result.right_ids.len(), 7);
        assert!(result.left_ids.contains(&6) || result.right_ids.contains(&6));
    }

    #[test]
    fn cell_without_edges_is_reported() {
        let upper_bound = Arc::new(AtomicI32::new(6));
        let result = sub_step::<Dinic>(&[], &NODE_ID_LIST, &COORDINATES, 0, 0.25, upper_bound);
        assert!(matches!(result, Err(super::FlowError::EmptyGraph)));
    }

    #[test]
    fn inertial_flow_all_indices() {
        let upper_bound = Arc::new(AtomicI32::new(6));
        let result = (0..4)
            .map(|axis| -> Result<_, _> {
                sub_step::<Dinic>(
                    &EDGES,
                    &NODE_ID_LIST,
                    &COORDINATES,
                    axis,
                    0.25,
                    upper_bound.clone(),
                )
            })
            .collect_vec();
        assert_eq!(result.len(), 4);

        for r in &result {
            let r = r.as_ref().expect("error should not happen");
            assert_eq!(r.flow, 1);
            assert_eq!(r.balance, 0.5);
            assert_eq!(r.left_ids.len(), 3);
            assert_eq!(r.right_ids.len(), 3);
        }

        let min_max = result.into_iter().map(|r| r.unwrap()).minmax_by(flow_cmp);
        let (min, max) = min_max.into_option().expect("minmax failed");
        assert_eq!(
            min,
            Flow {
                flow: 1,
                balance: 0.5,
                left_ids: vec![2, 0, 1],
                right_ids: vec![4, 5, 3]
            }
        );
        assert_eq!(
            max,
            Flow {
                flow: 1,
                balance: 0.5,
                left_ids: vec![4, 5, 3],
                right_ids: vec![2, 0, 1]
            }
        );
    }
}
